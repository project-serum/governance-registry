use crate::error::*;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token;
use anchor_spl::token::{Token, TokenAccount};

#[derive(Accounts)]
pub struct Clawback<'info> {
    #[account(has_one = realm_authority)]
    pub registrar: AccountLoader<'info, Registrar>,
    pub realm_authority: Signer<'info>,

    // checking the PDA address it just an extra precaution,
    // the other constraints must be exhaustive
    #[account(
        mut,
        seeds = [registrar.key().as_ref(), b"voter".as_ref(), voter.load()?.voter_authority.key().as_ref()],
        bump = voter.load()?.voter_bump,
        has_one = registrar)]
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

    #[account(
        mut,
        associated_token::authority = voter,
        associated_token::mint = destination.mint,
    )]
    pub vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub destination: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
}

impl<'info> Clawback<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::Transfer<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::Transfer {
            from: self.vault.to_account_info(),
            to: self.destination.to_account_info(),
            authority: self.voter.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }
}

/// Claws back locked tokens from a deposit entry.
///
/// `deposit_entry_index`: The index of the deposit entry to claw back tokens on.
///
/// The deposit entry must have been created with `allow_clawback=true`.
///
/// The instruction will always reclaim all locked tokens, while leaving tokens
/// that have already vested in place.
pub fn clawback(ctx: Context<Clawback>, deposit_entry_index: u8) -> Result<()> {
    let locked_amount = {
        // Load the accounts.
        let registrar = &ctx.accounts.registrar.load()?;
        let voter = &mut ctx.accounts.voter.load_mut()?;

        // Governance may forbid clawback which is a form of withdraw, for example when engaged
        // in a vote.
        let token_owner_record = voter.load_token_owner_record(
            &ctx.accounts.token_owner_record.to_account_info(),
            registrar,
        )?;
        token_owner_record.assert_can_withdraw_governing_tokens()?;

        // Get the deposit being clawed back from.
        let deposit_entry = voter.active_deposit_mut(deposit_entry_index)?;
        require!(
            deposit_entry.allow_clawback,
            ErrorCode::ClawbackNotAllowedOnDeposit
        );

        let curr_ts = registrar.clock_unix_timestamp();
        let locked_amount = deposit_entry.amount_locked(curr_ts);

        // Update deposit book keeping.
        require!(
            locked_amount <= deposit_entry.amount_deposited_native,
            InternalProgramError
        );
        deposit_entry.amount_deposited_native -= locked_amount;

        // Now that all locked funds are withdrawn, end the lockup
        let curr_ts = registrar.clock_unix_timestamp();
        deposit_entry.amount_initially_locked_native = 0;
        deposit_entry.lockup = Lockup::new_from_periods(LockupKind::None, curr_ts, curr_ts, 0)?;
        deposit_entry.allow_clawback = false;
        locked_amount
    };

    {
        // Transfer the tokens to withdraw.
        let voter = &mut ctx.accounts.voter.load()?;
        let voter_seeds = voter_seeds!(voter);
        token::transfer(
            ctx.accounts.transfer_ctx().with_signer(&[voter_seeds]),
            locked_amount,
        )?;
    }

    Ok(())
}
