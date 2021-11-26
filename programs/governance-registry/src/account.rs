use crate::error::*;
use anchor_lang::__private::bytemuck::Zeroable;
use anchor_lang::prelude::*;
use anchor_spl::vote_weight_record;
use std::convert::TryFrom;

// Generate a VoteWeightRecord Anchor wrapper, owned by the current program.
// VoteWeightRecords are unique in that they are defined by the SPL governance
// program, but they are actaully owned by this program.
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
pub const SECS_PER_MONTH: i64 = 86_400 * 30;

/// Maximum number of days one can lock for.
pub const MAX_DAYS_LOCKED: u64 = 2555;

/// Maximum number of months one can lock for.
pub const MAX_MONTHS_LOCKED: u64 = 2555;

/// Instance of a voting rights distributor.
#[account(zero_copy)]
pub struct Registrar {
    pub authority: Pubkey,
    pub realm: Pubkey,
    pub realm_community_mint: Pubkey,
    pub bump: u8,
    // The length should be adjusted for one's use case.
    pub rates: [ExchangeRateEntry; 2],
    // The decimals to use when converting deposits into a common currency.
    pub rate_decimals: u8,
}

impl Registrar {
    /// Converts the given amount into the common registrar currency--applying
    /// both the exchange rate and a decimal update.
    ///
    /// The "common regsitrar currency" is the unit used to calculate voting
    /// weight.
    pub fn convert(&self, er: &ExchangeRateEntry, amount: u64) -> Result<u64> {
        require!(self.rate_decimals >= er.decimals, InvalidDecimals);
        let decimal_diff = self.rate_decimals.checked_sub(er.decimals).unwrap();
        let convert = amount
            .checked_mul(er.rate)
            .unwrap()
            .checked_mul(10u64.pow(decimal_diff.into()))
            .unwrap();
        Ok(convert)
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
}

impl Voter {
    pub fn weight(&self) -> Result<u64> {
        let curr_ts = Clock::get()?.unix_timestamp;
        self.deposits
            .iter()
            .filter(|d| d.is_used)
            .try_fold(0, |sum, d| d.voting_power(curr_ts).map(|vp| sum + vp))
    }
}

/// Exchange rate for an asset that can be used to mint voting rights.
#[zero_copy]
#[derive(AnchorSerialize, AnchorDeserialize, Default)]
pub struct ExchangeRateEntry {
    // Mint for this entry.
    pub mint: Pubkey,
    // Exchange rate into the common currency.
    pub rate: u64,
    // Mint decimals.
    pub decimals: u8,
}

unsafe impl Zeroable for ExchangeRateEntry {}

/// Bookkeeping for a single deposit for a given mint and lockup schedule.
#[zero_copy]
pub struct DepositEntry {
    // True if the deposit entry is being used.
    pub is_used: bool,

    // Points to the ExchangeRate this deposit uses.
    pub rate_idx: u8,

    // Amount in the native currency deposited.
    pub amount_deposited: u64,

    // Amount withdrawn from the deposit in the native currency.
    pub amount_withdrawn: u64,

    // Amount in the native currency deposited, scaled by the exchange rate.
    pub amount_scaled: u64,

    // Locked state.
    pub lockup: Lockup,
}

impl DepositEntry {
    /// # Voting Power Caclulation
    ///
    /// Returns the voting power for the deposit, giving locked tokens boosted
    /// voting power that scales linearly with the lockup.
    ///
    /// The minimum lockup period is a single day. The max lockup period is
    /// seven years. And so a one day lockup has 1/2 the voting power as a two
    /// day lockup, which has 1/2555 the voting power of a 7 year lockup--
    /// assuming the amount locked up is equal.
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
    /// The calculation for this is straight forward
    ///
    /// ```
    /// voting_power = (number_days / 2555) * amount
    /// ```
    ///
    /// ### Decay
    ///
    /// As time passes, the voting power should decay proportionally, in which
    /// case one can substitute for `number_days` the number of days
    /// remaining on the lockup.
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
    /// have
    ///
    /// ```
    /// voting_power = 1/2555 * 5 + 2/2555 * 5
    /// ```
    ///
    /// Notice the scalar multipliers used to normalize the amounts.
    /// Since 7 years is the maximum lock, and 1 day is the minimum, we have
    /// a scalar of 1/2555 for a one day lock, 2/2555 for a two day lock,
    /// 2555/2555 for a 7 year lock, and 0 for no lock.
    ///
    /// We can rewrite the equation above as
    ///
    /// ```
    /// voting_power = 1/2555 * 5 + 2/2555 * 5
    ///              = 1/2555 * 10/2 + 2/2555 * 10/2
    /// ```
    ///
    /// Let's now generalize this to a daily vesting schedule over seven years.
    /// Let "amount" be the total amount for vesting. Then the total voting
    /// power to start is
    ///
    /// ```
    /// voting_power = 1/2555*(amount/2555) + 2/2555*(amount/2555) + ... + (2555/2555)*(amount/2555)
    ///              = 1/2555 * [1*(amount/2555) + 2*(amount/2555) + ... + 2555*(amount/255)]
    ///              = (1/2555) * (amount/2555) * (1 + 2 + ... + 2555)
    ///              = (1/2555) * (amount/2555) * [(2555 * [2555 + 1]) / 2]
    ///              = (1 / m) * (amount / n) * [(n * [n + 1]) / 2],
    /// ```
    ///
    /// where `m` is the max number of lockup days and `n` is the number of
    /// days for the entire vesting schedule.
    ///
    /// ### Decay
    ///
    /// To calculate the decay, we can simply re-use the above sum, adjusting
    /// `n` for the number of days left in the lockup.
    pub fn voting_power(&self, curr_ts: i64) -> Result<u64> {
        if curr_ts < self.lockup.start_ts {
            return Ok(0);
        }
        match self.lockup.kind {
            LockupKind::Daily => self.voting_power_daily(curr_ts),
            LockupKind::Monthly => self.voting_power_monthly(curr_ts),
            LockupKind::Cliff => self.voting_power_cliff(curr_ts),
        }
    }

