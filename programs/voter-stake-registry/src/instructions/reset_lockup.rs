use crate::error::*;
use crate::state::registrar::Registrar;
use crate::state::voter::Voter;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct ResetLockup<'info> {
    pub registrar: Box<Account<'info, Registrar>>,

    #[account(mut, has_one = voter_authority, has_one = registrar)]
    pub voter: AccountLoader<'info, Voter>,
    pub voter_authority: Signer<'info>,
}

/// Resets a lockup to start at the current slot timestamp and to last for
/// `periods`, which must be >= the number of periods left on the lockup.
/// This will re-lock any non-withdrawn vested funds.
pub fn reset_lockup(ctx: Context<ResetLockup>, deposit_id: u8, periods: i64) -> Result<()> {
    msg!("--------reset_lockup--------");
    let registrar = &ctx.accounts.registrar;
    let voter = &mut ctx.accounts.voter.load_mut()?;
    require!(voter.deposits.len() > deposit_id as usize, InvalidDepositId);

    let d = &mut voter.deposits[deposit_id as usize];
    require!(d.is_used, InvalidDepositId);

    // The lockup period can only be increased.
    let curr_ts = registrar.clock_unix_timestamp();
    require!(
        periods as u64 >= d.lockup.periods_left(curr_ts)?,
        InvalidDays
    );

    // TODO: Check for correctness
    d.amount_initially_locked_native = d.amount_deposited_native;

    d.lockup.start_ts = curr_ts;
    d.lockup.end_ts = curr_ts
        .checked_add(periods.checked_mul(d.lockup.kind.period_secs()).unwrap())
        .unwrap();

    Ok(())
}
