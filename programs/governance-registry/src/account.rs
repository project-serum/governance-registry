use crate::error::*;
use anchor_lang::__private::bytemuck::Zeroable;
use anchor_lang::prelude::*;
use anchor_spl::vote_weight_record;
use std::convert::TryFrom;

// Generate a VoteWeightRecord Anchor wrapper, owned by the current program.
// VoteWeightRecords are unique in that they are defined by the SPL governance
// program, but they are actually owned by this program.
vote_weight_record!(crate::ID);

/// Seconds in one day.
#[cfg(feature = "localnet")]
pub const SECS_PER_DAY: i64 = 10;
#[cfg(not(feature = "localnet"))]
pub const SECS_PER_DAY: i64 = 86_400;

/// Seconds in one month.
#[cfg(feature = "localnet")]
pub const SECS_PER_MONTH: i64 = 10;
#[cfg(not(feature = "localnet"))]
pub const SECS_PER_MONTH: i64 = 365 * SECS_PER_DAY / 12;

/// Maximum number of days one can lock for.
pub const MAX_DAYS_LOCKED: u64 = 7 * 365;

/// Maximum number of months one can lock for.
pub const MAX_MONTHS_LOCKED: u64 = 7 * 12;

/// Vote weight is amount * FIXED_VOTE_WEIGHT_FACTOR +
/// LOCKING_VOTE_WEIGHT_FACTOR * amount * time / max time
pub const FIXED_VOTE_WEIGHT_FACTOR: u64 = 1;
pub const LOCKING_VOTE_WEIGHT_FACTOR: u64 = 0;

/// Instance of a voting rights distributor.
#[account(zero_copy)]
pub struct Registrar {
    pub governance_program_id: Pubkey,
    pub authority: Pubkey,
    pub realm: Pubkey,
    pub realm_community_mint: Pubkey,
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
}

/// User account for minting voting rights.
#[account(zero_copy)]
pub struct Voter {
    pub authority: Pubkey,
    pub registrar: Pubkey,
    pub voter_bump: u8,
    pub voter_weight_record_bump: u8,
    pub deposits: [DepositEntry; 32],

    /// The most recent slot a deposit was made in.
    ///
    /// Would like to use solana_program::clock::Slot here, but Anchor's IDL
    /// does not know the type.
    pub last_deposit_slot: u64,
}

impl Voter {
    pub fn weight(&self, registrar: &Registrar) -> Result<u64> {
        let curr_ts = registrar.clock_unix_timestamp();
        self.deposits
            .iter()
            .filter(|d| d.is_used)
            .try_fold(0, |sum, d| {
                d.voting_power(&registrar.rates[d.rate_idx as usize], curr_ts)
                    .map(|vp| sum + vp)
            })
    }
}

/// Exchange rate for an asset that can be used to mint voting rights.
#[zero_copy]
#[derive(AnchorSerialize, AnchorDeserialize, Default)]
pub struct ExchangeRateEntry {
    /// Mint for this entry.
    pub mint: Pubkey,

    /// Mint decimals.
    pub mint_decimals: u8,

    /// Exchange rate for 1.0 decimal-respecting unit of mint currency
    /// into the common vote currency.
    ///
    /// Example: If rate=2, then 1.000 of mint currency has a vote weight
    /// of 2.000000 in common vote currency. In the example mint decimals
    /// was 3 and common_decimals was 6.
    pub rate: u64,

    /// Factor for converting mint native currency to common vote currency,
    /// including decimal handling.
    ///
    /// Examples:
    /// - if common and mint have the same number of decimals, this is the same as 'rate'
    /// - common decimals = 6, mint decimals = 3, rate = 5 -> 500
    pub conversion_factor: u64,
}

impl ExchangeRateEntry {
    /// Converts an amount in this ExchangeRateEntry's mint's native currency
    /// to the equivalent common registrar vote currency amount.
    pub fn convert(&self, amount_native: u64) -> u64 {
        amount_native.checked_mul(self.conversion_factor).unwrap()
    }
}

unsafe impl Zeroable for ExchangeRateEntry {}

/// Bookkeeping for a single deposit for a given mint and lockup schedule.
#[zero_copy]
pub struct DepositEntry {
    // True if the deposit entry is being used.
    pub is_used: bool,

