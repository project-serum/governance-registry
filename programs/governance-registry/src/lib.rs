use access_control::*;
use account::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint};
use context::*;
use error::*;
use spl_governance::addins::voter_weight::VoterWeightAccountType;
use spl_governance::state::token_owner_record;
use std::{convert::TryFrom, str::FromStr};

mod access_control;
pub mod account;
mod context;
mod error;

// The program address.
declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

/// # Introduction
///
/// The governance registry is an "addin" to the SPL governance program that
/// allows one to both vote with many different ypes of tokens for voting and to
/// scale voting power as a linear function of time locked--subject to some
/// maximum upper bound.
///
/// The flow for voting with this program is as follows:
///
/// - Create a SPL governance realm.
/// - Create a governance registry account.
/// - Add exchange rates for any tokens one wants to deposit. For example,
///   if one wants to vote with tokens A and B, where token B has twice the
///   voting power of token A, then the exchange rate of B would be 2 and the
///   exchange rate of A would be 1.
/// - Create a voter account.
/// - Deposit tokens into this program, with an optional lockup period.
/// - Vote.
///
/// Upon voting with SPL governance, a client is expected to call
/// `decay_voting_power` to get an up to date measurement of a given `Voter`'s
/// voting power for the given slot. If this is not done, then the transaction
/// will fail (since the SPL governance program will require the measurement
/// to be active for the current slot).
///
/// # Interacting with SPL Governance
///
/// This program does not directly interact with SPL governance via CPI.
/// Instead, it simply writes a `VoterWeightRecord` account with a well defined
/// format, which is then used by SPL governance as the voting power measurement
/// for a given user.
///
/// # Max Vote Weight
///
/// Given that one can use multiple tokens to vote, the max vote weight needs
/// to be a function of the total supply of all tokens, converted into a common
/// currency. For example, if you have Token A and Token B, where 1 Token B =
/// 10 Token A, then the `max_vote_weight` should be `supply(A) + supply(B)*10`
/// where both are converted into common decimals. Then, when calculating the
/// weight of an individual voter, one can convert B into A via the given
/// exchange rate, which must be fixed.
///
/// Note that the above also implies that the `max_vote_weight` must fit into
/// a u64.
#[program]
pub mod governance_registry {
    use super::*;

    /// Creates a new voting registrar. There can only be a single registrar
    /// per governance realm.
    pub fn create_registrar(
        ctx: Context<CreateRegistrar>,
        vote_weight_decimals: u8,
        registrar_bump: u8,
    ) -> Result<()> {
        msg!("--------create_registrar--------");
        let registrar = &mut ctx.accounts.registrar;
        registrar.bump = registrar_bump;
        registrar.governance_program_id = ctx.accounts.governance_program_id.key();
        registrar.realm = ctx.accounts.realm.key();
        registrar.realm_community_mint = ctx.accounts.realm_community_mint.key();
        registrar.registrar_authority = ctx.accounts.registrar_authority.key();
        registrar.clawback_authority = ctx.accounts.clawback_authority.key();
        registrar.vote_weight_decimals = vote_weight_decimals;
        registrar.time_offset = 0;

        Ok(())
    }

    /// Creates a new exchange rate for a given mint. This allows a voter to
    /// deposit the mint in exchange for vTokens. There can only be a single
    /// exchange rate per mint.
    ///
    /// WARNING: This can be freely called when any of the rates are empty.
    ///          This should be called immediately upon creation of a Registrar.
    #[access_control(rate_is_empty(&ctx, idx))]
    pub fn create_exchange_rate(
        ctx: Context<CreateExchangeRate>,
        idx: u16,
        mint: Pubkey,
        rate: u64,
        decimals: u8,
    ) -> Result<()> {
        msg!("--------create_exchange_rate--------");
        require!(rate > 0, InvalidRate);
        let registrar = &mut ctx.accounts.registrar;
        registrar.rates[idx as usize] = registrar.new_rate(mint, decimals, rate)?;
        Ok(())
    }

