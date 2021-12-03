use crate::error::*;
use crate::state::voter::Voter;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct CloseDepositEntry<'info> {
    #[account(mut, has_one = voter_authority)]
    pub voter: AccountLoader<'info, Voter>,
    pub voter_authority: Signer<'info>,
}

/// Close an empty deposit, allowing it to be reused in the future
pub fn close_deposit_entry(ctx: Context<CloseDepositEntry>, deposit_entry_index: u8) -> Result<()> {
    msg!("--------close_deposit--------");
    let voter = &mut ctx.accounts.voter.load_mut()?;

    require!(
        voter.deposits.len() > deposit_entry_index as usize,
        InvalidDepositEntryIndex
    );
    let d = &mut voter.deposits[deposit_entry_index as usize];
    require!(d.is_used, InvalidDepositEntryIndex);
    require!(d.amount_deposited_native == 0, VotingTokenNonZero);

    // Deposits that have clawback enabled are guaranteed to live until the end
    // of their locking period. That ensures a deposit can't be closed and reopenend
    // with a different locking kind or locking end time before funds are deposited.
    if d.allow_clawback {
        require!(
            d.lockup.end_ts < Clock::get()?.unix_timestamp,
            DepositStillLocked
        );
    }

    d.is_used = false;
    Ok(())
}
