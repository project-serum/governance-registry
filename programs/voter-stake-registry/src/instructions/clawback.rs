use crate::error::*;
use crate::state::lockup::*;
use crate::state::registrar::registrar_seeds;
use anchor_lang::prelude::*;
use anchor_spl::token;

use super::withdraw::WithdrawOrClawback;

pub fn clawback(ctx: Context<WithdrawOrClawback>, deposit_id: u8) -> Result<()> {
    msg!("--------clawback--------");
    // Load the accounts.
    let registrar = &ctx.accounts.registrar;
    let voter = &mut ctx.accounts.voter.load_mut()?;
    require!(voter.deposits.len() > deposit_id.into(), InvalidDepositId);
    require!(
        ctx.accounts.authority.key() == registrar.clawback_authority,
        InvalidAuthority
    );

    // Get the deposit being withdrawn from.
    let curr_ts = registrar.clock_unix_timestamp();
    let deposit_entry = &mut voter.deposits[deposit_id as usize];
    require!(deposit_entry.is_used, InvalidDepositId);
    require!(
        deposit_entry.allow_clawback,
        ErrorCode::ClawbackNotAllowedOnDeposit
    );
    let unvested_amount =
        deposit_entry.amount_initially_locked_native - deposit_entry.vested(curr_ts).unwrap();

    // sanity check only
    require!(
        deposit_entry.amount_deposited_native >= unvested_amount,
        InsufficientVestedTokens
    );

    // Transfer the tokens to withdraw.
    let registrar_seeds = registrar_seeds!(registrar);
    token::transfer(
        ctx.accounts.transfer_ctx().with_signer(&[registrar_seeds]),
        unvested_amount,
    )?;

    // Update deposit book keeping.
    deposit_entry.amount_deposited_native -= unvested_amount;

    // Now that all locked funds are withdrawn, end the lockup
    deposit_entry.amount_initially_locked_native = 0;
    deposit_entry.lockup.kind = LockupKind::None;
    deposit_entry.lockup.start_ts = registrar.clock_unix_timestamp();
    deposit_entry.lockup.end_ts = deposit_entry.lockup.start_ts;
    deposit_entry.allow_clawback = false;

    Ok(())
}
