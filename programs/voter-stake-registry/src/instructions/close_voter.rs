use crate::error::*;
use crate::state::*;
use crate::ErrorCode::SerializationError;
use anchor_lang::prelude::*;
use anchor_spl::token::TokenAccount;
use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::io::Write;

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
pub fn close_voter<'key, 'accounts, 'remaining, 'info>(
    ctx: Context<'key, 'accounts, 'remaining, 'info, CloseVoter<'info>>,
) -> Result<()> {
    let voter = &ctx.accounts.voter.load()?;
    let amount = voter.deposits.iter().fold(0u64, |sum, d| {
        sum.checked_add(d.amount_deposited_native).unwrap()
    });
    require!(amount == 0, VotingTokenNonZero);

    for account in &mut ctx.remaining_accounts.iter() {
        let token = Account::<'info, TokenAccount>::try_from(&account.clone()).unwrap();
        require!(token.owner == ctx.accounts.voter.key(), InvalidAuthority);

        close(&account, &ctx.accounts.sol_destination.to_account_info())?;
        account.exit(ctx.program_id);
    }

    Ok(())
}

/// Copy pasta from anchor_lang::common since its private
pub fn close<'info>(
    info: &AccountInfo<'info>,
    sol_destination: &AccountInfo<'info>,
) -> ProgramResult {
    // Transfer tokens from the account to the sol_destination.
    **sol_destination.lamports.borrow_mut() = sol_destination
        .lamports()
        .checked_add(info.lamports())
        .unwrap();
    **info.lamports.borrow_mut() = 0u64;

    // Mark the account discriminator as closed.
    let mut data = info.try_borrow_mut_data()?;
    let dst: &mut [u8] = &mut data;
    let mut cursor = std::io::Cursor::new(dst);
    cursor
        .write_all(&[255, 255, 255, 255, 255, 255, 255, 255])
        .map_err(|_| SerializationError)?;
    Ok(())
}