    // Points to the ExchangeRate this deposit uses.
    pub rate_idx: u8,

    /// Amount in deposited, in native currency. Withdraws of vested tokens
    /// directly reduce this amount.
    ///
    /// This directly tracks the total amount added by the user. They may
    /// never withdraw more than this amount.
    pub amount_deposited_native: u64,

    /// Amount in locked when the lockup began, in native currency.
    ///
    /// Note that this is not adjusted for withdraws. It is possible for this
    /// value to be bigger than amount_deposited_native after some vesting
    /// and withdrawals.
    ///
    /// This value is needed to compute the amount that vests each peroid,
    /// which should not change due to withdraws.
    pub amount_initially_locked_native: u64,

    // Locked state.
    pub lockup: Lockup,
}

impl DepositEntry {
    /// # Voting Power Caclulation
    ///
    /// Returns the voting power for the deposit, giving locked tokens boosted
    /// voting power that scales linearly with the lockup time.
    ///
    /// For each cliff-locked token, the vote weight is:
    ///
    /// ```
    ///    voting_power = amount * (fixed_factor + locking_factor * time_factor)
    /// ```
    ///
    /// with
    ///    fixed_factor = FIXED_VOTE_WEIGHT_FACTOR
    ///    locking_factor = LOCKING_VOTE_WEIGHT_FACTOR
    ///    time_factor = lockup_time_remaining / max_lockup_time
    ///
    /// Linear vesting schedules can be thought of as a sequence of cliff-
    /// locked tokens and have the matching voting weight.
    ///
    /// To achieve this with the SPL governance program--which requires a "max
    /// vote weight"--we attach what amounts to a scalar multiplier between 0
    /// and 1 to normalize voting power. This multiplier is a function of
    /// the lockup schedule. Here we will describe two, a one time
    /// cliff and a linear vesting schedule unlocking daily.
    ///
    /// ## Cliff Lockup
    ///
    /// The cliff lockup allows one to lockup their tokens for a set period
    /// of time, unlocking all at once on a given date.
    ///
    /// The calculation for this is straightforward and is detailed above.
    ///
    /// ### Decay
    ///
    /// As time passes, the voting power decays until it's back to just
    /// fixed_factor when the cliff has passed. This is important because at
    /// each point in time the lockup should be equivalent to a new lockup
    /// made for the remaining time period.
    ///
    /// ## Daily Vesting Lockup
    ///
    /// Daily vesting can be calculated with simple series sum.
    ///
    /// For the sake of example, suppose we locked up 10 tokens for two days,
    /// vesting linearly once a day. In other words, we have 5 tokens locked for
    /// 1 day and 5 tokens locked for two days.
    ///
    /// Visually, we can see this in a two year timeline
    ///
    /// 0      5      10   amount unlocked
    /// | ---- | ---- |
    /// 0      1      2   days
    ///
    /// Then, to calculate the voting power at any time in the first day, we
    /// have (for a max_lockup_time of 2555 days)
    ///
    /// ```
    /// voting_power =
    ///     5 * (fixed_factor + locking_factor * 1/2555)
    ///     + 5 * (fixed_factor + locking_factor * 2/2555)
    ///   = 10 * fixed_factor
    ///     + 5 * locking_factor * (1 + 2)/2555
    /// ```
    ///
    /// Since 7 years is the maximum lock, and 1 day is the minimum, we have
    /// a time_factor of 1/2555 for a one day lock, 2/2555 for a two day lock,
    /// 2555/2555 for a 7 year lock, and 0 for no lock.
    ///
    /// Let's now generalize this to a daily vesting schedule over N days.
    /// Let "amount" be the total amount for vesting. Then the total voting
    /// power to start is
    ///
    /// ```
    /// voting_power =
    ///   = amount * fixed_factor
    ///     + amount/N * locking_factor * (1 + 2 + ... + N)/2555
    /// ```
    ///
    /// ### Decay
    ///
    /// With every vesting one of the summands in the time term disappears
    /// and the remaining locking time for others decreases. That means after
    /// m days, the remaining voting power is
    ///
    /// ```
    /// voting_power =
    ///   = amount * fixed_factor
    ///     + amount/N * locking_factor * (1 + 2 + ... + (N - m))/2555
    /// ```
    ///
    /// Example: After N-1 days, only a 1/Nth fraction of the initial amount
    /// is still locked up and the rest has vested. And that amount has
    /// a time factor of 1/2555.
    ///
    /// The computation below uses 1 + 2 + ... + n = n * (n + 1) / 2.
    pub fn voting_power(&self, rate: &ExchangeRateEntry, curr_ts: i64) -> Result<u64> {
        let fixed_contribution = rate
            .convert(self.amount_deposited_native)
            .checked_mul(FIXED_VOTE_WEIGHT_FACTOR)
            .unwrap();
        if LOCKING_VOTE_WEIGHT_FACTOR == 0 {
            return Ok(fixed_contribution);
        }

        let max_locked_contribution = rate.convert(self.amount_initially_locked_native);
        Ok(fixed_contribution
            + self
                .voting_power_locked(curr_ts, max_locked_contribution)?
                .checked_mul(LOCKING_VOTE_WEIGHT_FACTOR)
                .unwrap())
    }

