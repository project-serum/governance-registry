use crate::error::*;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount};

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub registrar: Box<Account<'info, Registrar>>,

    // checking the PDA address it just an extra precaution,
    // the other constraints must be exhaustive
    #[account(
        mut,
        seeds = [registrar.key().as_ref(), b"voter".as_ref(), voter_authority.key().as_ref()],
        bump = voter.load()?.voter_bump,
        has_one = registrar,
        has_one = voter_authority,
    )]
    pub voter: AccountLoader<'info, Voter>,
    pub voter_authority: Signer<'info>,

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

impl<'info> Withdraw<'info> {
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
pub fn withdraw(ctx: Context<Withdraw>, deposit_entry_index: u8, amount: u64) -> Result<()> {
    msg!("--------withdraw--------");

    // Forbid voting with already withdraw tokens
    // e.g. flow
    // - update voter_weight_record
    // - withdraw
    // - vote
    let ixns = ctx.accounts.instructions.to_account_info();
    let current_index = tx_instructions::load_current_index_checked(&ixns)? as usize;
    require!(current_index == 0, ErrorCode::ShouldBeTheFirstIxInATx);

    // Load the accounts.
    let registrar = &ctx.accounts.registrar;
    let voter = &mut ctx.accounts.voter.load_mut()?;

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

    // Get the exchange rate for the token being withdrawn.
    let mint_idx = registrar.voting_mint_config_index(ctx.accounts.destination.mint)?;
    require!(
        mint_idx == deposit_entry.voting_mint_config_idx as usize,
        ErrorCode::InvalidMint
    );

    // Bookkeeping for withdrawn funds.
    assert!(amount <= deposit_entry.amount_deposited_native);
    deposit_entry.amount_deposited_native -= amount;

    // Transfer the tokens to withdraw.
    let registrar_seeds = registrar_seeds!(registrar);
    token::transfer(
        ctx.accounts.transfer_ctx().with_signer(&[registrar_seeds]),
        amount,
    )?;

    Ok(())
}
