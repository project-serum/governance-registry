use crate::error::*;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};
use std::convert::TryFrom;

#[derive(Accounts)]
pub struct CreateDepositEntry<'info> {
    pub registrar: AccountLoader<'info, Registrar>,

    // checking the PDA address it just an extra precaution,
    // the other constraints must be exhaustive
    #[account(
        mut,
        seeds = [registrar.key().as_ref(), b"voter".as_ref(), voter_authority.key().as_ref()],
        bump = voter.load()?.voter_bump,
        has_one = registrar,
        has_one = voter_authority)]
    pub voter: AccountLoader<'info, Voter>,

    #[account(
        init_if_needed,
        associated_token::authority = voter,
        associated_token::mint = deposit_mint,
        payer = voter_authority
    )]
    pub vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub voter_authority: Signer<'info>,

    pub deposit_mint: Box<Account<'info, Mint>>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

/// Creates a new deposit entry.
///
/// Initializes a deposit entry with the requested settings.
/// Will error if the deposit entry is already in use.
///
/// - `deposit_entry_index`: deposit entry to use
/// - `kind`: Type of lockup to use.
/// - `start_ts`: Start timestamp, defaults to current clock.
///    The lockup will end after `start + periods * period_secs()`.
///
///    Note that tokens will already be locked before start_ts, it only defines
///    the vesting start time and the anchor for the periods computation.
///
/// - `periods`: How long to lock up, depending on `kind`. See LockupKind::period_secs()
/// - `allow_clawback`: When enabled, the the clawback_authority is allowed to
///    unilaterally claim locked tokens.
pub fn create_deposit_entry(
    ctx: Context<CreateDepositEntry>,
    deposit_entry_index: u8,
    kind: LockupKind,
    start_ts: Option<u64>,
    periods: u32,
    allow_clawback: bool,
) -> Result<()> {
    // Load accounts.
    let registrar = &ctx.accounts.registrar.load()?;
    let voter = &mut ctx.accounts.voter.load_mut()?;

    // Get the exchange rate entry associated with this deposit.
    let mint_idx = registrar.voting_mint_config_index(ctx.accounts.deposit_mint.key())?;

    // Get and set up the deposit entry.
    require!(
        voter.deposits.len() > deposit_entry_index as usize,
        OutOfBoundsDepositEntryIndex
    );
    let d_entry = &mut voter.deposits[deposit_entry_index as usize];
    require!(!d_entry.is_used, UnusedDepositEntryIndex);

    let start_ts = if let Some(v) = start_ts {
        i64::try_from(v).unwrap()
    } else {
        registrar.clock_unix_timestamp()
    };

    *d_entry = DepositEntry::default();
    d_entry.is_used = true;
    d_entry.voting_mint_config_idx = mint_idx as u8;
    d_entry.amount_deposited_native = 0;
    d_entry.amount_initially_locked_native = 0;
    d_entry.allow_clawback = allow_clawback;
    d_entry.lockup = Lockup::new_from_periods(kind, start_ts, periods)?;

    Ok(())
}