    /// Vote contribution from locked funds only, not scaled by
    /// LOCKING_VOTE_WEIGHT_FACTOR yet.
    fn voting_power_locked(&self, curr_ts: i64, max_contribution: u64) -> Result<u64> {
        if curr_ts < self.lockup.start_ts || curr_ts >= self.lockup.end_ts {
            return Ok(0);
        }
        match self.lockup.kind {
            LockupKind::None => Ok(0),
            LockupKind::Daily => self.voting_power_linear_vesting(curr_ts, max_contribution),
            LockupKind::Monthly => self.voting_power_linear_vesting(curr_ts, max_contribution),
            LockupKind::Cliff => self.voting_power_cliff(curr_ts, max_contribution),
        }
    }

    fn voting_power_linear_vesting(&self, curr_ts: i64, max_contribution: u64) -> Result<u64> {
        let max_periods = self.lockup.kind.max_periods();
        let periods_left = self.lockup.periods_left(curr_ts)?;
        let periods_total = self.lockup.periods_total()?;

        if periods_left == 0 {
            return Ok(0);
        }

        // TODO: Switch the decay interval to be seconds, not days. That means each
        // of the period cliff-locked deposits here will decay in vote power over the
        // period. That complicates the computaton here, but makes it easier to do
        // the right thing if the period_secs() aren't a multiple of a day.
        //
        // This computes
        // amount / periods_total * (1 + 2 + ... + periods_left) / max_periods
        // See the comment on voting_power().
        let decayed_vote_weight = max_contribution
            .checked_mul(
                // Ok to divide by two here because, if n is zero, then the
                // voting power is zero. And if n is one or above, then the
                // numerator is 2 or above.
                periods_left.checked_mul(periods_left.checked_add(1).unwrap())
                    .unwrap()
                    .checked_div(2)
                    .unwrap(),
            )
            .unwrap()
            .checked_div(max_periods.checked_mul(periods_total).unwrap())
            .unwrap();

        Ok(decayed_vote_weight)
    }

    fn voting_power_cliff(&self, curr_ts: i64, max_contribution: u64) -> Result<u64> {
        // TODO: Decay by the second, not by the day.
        let decayed_voting_weight = self
            .lockup
            .periods_left(curr_ts)?
            .checked_mul(max_contribution)
            .unwrap()
            .checked_div(self.lockup.kind.max_periods())
            .unwrap();

        Ok(decayed_voting_weight)
    }

    /// Returns the amount of unlocked tokens for this deposit--in native units
    /// of the original token amount (not scaled by the exchange rate).
    pub fn vested(&self, curr_ts: i64) -> Result<u64> {
        if curr_ts < self.lockup.start_ts {
            return Ok(0);
        }
        if curr_ts >= self.lockup.end_ts {
            return Ok(self.amount_initially_locked_native);
        }
        match self.lockup.kind {
            LockupKind::None => Ok(self.amount_initially_locked_native),
            LockupKind::Daily => self.vested_linearly(curr_ts),
            LockupKind::Monthly => self.vested_linearly(curr_ts),
            LockupKind::Cliff => Ok(0),
        }
    }

