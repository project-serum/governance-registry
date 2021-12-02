use crate::account::*;
use crate::error::*;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};

#[derive(Accounts)]
#[instruction(idx: u16, mint: Pubkey, rate: u64, decimals: u8)]
pub struct CreateExchangeRate<'info> {
    #[account(mut, has_one = realm_authority)]
    pub registrar: Box<Account<'info, Registrar>>,
    pub realm_authority: Signer<'info>,

    #[account(
        init,
        payer = payer,
        associated_token::authority = registrar,
        associated_token::mint = deposit_mint,
    )]
    pub exchange_vault: Account<'info, TokenAccount>,
    pub deposit_mint: Account<'info, Mint>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Creates a new exchange rate for a given mint. This allows a voter to
/// deposit the mint in exchange for vTokens. There can only be a single
/// exchange rate per mint.
///
/// WARNING: This can be freely called when any of the rates are empty.
///          This should be called immediately upon creation of a Registrar.
pub fn create_exchange_rate(
    ctx: Context<CreateExchangeRate>,
    idx: u16,
    mint: Pubkey,
    rate: u64,
    decimals: u8,
) -> Result<()> {
    msg!("--------create_exchange_rate--------");
    require!(rate > 0, InvalidRate);
    let registrar = &mut ctx.accounts.registrar;
    require!((idx as usize) < registrar.rates.len(), InvalidIndex);
    require!(registrar.rates[idx as usize].rate == 0, RateNotZero);
    registrar.rates[idx as usize] = registrar.new_rate(mint, decimals, rate)?;
    Ok(())
}
