use crate::error::*;
use crate::state::exchange_entry::ExchangeRateEntry;
use crate::state::lockup::{Lockup, LockupKind};
use anchor_lang::prelude::*;

/// Vote weight is amount * FIXED_VOTE_WEIGHT_FACTOR +
/// LOCKING_VOTE_WEIGHT_FACTOR * amount * time / max time
pub const FIXED_VOTE_WEIGHT_FACTOR: u64 = 1;
pub const LOCKING_VOTE_WEIGHT_FACTOR: u64 = 0;

/// Bookkeeping for a single deposit for a given mint and lockup schedule.
#[zero_copy]
#[derive(Default)]
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

    pub allow_clawback: bool,

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
    pub fn voting_power_locked(&self, curr_ts: i64, max_contribution: u64) -> Result<u64> {
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
                periods_left
                    .checked_mul(periods_left.checked_add(1).unwrap())
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

    /// Returns native tokens still locked.
    #[inline(always)]
    pub fn amount_locked(&self, curr_ts: i64) -> u64 {
        self.amount_initially_locked_native
            .checked_sub(self.vested(curr_ts).unwrap())
            .unwrap()
    }

    /// Returns the amount that may be withdrawn given current vesting
    /// and previous withdraws.
    #[inline(always)]
    pub fn amount_withdrawable(&self, curr_ts: i64) -> u64 {
        self.amount_deposited_native
            .checked_sub(self.amount_locked(curr_ts))
            .unwrap()
    }
}
