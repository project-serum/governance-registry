use crate::error::*;
use crate::state::*;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct CloseVoter<'info> {
    // checking the PDA address it just an extra precaution,
    // the other constraints must be exhaustive
    #[account(
        mut,
        seeds = [voter.load()?.registrar.key().as_ref(), b"voter".as_ref(), voter_authority.key().as_ref()],
        bump = voter.load()?.voter_bump,
        has_one = voter_authority,
        close = sol_destination)]
    pub voter: AccountLoader<'info, Voter>,
    pub voter_authority: Signer<'info>,
    pub sol_destination: UncheckedAccount<'info>,
}

/// Closes the voter account, allowing one to retrieve rent exemption SOL.
/// Only accounts with no remaining deposits can be closed.
pub fn close_voter(ctx: Context<CloseVoter>) -> Result<()> {
    let voter = &ctx.accounts.voter.load()?;
    let amount = voter.deposits.iter().fold(0u64, |sum, d| {
        sum.checked_add(d.amount_deposited_native).unwrap()
    });
    require!(amount == 0, VotingTokenNonZero);
    Ok(())
}
