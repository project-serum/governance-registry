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
pub const SECS_PER_DAY: i64 = 86_400;

/// Maximum number of days one can lock for.
pub const MAX_DAYS_LOCKED: u64 = 2555;

/// Instance of a voting rights distributor.
#[account(zero_copy)]
pub struct Registrar {
    pub authority: Pubkey,
    pub realm: Pubkey,
    pub warmup_secs: i64,
    pub bump: u8,
    // The length should be adjusted for one's use case.
    pub rates: [ExchangeRateEntry; 2],
}

/// User account for minting voting rights.
#[account(zero_copy)]
pub struct Voter {
    pub authority: Pubkey,
    pub registrar: Pubkey,
    pub voter_bump: u8,
    pub deposits: [DepositEntry; 32],
}

impl Voter {
    pub fn weight(&self) -> Result<u64> {
        self.deposits
            .iter()
            .filter(|d| d.is_used)
            .try_fold(0, |sum, d| d.voting_power().map(|vp| sum + vp))
    }
}

/// Exchange rate for an asset that can be used to mint voting rights.
#[zero_copy]
#[derive(AnchorSerialize, AnchorDeserialize, Default)]
pub struct ExchangeRateEntry {
    pub mint: Pubkey,
    pub rate: u64,
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
    ///
    /// ## Voting Power Warmup
    ///
    /// To prevent the case where one borrows tokens to suddenly vote on a
    /// favorable proposal, one can introduce a "warmup" period, where the
    /// lockup calculation doesn't start until a specific date, so that
    /// the voting power of all new depositors remains zero for an initial
    /// period of time, say, two weeks.
    pub fn voting_power(&self) -> Result<u64> {
        let curr_ts = Clock::get()?.unix_timestamp;

        // Voting power is zero until the warmup period ends.
        if curr_ts < self.lockup.start_ts {
            return Ok(0);
        }

        match self.lockup.kind {
            LockupKind::Daily => self.voting_power_daily(),
            LockupKind::Cliff => self.voting_power_cliff(),
        }
    }

    fn voting_power_daily(&self) -> Result<u64> {
        let m = MAX_DAYS_LOCKED;
        let n = self.lockup.days_left()?;

        let decayed_vote_weight = self
            .amount_scaled
            .checked_mul(n.checked_mul(n.checked_add(1).unwrap()).unwrap())
            .unwrap()
            .checked_div(m.checked_mul(n).unwrap().checked_mul(2).unwrap())
            .unwrap();

        Ok(decayed_vote_weight)
    }

    fn voting_power_cliff(&self) -> Result<u64> {
        let voting_weight = self
            .lockup
            .days_left()?
            .checked_mul(self.amount_scaled)
            .unwrap()
            .checked_div(MAX_DAYS_LOCKED)
            .unwrap();

        Ok(voting_weight)
    }

    /// Returns the amount of unlocked tokens for this deposit--in native units
    /// of the original token amount (not scaled by the exchange rate).
    pub fn vested(&self) -> Result<u64> {
        let curr_ts = Clock::get()?.unix_timestamp;
        if curr_ts < self.lockup.start_ts {
            return Ok(0);
        }
        match self.lockup.kind {
            LockupKind::Daily => self.vested_daily(),
            LockupKind::Cliff => self.vested_cliff(),
        }
    }

    fn vested_daily(&self) -> Result<u64> {
        let day_current = self.lockup.day_current()?;
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
    // Start of the lockup, shifted by the warmup period.
    pub start_ts: i64,
    // End of the lockup, shifted by the warmup period.
    pub end_ts: i64,
    // Empty bytes for future upgrades.
    pub padding: [u8; 16],
}

impl Lockup {
    /// Returns the number of days left on the lockup, ignoring the warmup
    /// period.
    pub fn days_left(&self) -> Result<u64> {
        Ok(self.days_total()?.checked_sub(self.day_current()?).unwrap())
    }

    /// Returns the current day in the vesting schedule. The warmup period is
    /// treated as day zero.
    pub fn day_current(&self) -> Result<u64> {
        let curr_ts = Clock::get()?.unix_timestamp;

        // Warmup period hasn't ended.
        if curr_ts < self.start_ts {
            return Ok(0);
        }

        u64::try_from({
            let secs_elapsed = curr_ts.checked_sub(self.start_ts).unwrap();
            secs_elapsed.checked_sub(SECS_PER_DAY).unwrap()
        })
        .map_err(|_| ErrorCode::UnableToConvert.into())
    }

    /// Returns the total amount of days in the lockup period, ignoring the
    /// warmup period.
    pub fn days_total(&self) -> Result<u64> {
        // Number of seconds in the entire lockup.
        let lockup_secs = self.end_ts.checked_sub(self.start_ts).unwrap();
        require!(lockup_secs % SECS_PER_DAY == 0, InvalidLockupPeriod);

        // Total days in the entire lockup.
        let lockup_days = u64::try_from(lockup_secs.checked_div(SECS_PER_DAY).unwrap()).unwrap();

        Ok(lockup_days)
    }
}

#[repr(u8)]
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, Copy)]
pub enum LockupKind {
    Daily,
    Cliff,
}