    fn voting_power_daily(&self, curr_ts: i64) -> Result<u64> {
        let m = MAX_DAYS_LOCKED;
        let n = self.lockup.days_left(curr_ts)?;

        if n == 0 {
            return Ok(0);
        }

        let decayed_vote_weight = self
            .amount_scaled
            .checked_mul(
                // Ok to divide by two here because, if n is zero, then the
                // voting power is zero. And if n is one or above, then the
                // numerator is 2 or above.
                n.checked_mul(n.checked_add(1).unwrap())
                    .unwrap()
                    .checked_div(2)
                    .unwrap(),
            )
            .unwrap()
            .checked_div(m.checked_mul(n).unwrap()) //.checked_mul(2).unwrap())
            .unwrap();

        Ok(decayed_vote_weight)
    }

    fn voting_power_monthly(&self, curr_ts: i64) -> Result<u64> {
        let m = MAX_MONTHS_LOCKED;
        let n = self.lockup.months_left(curr_ts)?;

        if n == 0 {
            return Ok(0);
        }

        let decayed_vote_weight = self
            .amount_scaled
            .checked_mul(
                // Ok to divide by two here because, if n is zero, then the
                // voting power is zero. And if n is one or above, then the
                // numerator is 2 or above.
                n.checked_mul(n.checked_add(1).unwrap())
                    .unwrap()
                    .checked_div(2)
                    .unwrap(),
            )
            .unwrap()
            .checked_div(m.checked_mul(n).unwrap()) //.checked_mul(2).unwrap())
            .unwrap();

        Ok(decayed_vote_weight)
    }

    fn voting_power_cliff(&self, curr_ts: i64) -> Result<u64> {
        let decayed_voting_weight = self
            .lockup
            .days_left(curr_ts)?
            .checked_mul(self.amount_scaled)
            .unwrap()
            .checked_div(MAX_DAYS_LOCKED)
            .unwrap();

        Ok(decayed_voting_weight)
    }

    /// Returns the amount of unlocked tokens for this deposit--in native units
    /// of the original token amount (not scaled by the exchange rate).
    pub fn vested(&self) -> Result<u64> {
        let curr_ts = Clock::get()?.unix_timestamp;
        if curr_ts < self.lockup.start_ts {
            return Ok(0);
        }
        match self.lockup.kind {
            LockupKind::Daily => self.vested_daily(curr_ts),
            LockupKind::Monthly => self.vested_monthly(curr_ts),
            LockupKind::Cliff => self.vested_cliff(),
        }
    }

    fn vested_daily(&self, curr_ts: i64) -> Result<u64> {
        let day_current = self.lockup.day_current(curr_ts)?;
        let days_total = self.lockup.days_total()?;
        if day_current >= days_total {
            return Ok(self.amount_deposited);
        }
        let vested = self
            .amount_deposited
            .checked_mul(day_current)
            .unwrap()
            .checked_div(days_total)
            .unwrap();
        Ok(vested)
    }

    fn vested_monthly(&self, curr_ts: i64) -> Result<u64> {
        let month_current = self.lockup.month_current(curr_ts)?;
        let months_total = self.lockup.months_total()?;
        if month_current >= months_total {
            return Ok(self.amount_deposited);
        }
        let vested = self
            .amount_deposited
            .checked_mul(month_current)
            .unwrap()
            .checked_div(months_total)
            .unwrap();
        Ok(vested)
    }

