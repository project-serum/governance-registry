use crate::error::*;
use crate::state::lockup::*;
use crate::state::voter::Voter;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct CloseVoter<'info> {
    #[account(mut, has_one = voter_authority, close = sol_destination)]
    pub voter: AccountLoader<'info, Voter>,
    pub voter_authority: Signer<'info>,
    pub sol_destination: UncheckedAccount<'info>,
}

/// Closes the voter account, allowing one to retrieve rent exemption SOL.
/// Only accounts with no remaining deposits can be closed.
pub fn close_voter(ctx: Context<CloseVoter>) -> Result<()> {
    msg!("--------close_voter--------");
    let voter = &ctx.accounts.voter.load()?;
    let amount = voter.deposits.iter().fold(0u64, |sum, d| {
        sum.checked_add(d.amount_deposited_native).unwrap()
    });
    require!(amount == 0, VotingTokenNonZero);
    Ok(())
}
