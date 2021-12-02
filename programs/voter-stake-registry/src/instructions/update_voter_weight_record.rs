use crate::error::*;
use crate::state::lockup::*;
use crate::state::registrar::Registrar;
use crate::state::voter::Voter;
use anchor_lang::prelude::*;

pub const VOTER_WEIGHT_RECORD: [u8; 19] = *b"voter-weight-record";

#[derive(Accounts)]
pub struct UpdateVoterWeightRecord<'info> {
    pub registrar: Box<Account<'info, Registrar>>,

    #[account(has_one = registrar)]
    pub voter: AccountLoader<'info, Voter>,

    #[account(
        mut,
        seeds = [VOTER_WEIGHT_RECORD.as_ref(), registrar.key().as_ref(), voter.load()?.voter_authority.key().as_ref()],
        bump = voter.load()?.voter_weight_record_bump,
        constraint = voter_weight_record.realm == registrar.realm,
        constraint = voter_weight_record.governing_token_owner == voter.load()?.voter_authority,
        constraint = voter_weight_record.governing_token_mint == registrar.realm_governing_token_mint,
    )]
    pub voter_weight_record: Account<'info, VoterWeightRecord>,

    pub system_program: Program<'info, System>,
}

/// Calculates the lockup-scaled, time-decayed voting power for the given
/// voter and writes it into a `VoteWeightRecord` account to be used by
/// the SPL governance program.
///
/// This "revise" instruction should be called in the same transaction,
/// immediately before voting.
pub fn update_voter_weight_record(ctx: Context<UpdateVoterWeightRecord>) -> Result<()> {
    msg!("--------update_voter_weight_record--------");
    let registrar = &ctx.accounts.registrar;
    let voter = ctx.accounts.voter.load()?;
    let record = &mut ctx.accounts.voter_weight_record;
    record.voter_weight = voter.weight(&registrar)?;
    record.voter_weight_expiry = Some(Clock::get()?.slot);

    Ok(())
}