    fn vested_linearly(&self, curr_ts: i64) -> Result<u64> {
        let period_current = self.lockup.period_current(curr_ts)?;
        let periods_total = self.lockup.periods_total()?;
        if period_current >= periods_total {
            return Ok(self.amount_initially_locked_native);
        }
        let vested = self
            .amount_initially_locked_native
            .checked_mul(period_current)
            .unwrap()
            .checked_div(periods_total)
            .unwrap();
        Ok(vested)
    }

    /// Returns the amount that may be withdrawn given current vesting
    /// and previous withdraws.
    pub fn amount_withdrawable(&self, curr_ts: i64) -> u64 {
        let still_locked = self
            .amount_initially_locked_native
            .checked_sub(self.vested(curr_ts).unwrap())
            .unwrap();
        self.amount_deposited_native
            .checked_sub(still_locked)
            .unwrap()
    }
}

#[zero_copy]
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct Lockup {
    pub kind: LockupKind,
    // Start of the lockup.
    pub start_ts: i64,
    // End of the lockup.
    pub end_ts: i64,
    // Empty bytes for future upgrades.
    // TODO: what kinds of upgrades do we foresee?
    pub padding: [u8; 16],
}

impl Lockup {
    /// Returns the number of periods left on the lockup.
    pub fn periods_left(&self, curr_ts: i64) -> Result<u64> {
        Ok(self
            .periods_total()?
            .saturating_sub(self.period_current(curr_ts)?))
    }

    /// Returns the current period in the vesting schedule.
    pub fn period_current(&self, curr_ts: i64) -> Result<u64> {
        let period_secs = self.kind.period_secs();
        if period_secs == 0 {
            return Ok(0);
        }
        let d = u64::try_from({
            let secs_elapsed = curr_ts.saturating_sub(self.start_ts);
            secs_elapsed.checked_div(period_secs).unwrap()
        })
        .map_err(|_| ErrorCode::UnableToConvert)?;
        Ok(d)
    }

    /// Returns the total amount of periods in the lockup period.
    pub fn periods_total(&self) -> Result<u64> {
        let period_secs = self.kind.period_secs();
        if period_secs == 0 {
            return Ok(0);
        }

        // Number of seconds in the entire lockup.
        let lockup_secs = self.end_ts.checked_sub(self.start_ts).unwrap();
        require!(lockup_secs % period_secs == 0, InvalidLockupPeriod);

        // Total periods in the entire lockup.
        Ok(u64::try_from(lockup_secs.checked_div(period_secs).unwrap()).unwrap())
    }
}

#[repr(u8)]
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, Copy)]
pub enum LockupKind {
    None,
    Daily,
    Monthly,
    Cliff,
}

impl LockupKind {
    pub fn period_secs(&self) -> i64 {
        match self {
            LockupKind::None => 0,
            LockupKind::Daily => SECS_PER_DAY,
            LockupKind::Monthly => SECS_PER_MONTH,
            LockupKind::Cliff => SECS_PER_DAY, // arbitrary choice
        }
    }

