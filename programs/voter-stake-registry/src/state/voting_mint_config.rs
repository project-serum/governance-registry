use crate::error::*;
use anchor_lang::__private::bytemuck::Zeroable;
use anchor_lang::prelude::*;
use std::convert::TryFrom;

const SCALED_FACTOR_BASE: u64 = 1_000_000_000;

/// Exchange rate for an asset that can be used to mint voting rights.
///
/// See documentation of configure_voting_mint for details on how
/// native token amounts convert to vote weight.
#[zero_copy]
#[derive(Default)]
pub struct VotingMintConfig {
    /// Mint for this entry.
    pub mint: Pubkey,

    /// The authority that is allowed to push grants into voters
    pub grant_authority: Pubkey,

    /// Vote weight factor for unlocked deposits, in 1/SCALED_FACTOR_BASE units.
    pub unlocked_scaled_factor: u64,

    /// Maximum vote weight factor for lockups, in 1/SCALED_FACTOR_BASE units.
    pub lockup_scaled_factor: u64,

    /// Number of seconds of lockup needed to reach the maximum lockup bonus.
    pub lockup_saturation_secs: u64,

    /// Number of digits to shift native amounts, applying a 10^digit_shift factor.
    pub digit_shift: i8,

    // Empty bytes for future upgrades.
    pub padding: [u8; 31],
}
const_assert!(std::mem::size_of::<VotingMintConfig>() == 2 * 32 + 3 * 8 + 1 + 31);

impl VotingMintConfig {
    /// Converts an amount in this voting mints's native currency
    /// to the base vote weight (without the deposit or lockup scalings)
    /// by applying the digit_shift factor.
    pub fn base_vote_weight(&self, amount_native: u64) -> Result<u64> {
        let compute = || -> Option<u64> {
            let val = if self.digit_shift < 0 {
                (amount_native as u128).checked_div(10u128.pow((-self.digit_shift) as u32))?
            } else {
                (amount_native as u128).checked_mul(10u128.pow(self.digit_shift as u32))?
            };
            u64::try_from(val).ok()
        };
        compute().ok_or(Error::ErrorCode(ErrorCode::VoterWeightOverflow))
    }

    /// Apply a factor in SCALED_FACTOR_BASE units.
    fn apply_factor(base_vote_weight: u64, factor: u64) -> Result<u64> {
        let compute = || -> Option<u64> {
            u64::try_from(
                (base_vote_weight as u128)
                    .checked_mul(factor as u128)?
                    .checked_div(SCALED_FACTOR_BASE as u128)?,
            )
            .ok()
        };
        compute().ok_or(Error::ErrorCode(ErrorCode::VoterWeightOverflow))
    }

    /// The vote weight a deposit of a number of native tokens should have.
    pub fn unlocked_vote_weight(&self, amount_native: u64) -> Result<u64> {
        Self::apply_factor(
            self.base_vote_weight(amount_native)?,
            self.unlocked_scaled_factor,
        )
    }

    /// The maximum vote weight a number of locked up native tokens can have.
    /// Will be multiplied with a factor between 0 and 1 for the lockup duration.
    pub fn max_lockup_vote_weight(&self, amount_native: u64) -> Result<u64> {
        Self::apply_factor(
            self.base_vote_weight(amount_native)?,
            self.lockup_scaled_factor,
        )
    }

    /// Whether this voting mint is configured.
    pub fn in_use(&self) -> bool {
        self.mint != Pubkey::default()
    }
}

unsafe impl Zeroable for VotingMintConfig {}
