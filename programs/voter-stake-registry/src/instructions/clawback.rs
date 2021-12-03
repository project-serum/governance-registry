use crate::error::*;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token;

use super::withdraw::WithdrawOrClawback;

/// Claws back locked tokens from a deposit entry.
///
/// `deposit_entry_index`: The index of the deposit entry to claw back tokens on.
///
/// The deposit entry must have been created with `allow_clawback=true`.
///
/// The instruction will always reclaim all locked tokens, while leaving tokens
/// that have already vested in place.
pub fn clawback(ctx: Context<WithdrawOrClawback>, deposit_entry_index: u8) -> Result<()> {
    msg!("--------clawback--------");
    // Load the accounts.
    let registrar = &ctx.accounts.registrar;
    let voter = &mut ctx.accounts.voter.load_mut()?;
    let deposit_entry = voter.active_deposit_mut(deposit_entry_index)?;
    require!(
        ctx.accounts.authority.key() == registrar.clawback_authority,
        InvalidAuthority
    );
    require!(
        deposit_entry.allow_clawback,
        ErrorCode::ClawbackNotAllowedOnDeposit
    );

    let curr_ts = registrar.clock_unix_timestamp();
    let locked_amount = deposit_entry.amount_locked(curr_ts);

    // sanity check only
    require!(
        deposit_entry.amount_deposited_native >= locked_amount,
        InsufficientVestedTokens
    );

    // Transfer the tokens to withdraw.
    let registrar_seeds = registrar_seeds!(registrar);
    token::transfer(
        ctx.accounts.transfer_ctx().with_signer(&[registrar_seeds]),
        locked_amount,
    )?;

    // Update deposit book keeping.
    deposit_entry.amount_deposited_native -= locked_amount;

    // Now that all locked funds are withdrawn, end the lockup
    deposit_entry.amount_initially_locked_native = 0;
    deposit_entry.lockup.kind = LockupKind::None;
    deposit_entry.lockup.start_ts = registrar.clock_unix_timestamp();
    deposit_entry.lockup.end_ts = deposit_entry.lockup.start_ts;
    deposit_entry.allow_clawback = false;

    Ok(())
}