    pub fn max_periods(&self) -> u64 {
        match self {
            LockupKind::None => 0,
            LockupKind::Daily => MAX_DAYS_LOCKED,
            LockupKind::Monthly => MAX_MONTHS_LOCKED,
            LockupKind::Cliff => MAX_DAYS_LOCKED,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn days_left_start() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 10,
            days_total: 10.0,
            curr_day: 0.0,
        })
    }

    #[test]
    pub fn days_left_one_half() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 10,
            days_total: 10.0,
            curr_day: 0.5,
        })
    }

    #[test]
    pub fn days_left_one() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 9,
            days_total: 10.0,
            curr_day: 1.0,
        })
    }

    #[test]
    pub fn days_left_one_and_one_half() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 9,
            days_total: 10.0,
            curr_day: 1.5,
        })
    }

    #[test]
    pub fn days_left_9() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 1,
            days_total: 10.0,
            curr_day: 9.0,
        })
    }

    #[test]
    pub fn days_left_9_dot_one() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 1,
            days_total: 10.0,
            curr_day: 9.1,
        })
    }

    #[test]
    pub fn days_left_9_dot_nine() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 1,
            days_total: 10.0,
            curr_day: 9.9,
        })
    }

    #[test]
    pub fn days_left_ten() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 0,
            days_total: 10.0,
            curr_day: 10.0,
        })
    }

    #[test]
    pub fn days_left_eleven() -> Result<()> {
        run_test_days_left(TestDaysLeft {
            expected_days_left: 0,
            days_total: 10.0,
            curr_day: 11.0,
        })
    }

    #[test]
    pub fn months_left_start() -> Result<()> {
        run_test_months_left(TestMonthsLeft {
            expected_months_left: 10,
            months_total: 10.0,
            curr_month: 0.,
        })
    }

    #[test]
    pub fn months_left_one_half() -> Result<()> {
        run_test_months_left(TestMonthsLeft {
            expected_months_left: 10,
            months_total: 10.0,
            curr_month: 0.5,
        })
    }

    #[test]
    pub fn months_left_one_and_a_half() -> Result<()> {
        run_test_months_left(TestMonthsLeft {
            expected_months_left: 9,
            months_total: 10.0,
            curr_month: 1.5,
        })
    }

    #[test]
    pub fn months_left_ten() -> Result<()> {
        run_test_months_left(TestMonthsLeft {
            expected_months_left: 9,
            months_total: 10.0,
            curr_month: 1.5,
        })
    }

    #[test]
    pub fn months_left_eleven() -> Result<()> {
        run_test_months_left(TestMonthsLeft {
            expected_months_left: 0,
            months_total: 10.0,
            curr_month: 11.,
        })
    }

    #[test]
    pub fn voting_power_cliff_warmup() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        run_test_voting_power(TestVotingPower {
            expected_voting_power: 0, // 0 warmup.
            amount_deposited,
            days_total: 10.0,
            curr_day: -0.5,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_start() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = (10 * amount_deposited) / MAX_DAYS_LOCKED;
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 0.5,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_one_third_day() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = (10 * amount_deposited) / MAX_DAYS_LOCKED;
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 0.33,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_half_day() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = (10 * amount_deposited) / MAX_DAYS_LOCKED;
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 0.5,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_two_thirds_day() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = (10 * amount_deposited) / MAX_DAYS_LOCKED;
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 0.66,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_one_day() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = (9 * amount_deposited) / MAX_DAYS_LOCKED;
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 1.0,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_one_day_one_third() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = (9 * amount_deposited) / MAX_DAYS_LOCKED;
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 1.33,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_two_days() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        // (8/2555) * deposit w/ 6 decimals.
        let expected_voting_power = (8 * amount_deposited) / MAX_DAYS_LOCKED;
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 2.0,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_nine_dot_nine_days() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = amount_deposited / MAX_DAYS_LOCKED;
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 9.9,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_ten_days() -> Result<()> {
        run_test_voting_power(TestVotingPower {
            expected_voting_power: 0, // (0/MAX_DAYS_LOCKED) * deposit w/ 6 decimals.
            amount_deposited: 10 * 1_000_000,
            days_total: 10.0,
            curr_day: 10.0,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_ten_dot_one_days() -> Result<()> {
        run_test_voting_power(TestVotingPower {
            expected_voting_power: 0, // (0/MAX_DAYS_LOCKED) * deposit w/ 6 decimals.
            amount_deposited: 10 * 1_000_000, // 10 tokens with 6 decimals.
            days_total: 10.0,
            curr_day: 10.1,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_cliff_eleven_days() -> Result<()> {
        run_test_voting_power(TestVotingPower {
            expected_voting_power: 0, // (0/MAX_DAYS_LOCKED) * deposit w/ 6 decimals.
            amount_deposited: 10 * 1_000_000, // 10 tokens with 6 decimals.
            days_total: 10.0,
            curr_day: 10.1,
            kind: LockupKind::Cliff,
        })
    }

    #[test]
    pub fn voting_power_daily_warmup() -> Result<()> {
        run_test_voting_power(TestVotingPower {
            expected_voting_power: 0, // (0/MAX_DAYS_LOCKED) * deposit w/ 6 decimals.
            amount_deposited: 10 * 1_000_000, // 10 tokens with 6 decimals.
            days_total: 10.0,
            curr_day: -1.0,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_start() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 0, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 0.0,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_one_half() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 0, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 0.5,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_one() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 1, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 1.0,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_one_and_one_third() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 1, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 1.3,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_two() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 2, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 2.0,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_nine() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 9, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 9.0,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_nine_dot_nine() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 9, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 9.9,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_ten() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 10, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 10.0,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_ten_dot_one() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 10, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 10.1,
            kind: LockupKind::Daily,
        })
    }

    #[test]
    pub fn voting_power_daily_eleven() -> Result<()> {
        // 10 tokens with 6 decimals.
        let amount_deposited = 10 * 1_000_000;
        let expected_voting_power = locked_daily_power(amount_deposited, 11, 10);
        run_test_voting_power(TestVotingPower {
            expected_voting_power,
            amount_deposited,
            days_total: 10.0,
            curr_day: 11.0,
            kind: LockupKind::Daily,
        })
    }

    struct TestDaysLeft {
        expected_days_left: u64,
        days_total: f64,
        curr_day: f64,
    }

    struct TestMonthsLeft {
        expected_months_left: u64,
        months_total: f64,
        curr_month: f64,
    }

    struct TestVotingPower {
        amount_deposited: u64,
        days_total: f64,
        curr_day: f64,
        expected_voting_power: u64,
        kind: LockupKind,
    }

    fn run_test_days_left(t: TestDaysLeft) -> Result<()> {
        let start_ts = 1634929833;
        let end_ts = start_ts + days_to_secs(t.days_total);
        let curr_ts = start_ts + days_to_secs(t.curr_day);
        let l = Lockup {
            kind: LockupKind::Cliff,
            start_ts,
            end_ts,
            padding: [0u8; 16],
        };
        let days_left = l.periods_left(curr_ts)?;
        assert_eq!(days_left, t.expected_days_left);
        Ok(())
    }

    fn run_test_months_left(t: TestMonthsLeft) -> Result<()> {
        let start_ts = 1634929833;
        let end_ts = start_ts + months_to_secs(t.months_total);
        let curr_ts = start_ts + months_to_secs(t.curr_month);
        let l = Lockup {
            kind: LockupKind::Monthly,
            start_ts,
            end_ts,
            padding: [0u8; 16],
        };
        let months_left = l.periods_left(curr_ts)?;
        assert_eq!(months_left, t.expected_months_left);
        Ok(())
    }

    fn run_test_voting_power(t: TestVotingPower) -> Result<()> {
        let start_ts = 1634929833;
        let end_ts = start_ts + days_to_secs(t.days_total);
        let d = DepositEntry {
            is_used: true,
            rate_idx: 0,
            amount_deposited_native: t.amount_deposited,
            amount_initially_locked_native: t.amount_deposited,
            lockup: Lockup {
                start_ts,
                end_ts,
                kind: t.kind,
                padding: [0u8; 16],
            },
        };
        let curr_ts = start_ts + days_to_secs(t.curr_day);
        let power = d.voting_power_locked(curr_ts, t.amount_deposited)?;
        assert_eq!(power, t.expected_voting_power);
        Ok(())
    }

    fn days_to_secs(days: f64) -> i64 {
        let d = (SECS_PER_DAY as f64) * days;
        d.round() as i64
    }

    fn months_to_secs(months: f64) -> i64 {
        let d = (SECS_PER_MONTH as f64) * months;
        d.round() as i64
    }

    // Calculates locked voting power. Done iteratively as a sanity check on
    // the closed form calcuation.
    //
    // deposit - the amount locked up
    // day - the current day in the lockup period
    // total_days - the number of days locked up
    fn locked_daily_power(amount: u64, day: u64, total_days: u64) -> u64 {
        if day >= total_days {
            return 0;
        }
        let days_remaining = total_days - day;
        let mut total = 0f64;
        for k in 1..=days_remaining {
            // We have 'days_remaining' remaining cliff-locked deposits of
            // amount / total_days each. Each of these deposits gets a scaling
            // of k / MAX_DAYS_LOCKED.
            total += (k as f64 * amount as f64) / (MAX_DAYS_LOCKED as f64 * total_days as f64)
        }
        total.floor() as u64
    }
}