    /// Creates a new voter account. There can only be a single voter per
    /// user wallet.
    pub fn create_voter(
        ctx: Context<CreateVoter>,
        voter_bump: u8,
        voter_weight_record_bump: u8,
    ) -> Result<()> {
        msg!("--------create_voter--------");
        // Forbid creating voter accounts from CPI. The goal is to make automation
        // impossible that weakens some of the limitations intentionally imposed on
        // locked tokens.
        {
            use anchor_lang::solana_program::sysvar::instructions as tx_instructions;
            let ixns = ctx.accounts.instructions.to_account_info();
            let current_index = tx_instructions::load_current_index_checked(&ixns)? as usize;
            let current_ixn = tx_instructions::load_instruction_at_checked(current_index, &ixns)?;
            require!(
                current_ixn.program_id == *ctx.program_id,
                ErrorCode::ForbiddenCpi
            );
        }

        // Load accounts.
        let registrar = &ctx.accounts.registrar;
        let voter = &mut ctx.accounts.voter.load_init()?;
        let voter_weight_record = &mut ctx.accounts.voter_weight_record;

        // Init the voter.
        voter.voter_bump = voter_bump;
        voter.voter_weight_record_bump = voter_weight_record_bump;
        voter.voter_authority = ctx.accounts.voter_authority.key();
        voter.registrar = ctx.accounts.registrar.key();

        // Init the voter weight record.
        voter_weight_record.account_type = VoterWeightAccountType::VoterWeightRecord;
        voter_weight_record.realm = registrar.realm;
        voter_weight_record.governing_token_mint = registrar.realm_community_mint;
        voter_weight_record.governing_token_owner = ctx.accounts.voter_authority.key();

        Ok(())
    }

    /// Creates a new deposit entry and updates it by transferring in tokens.
    pub fn create_deposit(
        ctx: Context<CreateDeposit>,
        kind: LockupKind,
        amount: u64,
        periods: i32,
        allow_clawback: bool,
    ) -> Result<()> {
        msg!("--------create_deposit--------");
        // Creates the new deposit.
        let deposit_id = {
            // Load accounts.
            let registrar = &ctx.accounts.deposit.registrar;
            let voter = &mut ctx.accounts.deposit.voter.load_mut()?;

            // Set the lockup start timestamp.
            let start_ts = registrar.clock_unix_timestamp();

            // Get the exchange rate entry associated with this deposit.
            let er_idx = registrar
                .rates
                .iter()
                .position(|r| r.mint == ctx.accounts.deposit.deposit_mint.key())
                .ok_or(ErrorCode::ExchangeRateEntryNotFound)?;

            // Get and set up the first free deposit entry.
            let free_entry_idx = voter
                .deposits
                .iter()
                .position(|d_entry| !d_entry.is_used)
                .ok_or(ErrorCode::DepositEntryFull)?;
            let d_entry = &mut voter.deposits[free_entry_idx];
            d_entry.is_used = true;
            d_entry.rate_idx = free_entry_idx as u8;
            d_entry.rate_idx = er_idx as u8;
            d_entry.amount_deposited_native = 0;
            d_entry.amount_initially_locked_native = 0;
            d_entry.allow_clawback = allow_clawback;
            d_entry.lockup = Lockup {
                kind,
                start_ts,
                end_ts: start_ts
                    .checked_add(i64::from(periods).checked_mul(kind.period_secs()).unwrap())
                    .unwrap(),
                padding: [0u8; 16],
            };

            free_entry_idx as u8
        };

        // Updates the entry by transferring in tokens.
        let update_ctx = Context::new(ctx.program_id, &mut ctx.accounts.deposit, &[]);
        update_deposit(update_ctx, deposit_id, amount)?;

        Ok(())
    }

    /// Updates a deposit entry by depositing tokens into the registrar in
    /// exchange for *frozen* voting tokens. These tokens are not used for
    /// anything other than displaying the amount in wallets.
    pub fn update_deposit(
        ctx: Context<UpdateDeposit>,
        id: u8,
        amount: u64,
    ) -> Result<()> {
        msg!("--------update_deposit--------");
        let registrar = &ctx.accounts.registrar;
        let voter = &mut ctx.accounts.voter.load_mut()?;

        voter.last_deposit_slot = Clock::get()?.slot;

        // Get the exchange rate entry associated with this deposit.
        let er_idx = registrar
            .rates
            .iter()
            .position(|r| r.mint == ctx.accounts.deposit_mint.key())
            .ok_or(ErrorCode::ExchangeRateEntryNotFound)?;
        let _er_entry = registrar.rates[er_idx];

        require!(voter.deposits.len() > id as usize, InvalidDepositId);
        let d_entry = &mut voter.deposits[id as usize];
        require!(d_entry.is_used, InvalidDepositId);

        // Deposit tokens into the registrar.
        token::transfer(ctx.accounts.transfer_ctx(), amount)?;
        d_entry.amount_deposited_native += amount;

        // Adding funds to a lockup that is already in progress can be complicated
        // for linear vesting schedules because all added funds should be paid out
        // gradually over the remaining lockup duration.
        // The logic used is to wrap up the current lockup, and create a new one
        // for the expected remainder duration.
        let curr_ts = registrar.clock_unix_timestamp();
        d_entry.amount_initially_locked_native -= d_entry.vested(curr_ts)?;
        d_entry.amount_initially_locked_native += amount;
        d_entry.lockup.start_ts = d_entry
            .lockup
            .start_ts
            .checked_add(
                i64::try_from(d_entry.lockup.period_current(curr_ts)?)
                    .unwrap()
                    .checked_mul(d_entry.lockup.kind.period_secs())
                    .unwrap(),
            )
            .unwrap();

        // Thaw the account if it's frozen, so that we can mint.
        if ctx.accounts.voting_token.is_frozen() {
            token::thaw_account(
                ctx.accounts
                    .thaw_ctx()
                    .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
            )?;
        }

        // Mint vote tokens to the depositor.
        token::mint_to(
            ctx.accounts
                .mint_to_ctx()
                .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
            amount,
        )?;

        // Freeze the vote tokens; they are just used for UIs + accounting.
        token::freeze_account(
            ctx.accounts
                .freeze_ctx()
                .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
        )?;

        Ok(())
    }

