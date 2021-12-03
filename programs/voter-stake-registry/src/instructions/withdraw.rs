use crate::error::*;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount};

#[derive(Accounts)]
pub struct WithdrawOrClawback<'info> {
    pub registrar: Box<Account<'info, Registrar>>,

    #[account(mut, has_one = registrar)]
    pub voter: AccountLoader<'info, Voter>,

    /// The token_owner_record for the voter_authority. This is needed
    /// to be able to forbid withdraws while the voter is engaged with
    /// a vote or has an open proposal.
    ///
    /// token_owner_record is validated in the instruction:
    /// - owned by registrar.governance_program_id
    /// - for the registrar.realm
    /// - for the registrar.realm_governing_token_mint
    /// - governing_token_owner is voter_authority
    pub token_owner_record: UncheckedAccount<'info>,

    /// The authority that allows the withdraw/clawback.
    ///
    /// For withdraw: must be voter.voter_authority
    /// For clawback: must be registrar.clawback_authority
    ///
    /// The address is verified in the instructions.
    pub authority: Signer<'info>,

    #[account(
        mut,
        associated_token::authority = registrar,
        associated_token::mint = destination.mint,
    )]
    pub vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub destination: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
}

impl<'info> WithdrawOrClawback<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::Transfer<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::Transfer {
            from: self.vault.to_account_info(),
            to: self.destination.to_account_info(),
            authority: self.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }
}

/// Withdraws tokens from a deposit entry, if they are unlocked according
/// to the deposit's vesting schedule.
///
/// `deposit_entry_index`: The deposit entry to withdraw from.
/// `amount` is in units of the native currency being withdrawn.
pub fn withdraw(
    ctx: Context<WithdrawOrClawback>,
    deposit_entry_index: u8,
    amount: u64,
) -> Result<()> {
    msg!("--------withdraw--------");
    // Load the accounts.
    let registrar = &ctx.accounts.registrar;
    let voter = &mut ctx.accounts.voter.load_mut()?;
    require!(
        ctx.accounts.authority.key() == voter.voter_authority,
        InvalidAuthority
    );

    // Governance may forbid withdraws, for example when engaged in a vote.
    let token_owner_record = voter.load_token_owner_record(
        &ctx.accounts.token_owner_record.to_account_info(),
        registrar,
    )?;
    token_owner_record.assert_can_withdraw_governing_tokens()?;

    // Must not withdraw in the same slot as depositing, to prevent people
    // depositing, having the vote weight updated, withdrawing and then
    // voting.
    require!(
        voter.last_deposit_slot < Clock::get()?.slot,
        ErrorCode::InvalidToDepositAndWithdrawInOneSlot
    );

    // Get the deposit being withdrawn from.
    let curr_ts = registrar.clock_unix_timestamp();
    let deposit_entry = voter.active_deposit_mut(deposit_entry_index)?;
    require!(
        deposit_entry.amount_withdrawable(curr_ts) >= amount,
        InsufficientVestedTokens
    );
    // technically unnecessary
    require!(
        deposit_entry.amount_deposited_native >= amount,
        InsufficientVestedTokens
    );

    // Get the exchange rate for the token being withdrawn.
    let er_idx = registrar.exchange_rate_index_for_mint(ctx.accounts.destination.mint)?;
    require!(
        er_idx == deposit_entry.rate_idx as usize,
        ErrorCode::InvalidMint
    );

    // Update deposit book keeping.
    deposit_entry.amount_deposited_native -= amount;

    // Transfer the tokens to withdraw.
    let registrar_seeds = registrar_seeds!(registrar);
    token::transfer(
        ctx.accounts.transfer_ctx().with_signer(&[registrar_seeds]),
        amount,
    )?;

    Ok(())
}
