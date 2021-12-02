use crate::error::*;
use crate::state::deposit_entry::{FIXED_VOTE_WEIGHT_FACTOR, LOCKING_VOTE_WEIGHT_FACTOR};
use crate::state::registrar::Registrar;
use anchor_lang::prelude::*;
use anchor_spl::token::Mint;

// Remaining accounts should all the token mints that have registered
// exchange rates.
#[derive(Accounts)]
pub struct UpdateMaxVoteWeight<'info> {
    pub registrar: Box<Account<'info, Registrar>>,
    // TODO: SPL governance has not yet implemented this.
    pub max_vote_weight_record: UncheckedAccount<'info>,
}

/// Calculates the max vote weight for the registry. This is a function
/// of the total supply of all exchange rate mints, converted into a
/// common currency with a common number of decimals.
///
/// Note that this method is only safe to use if the cumulative supply for
/// all tokens fits into a u64 *after* converting into common decimals, as
/// defined by the registrar's `rate_decimal` field.
pub fn update_max_vote_weight<'info>(ctx: Context<UpdateMaxVoteWeight>) -> Result<()> {
    msg!("--------update_max_vote_weight--------");
    let registrar = &ctx.accounts.registrar;
    let _max_vote_weight = {
        let total: Result<u64> = ctx
            .remaining_accounts
            .iter()
            .map(|acc| Account::<Mint>::try_from(acc))
            .collect::<std::result::Result<Vec<Account<Mint>>, ProgramError>>()?
            .iter()
            .try_fold(0u64, |sum, m| {
                let er_idx = registrar
                    .rates
                    .iter()
                    .position(|r| r.mint == m.key())
                    .ok_or(ErrorCode::ExchangeRateEntryNotFound)?;
                let er_entry = registrar.rates[er_idx];
                let amount = er_entry.convert(m.supply);
                let total = sum.checked_add(amount).unwrap();
                Ok(total)
            });
        total?
            .checked_mul(FIXED_VOTE_WEIGHT_FACTOR + LOCKING_VOTE_WEIGHT_FACTOR)
            .unwrap()
    };
    // TODO: SPL governance has not yet implemented this feature.
    //       When it has, probably need to write the result into an account,
    //       similar to VoterWeightRecord.
    Ok(())
}