    pub fn clawback(ctx: Context<Withdraw>, deposit_id: u8) -> Result<()> {
        msg!("--------clawback--------");
        // Load the accounts.
        let registrar = &ctx.accounts.registrar;
        let voter = &mut ctx.accounts.voter.load_mut()?;
        require!(voter.deposits.len() > deposit_id.into(), InvalidDepositId);

        // TODO: verify that the destination is owned by the realm and its governance

        // Get the deposit being withdrawn from.
        let curr_ts = registrar.clock_unix_timestamp();
        let deposit_entry = &mut voter.deposits[deposit_id as usize];
        require!(
            deposit_entry.allow_clawback,
            ErrorCode::ClawbackNotAllowedOnDeposit
        );
        let amount_not_yet_vested =
            deposit_entry.amount_deposited_native - deposit_entry.vested(curr_ts).unwrap();

        // Transfer the tokens to withdraw.
        token::transfer(
            ctx.accounts
                .transfer_ctx()
                .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
            amount_not_yet_vested,
        )?;

        // Unfreeze the voting token.
        token::thaw_account(
            ctx.accounts
                .thaw_ctx()
                .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
        )?;

        // Burn the voting tokens.
        token::burn(ctx.accounts.burn_ctx(), amount_not_yet_vested)?;

        // Re-freeze the vote token.
        token::freeze_account(
            ctx.accounts
                .freeze_ctx()
                .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
        )?;

        // Update deposit book keeping.
        deposit_entry.amount_deposited_native = deposit_entry.vested(curr_ts).unwrap();
        deposit_entry.amount_initially_locked_native = 0;
        deposit_entry.lockup.kind = LockupKind::None;

        Ok(())
    }
    /// Withdraws tokens from a deposit entry, if they are unlocked according
    /// to a vesting schedule.
    ///
    /// `amount` is in units of the native currency being withdrawn.
    pub fn withdraw(ctx: Context<Withdraw>, deposit_id: u8, amount: u64) -> Result<()> {
        msg!("--------withdraw--------");
        // Load the accounts.
        let registrar = &ctx.accounts.registrar;
        let voter = &mut ctx.accounts.voter.load_mut()?;
        require!(voter.deposits.len() > deposit_id.into(), InvalidDepositId);

        // Governance may forbid withdraws, for example when engaged in a vote.
        let token_owner = ctx.accounts.voter_authority.key();
        let token_owner_record_address_seeds =
            token_owner_record::get_token_owner_record_address_seeds(
                &registrar.realm,
                &registrar.realm_community_mint,
                &token_owner,
            );
        let token_owner_record_data = token_owner_record::get_token_owner_record_data_for_seeds(
            &registrar.governance_program_id,
            &ctx.accounts.token_owner_record.to_account_info(),
            &token_owner_record_address_seeds,
        )?;
        token_owner_record_data.assert_can_withdraw_governing_tokens()?;

        // Must not withdraw in the same slot as depositing, to prevent people
        // depositing, having the vote weight updated, withdrawing and then
        // voting.
        require!(
            voter.last_deposit_slot < Clock::get()?.slot,
            ErrorCode::InvalidToDepositAndWithdrawInOneSlot
        );

        // Get the deposit being withdrawn from.
        let curr_ts = registrar.clock_unix_timestamp();
        let deposit_entry = &mut voter.deposits[deposit_id as usize];
        require!(deposit_entry.is_used, InvalidDepositId);
        require!(
            deposit_entry.amount_withdrawable(curr_ts) >= amount,
            InsufficientVestedTokens
        );
        // technically unnecessary
        require!(
            deposit_entry.amount_deposited_native >= amount,
            InsufficientVestedTokens
        );

        // Get the exchange rate for the token being withdrawn.
        let er_idx = registrar
            .rates
            .iter()
            .position(|r| r.mint == ctx.accounts.withdraw_mint.key())
            .ok_or(ErrorCode::ExchangeRateEntryNotFound)?;
        let _er_entry = registrar.rates[er_idx];
        require!(
            er_idx == deposit_entry.rate_idx as usize,
            ErrorCode::InvalidMint
        );

        // Update deposit book keeping.
        deposit_entry.amount_deposited_native -= amount;

        // Transfer the tokens to withdraw.
        token::transfer(
            ctx.accounts
                .transfer_ctx()
                .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
            amount,
        )?;

        // Unfreeze the voting token.
        token::thaw_account(
            ctx.accounts
                .thaw_ctx()
                .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
        )?;

        // Burn the voting tokens.
        token::burn(ctx.accounts.burn_ctx(), amount)?;

        // Re-freeze the vote token.
        token::freeze_account(
            ctx.accounts
                .freeze_ctx()
                .with_signer(&[&[registrar.realm.as_ref(), &[registrar.bump]]]),
        )?;

        Ok(())
    }

