use crate::error::*;
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

impl Default for Lockup {
    fn default() -> Self {
        Self {
            kind: LockupKind::None,
            start_ts: 0,
            end_ts: 0,
            padding: [0; 16],
        }
    }
}

impl Lockup {
    /// Create lockup for a given period
    pub fn new_from_periods(kind: LockupKind, start_ts: i64, periods: u32) -> Result<Self> {
        require!(periods as u64 <= kind.max_periods(), InvalidDays);
        Ok(Self {
            kind,
            start_ts,
            end_ts: start_ts
                .checked_add(i64::from(periods).checked_mul(kind.period_secs()).unwrap())
                .unwrap(),
            padding: [0; 16],
        })
    }

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
    use crate::state::deposit_entry::DepositEntry;

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
            allow_clawback: false,
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