    fn vested_cliff(&self) -> Result<u64> {
        let curr_ts = Clock::get()?.unix_timestamp;
        if curr_ts < self.lockup.end_ts {
            return Ok(0);
        }
        Ok(self.amount_deposited)
    }

    /// Returns the amount left in the deposit, ignoring the vesting schedule.
    pub fn amount_left(&self) -> u64 {
        self.amount_deposited
            .checked_sub(self.amount_withdrawn)
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
    pub padding: [u8; 16],
}

impl Lockup {
    /// Returns the number of days left on the lockup.
    pub fn days_left(&self, curr_ts: i64) -> Result<u64> {
        Ok(self
            .days_total()?
            .saturating_sub(self.day_current(curr_ts)?))
    }

    /// Returns the current day in the vesting schedule.
    pub fn day_current(&self, curr_ts: i64) -> Result<u64> {
        let d = u64::try_from({
            let secs_elapsed = curr_ts.saturating_sub(self.start_ts);
            secs_elapsed.checked_div(SECS_PER_DAY).unwrap()
        })
        .map_err(|_| ErrorCode::UnableToConvert)?;
        Ok(d)
    }

    /// Returns the total amount of days in the lockup period.
    pub fn days_total(&self) -> Result<u64> {
        // Number of seconds in the entire lockup.
        let lockup_secs = self.end_ts.checked_sub(self.start_ts).unwrap();
        require!(lockup_secs % SECS_PER_DAY == 0, InvalidLockupPeriod);

        // Total days in the entire lockup.
        let lockup_days = u64::try_from(lockup_secs.checked_div(SECS_PER_DAY).unwrap()).unwrap();

        Ok(lockup_days)
    }

    /// Returns the number of months left on the lockup.
    pub fn months_left(&self, curr_ts: i64) -> Result<u64> {
        Ok(self
            .months_total()?
            .saturating_sub(self.month_current(curr_ts)?))
    }

    /// Returns the current month in the vesting schedule.
    pub fn month_current(&self, curr_ts: i64) -> Result<u64> {
        let d = u64::try_from({
            let secs_elapsed = curr_ts.saturating_sub(self.start_ts);
            secs_elapsed.checked_div(SECS_PER_MONTH).unwrap()
        })
        .map_err(|_| ErrorCode::UnableToConvert)?;
        Ok(d)
    }

    /// Returns the total amount of months in the lockup period.
    pub fn months_total(&self) -> Result<u64> {
        // Number of seconds in the entire lockup.
        let lockup_secs = self.end_ts.checked_sub(self.start_ts).unwrap();
        require!(lockup_secs % SECS_PER_MONTH == 0, InvalidLockupPeriod);

        // Total months in the entire lockup.
        let lockup_months =
            u64::try_from(lockup_secs.checked_div(SECS_PER_MONTH).unwrap()).unwrap();

        Ok(lockup_months)
    }
}

#[repr(u8)]
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, Copy)]
pub enum LockupKind {
    Daily,
    Monthly,
    Cliff,
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
        let expected_voting_power = locked_daily_power(amount_deposited, 10);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 10);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 9);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 9);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 8);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 1);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 1);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 0);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 0);
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
        let expected_voting_power = locked_daily_power(amount_deposited, 0);
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
        let days_left = l.days_left(curr_ts)?;
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
        let months_left = l.months_left(curr_ts)?;
        assert_eq!(months_left, t.expected_months_left);
        Ok(())
    }

    fn run_test_voting_power(t: TestVotingPower) -> Result<()> {
        let start_ts = 1634929833;
        let end_ts = start_ts + days_to_secs(t.days_total);
        let d = DepositEntry {
            is_used: true,
            rate_idx: 0,
            amount_deposited: t.amount_deposited,
            amount_withdrawn: 0,
            amount_scaled: t.amount_deposited,
            lockup: Lockup {
                start_ts,
                end_ts,
                kind: t.kind,
                padding: [0u8; 16],
            },
        };
        let curr_ts = start_ts + days_to_secs(t.curr_day);
        let power = d.voting_power(curr_ts)?;
        assert_eq!(power, t.expected_voting_power);
        Ok(())
    }

    fn days_to_secs(days: f64) -> i64 {
        let d = 86_400. * days;
        d.round() as i64
    }

    fn months_to_secs(months: f64) -> i64 {
        let d = 86_400. * 30. * months;
        d.round() as i64
    }

    // Calculates locked voting power. Done iteratively as a sanity check on
    // the closed form calcuation.
    //
    // deposit - the amount locked up
    // days - the number of days locked
    fn locked_daily_power(amount: u64, days: u64) -> u64 {
        let mut total = 0f64;
        for k in 1..(days + 1) {
            total += (k as f64 * amount as f64) / (MAX_DAYS_LOCKED as f64 * days as f64)
        }
        total.floor() as u64
    }
}