    /// Close an empty deposit, allowing it to be reused in the future
    pub fn close_deposit(ctx: Context<CloseDeposit>, deposit_id: u8) -> Result<()> {
        msg!("--------close_deposit--------");
        let voter = &mut ctx.accounts.voter.load_mut()?;

        require!(voter.deposits.len() > deposit_id as usize, InvalidDepositId);
        let d = &mut voter.deposits[deposit_id as usize];
        require!(d.is_used, InvalidDepositId);
        require!(d.amount_deposited_native == 0, VotingTokenNonZero);

        // We do not need to check d.amount_initially_locked_native or d.lockup
        // here - the fact that the deposit contains no tokens is sufficient.

        d.is_used = false;
        Ok(())
    }

    /// Resets a lockup to start at the current slot timestamp and to last for
    /// `periods`, which must be >= the number of periods left on the lockup.
    /// This will re-lock any non-withdrawn vested funds.
    pub fn reset_lockup(ctx: Context<UpdateSchedule>, deposit_id: u8, periods: i64) -> Result<()> {
        msg!("--------reset_lockup--------");
        let registrar = &ctx.accounts.registrar;
        let voter = &mut ctx.accounts.voter.load_mut()?;
        require!(voter.deposits.len() > deposit_id as usize, InvalidDepositId);

        let d = &mut voter.deposits[deposit_id as usize];
        require!(d.is_used, InvalidDepositId);

        // The lockup period can only be increased.
        let curr_ts = registrar.clock_unix_timestamp();
        require!(
            periods as u64 >= d.lockup.periods_left(curr_ts)?,
            InvalidDays
        );

        // TODO: Check for correctness
        d.amount_initially_locked_native = d.amount_deposited_native;

        d.lockup.start_ts = curr_ts;
        d.lockup.end_ts = curr_ts
            .checked_add(periods.checked_mul(d.lockup.kind.period_secs()).unwrap())
            .unwrap();

        Ok(())
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

    /// Calculates the max vote weight for the registry. This is a function
    /// of the total supply of all exchange rate mints, converted into a
    /// common currency with a common number of decimals.
    ///
    /// Note that this method is only safe to use if the cumulative supply for
    /// all tokens fits into a u64 *after* converting into common decimals, as
    /// defined by the registrar's `rate_decimal` field.
    pub fn update_max_vote_weight<'info>(
        ctx: Context<'_, '_, '_, 'info, UpdateMaxVoteWeight<'info>>,
    ) -> Result<()> {
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

    /// Closes the voter account, allowing one to retrieve rent exemption SOL.
    /// Only accounts with no remaining deposits can be closed.
    pub fn close_voter(ctx: Context<CloseVoter>) -> Result<()> {
        msg!("--------close_voter--------");
        let voter = &ctx.accounts.voter.load()?;
        let amount = voter.deposits.iter().fold(0u64, |sum, d| {
            sum.checked_add(d.amount_deposited_native).unwrap()
        });
        require!(amount == 0, VotingTokenNonZero);
        Ok(())
    }

    pub fn set_time_offset(ctx: Context<SetTimeOffset>, time_offset: i64) -> Result<()> {
        msg!("--------set_time_offset--------");
        let allowed_program =
            Pubkey::from_str("GovernanceProgram11111111111111111111111111").unwrap();
        let registrar = &mut ctx.accounts.registrar;
        require!(
            registrar.governance_program_id == allowed_program,
            ErrorCode::DebugInstruction
        );
        registrar.time_offset = time_offset;
        Ok(())
    }
}
