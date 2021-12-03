use crate::error::*;
use crate::state::exchange_entry::ExchangeRateEntry;
use anchor_lang::prelude::*;

/// Instance of a voting rights distributor.
#[account]
#[derive(Default)]
pub struct Registrar {
    pub governance_program_id: Pubkey,
    pub realm: Pubkey,
    pub realm_governing_token_mint: Pubkey,
    pub realm_authority: Pubkey,
    pub clawback_authority: Pubkey,
    pub bump: u8,
    // The length should be adjusted for one's use case.
    pub rates: [ExchangeRateEntry; 2],

    /// The decimals to use when converting deposits into a common currency.
    ///
    /// This must be larger or equal to the max of decimals over all accepted
    /// token mints.
    pub vote_weight_decimals: u8,

    /// Debug only: time offset, to allow tests to move forward in time.
    pub time_offset: i64,
}

impl Registrar {
    pub fn new_rate(
        &self,
        mint: Pubkey,
        mint_decimals: u8,
        rate: u64,
    ) -> Result<ExchangeRateEntry> {
        require!(self.vote_weight_decimals >= mint_decimals, InvalidDecimals);
        let decimal_diff = self
            .vote_weight_decimals
            .checked_sub(mint_decimals)
            .unwrap();
        Ok(ExchangeRateEntry {
            mint,
            rate,
            mint_decimals,
            conversion_factor: rate.checked_mul(10u64.pow(decimal_diff.into())).unwrap(),
        })
    }

    pub fn clock_unix_timestamp(&self) -> i64 {
        Clock::get().unwrap().unix_timestamp + self.time_offset
    }

    pub fn exchange_rate_index_for_mint(&self, mint: Pubkey) -> Result<usize> {
        self.rates
            .iter()
            .position(|r| r.mint == mint)
            .ok_or(Error::ErrorCode(ErrorCode::ExchangeRateEntryNotFound))
    }
}

#[macro_export]
macro_rules! registrar_seeds {
    ( $registrar:expr ) => {
        &[
            $registrar.realm.as_ref(),
            b"registrar".as_ref(),
            $registrar.realm_governing_token_mint.as_ref(),
            &[$registrar.bump],
        ]
    };
}

pub use registrar_seeds;
