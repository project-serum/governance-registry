use anchor_lang::__private::bytemuck::Zeroable;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};
use std::mem::size_of;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod governance_registry {
    use super::*;

    /// Creates a new voting registrar. There can only be a single regsitrar
    /// per governance realm.
    pub fn init_registrar(
        ctx: Context<InitRegistrar>,
        registrar_bump: u8,
        voting_mint_bump: u8,
        _voting_mint_decimals: u8,
    ) -> Result<()> {
        let registrar = &mut ctx.accounts.registrar.load_init()?;
        registrar.registrar_bump = registrar_bump;
        registrar.voting_mint_bump = voting_mint_bump;
        registrar.realm = ctx.accounts.realm.key();
        registrar.voting_mint = ctx.accounts.voting_mint.key();
        registrar.authority = ctx.accounts.authority.key();

        Ok(())
    }

    /// Creates a new voter account. There can only be a single voter per
    /// user wallet.
    pub fn init_voter(ctx: Context<InitVoter>, voter_bump: u8) -> Result<()> {
        let voter = &mut ctx.accounts.voter.load_mut()?;
        voter.voter_bump = voter_bump;
        voter.authority = ctx.accounts.authority.key();
        voter.registrar = ctx.accounts.registrar.key();

        Ok(())
    }

    /// Creates a new exchange rate for a given mint. This allows a voter to
    /// deposit the mint in exchange for vTokens. There can only be a single
    /// exchange rate per mint.
    pub fn add_exchange_rate(ctx: Context<AddExchangeRate>, er: ExchangeRateEntry) -> Result<()> {
        require!(er.rate > 0, InvalidRate);
        let registrar = &mut ctx.accounts.registrar.load_mut()?;
        let idx = registrar
            .rates
            .iter()
            .position(|r| !r.is_used)
            .ok_or(ErrorCode::RatesFull)?;
        registrar.rates[idx] = er;
        Ok(())
    }

    /// Deposits tokens into the registrar in exchange for *frozen* voting
    /// tokens. These tokens are not used for anything other than displaying
    /// the amount in walletes.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        let registrar = &mut ctx.accounts.registrar.load()?;
        let voter = &mut ctx.accounts.voter.load_mut()?;

        // Get the exchange rate entry associated with this deposit.
        let er_idx = registrar
            .rates
            .iter()
            .position(|r| r.mint == ctx.accounts.deposit_mint.key())
            .ok_or(ErrorCode::ExchangeRateEntryNotFound)?;
        let er_entry = registrar.rates[er_idx];

        // Get the deposit entry associated with this deposit.
        let deposit_entry = {
            match voter.deposits.iter().position(|deposit_entry| {
                registrar.rates[deposit_entry.rate_idx as usize].mint
                    == ctx.accounts.deposit_mint.key()
            }) {
                // Lazily instantiate the deposit if needed.
                None => {
                    let free_entry_idx = voter
                        .deposits
                        .iter()
                        .position(|deposit_entry| !deposit_entry.is_used)
                        .ok_or(ErrorCode::DepositEntryFull)?;
                    let entry = &mut voter.deposits[free_entry_idx];
                    entry.is_used = true;
                    entry.rate_idx = free_entry_idx as u8;
                    entry
                }
                // Use the existing deposit.
                Some(e) => &mut voter.deposits[e],
            }
        };
        deposit_entry.amount += amount;

        // Calculate the amount of voting tokens to mint.
        let scaled_amount = er_entry.rate * amount;

        // Deposit tokens into the registrar.
        token::transfer((&*ctx.accounts).into(), amount)?;

        // Mint vote tokens to the depositor.
        token::mint_to((&*ctx.accounts).into(), scaled_amount)?;

        Ok(())
    }

    /// Withdraws tokens from a deposit entry, if they are unlocked according
    /// to a vesting schedule.
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        // todo
        Ok(())
    }

    /// Updates a vesting schedule. Can only increase the lockup time. If all
    /// tokens are unlocked, then the period count can also be updated.
    pub fn update_schedule(ctx: Context<UpdateSchedule>) -> Result<()> {
        // todo
        Ok(())
    }
}

// Contexts.

#[derive(Accounts)]
#[instruction(registrar_bump: u8, voting_mint_bump: u8, voting_mint_decimals: u8)]
pub struct InitRegistrar<'info> {
    #[account(
        init,
        seeds = [realm.key().as_ref()],
        bump = registrar_bump,
        payer = payer,
				space = 8 + size_of::<Registrar>()
    )]
    registrar: AccountLoader<'info, Registrar>,
    #[account(
        init,
        seeds = [registrar.key().as_ref()],
        bump = voting_mint_bump,
        payer = payer,
        mint::authority = registrar,
        mint::decimals = voting_mint_decimals,
    )]
    voting_mint: Account<'info, Mint>,
    realm: UncheckedAccount<'info>,
    authority: UncheckedAccount<'info>,
    payer: Signer<'info>,
    system_program: Program<'info, System>,
    token_program: Program<'info, Token>,
    rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(voter_bump: u8)]
pub struct InitVoter<'info> {
    #[account(
        init,
        seeds = [registrar.key().as_ref(), authority.key().as_ref()],
        bump = voter_bump,
        payer = authority,
				space = 8 + size_of::<Voter>()
    )]
    voter: AccountLoader<'info, Voter>,
    registrar: AccountLoader<'info, Registrar>,
    authority: Signer<'info>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(rate: ExchangeRateEntry)]
pub struct AddExchangeRate<'info> {
    #[account(
        init,
        payer = payer,
        associated_token::authority = registrar,
        associated_token::mint = deposit_mint,
    )]
    exchange_vault: Account<'info, TokenAccount>,
    deposit_mint: Account<'info, Mint>,
    #[account(has_one = authority)]
    registrar: AccountLoader<'info, Registrar>,
    authority: Signer<'info>,
    payer: Signer<'info>,
    rent: Sysvar<'info, Rent>,
    token_program: Program<'info, Token>,
    associated_token_program: Program<'info, AssociatedToken>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(has_one = authority)]
    voter: AccountLoader<'info, Voter>,
    #[account(
        associated_token::authority = registrar,
        associated_token::mint = deposit_mint,
    )]
    exchange_vault: Account<'info, TokenAccount>,
    #[account(
        constraint = deposit_token.mint == deposit_mint.key(),
    )]
    deposit_token: Account<'info, TokenAccount>,
    #[account(
        constraint = registrar.load()?.voting_mint == voting_token.mint,
    )]
    voting_token: Account<'info, TokenAccount>,
    authority: Signer<'info>,
    registrar: AccountLoader<'info, Registrar>,
    deposit_mint: Account<'info, Mint>,
    voting_mint: Account<'info, Mint>,
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Withdraw {
    // todo
}

#[derive(Accounts)]
pub struct UpdateSchedule {
    // todo
}

// Accounts.

/// Instance of a voting rights distributor.
#[account(zero_copy)]
pub struct Registrar {
    pub authority: Pubkey,
    pub realm: Pubkey,
    pub voting_mint: Pubkey,
    pub voting_mint_bump: u8,
    pub registrar_bump: u8,
    pub rates: [ExchangeRateEntry; 32],
}

/// User account for minting voting rights.
#[account(zero_copy)]
pub struct Voter {
    pub authority: Pubkey,
    pub registrar: Pubkey,
    pub voter_bump: u8,
    pub deposits: [DepositEntry; 32],
}

/// Exchange rate for an asset that can be used to mint voting rights.
#[zero_copy]
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ExchangeRateEntry {
    // True if the exchange rate entry is being used.
    pub is_used: bool,

    pub mint: Pubkey,
    pub rate: u64,
}

unsafe impl Zeroable for ExchangeRateEntry {}

#[zero_copy]
pub struct DepositEntry {
    // True if the deposit entry is being used.
    pub is_used: bool,

    // Points to the ExchangeRate this deposit uses.
    pub rate_idx: u8,
    pub amount: u64,

    // Locked state.
    pub period_count: u64,
    pub start_ts: i64,
    pub end_ts: i64,
}

impl DepositEntry {
    /// Returns the voting power given by this deposit, scaled to account for
    /// a lockup.
    pub fn voting_power(&self) -> u64 {
        let locked_multiplier = 1; // todo
        self.amount * locked_multiplier
    }
}

// Error.

#[error]
pub enum ErrorCode {
    #[msg("Exchange rate must be greater than zero")]
    InvalidRate,
    #[msg("")]
    RatesFull,
    #[msg("")]
    ExchangeRateEntryNotFound,
    #[msg("")]
    DepositEntryNotFound,
    DepositEntryFull,
}

// CpiContext.

impl<'info> From<&Deposit<'info>> for CpiContext<'_, '_, '_, 'info, token::Transfer<'info>> {
    fn from(accs: &Deposit<'info>) -> Self {
        let program = accs.token_program.to_account_info();
        let accounts = token::Transfer {
            from: accs.deposit_token.to_account_info(),
            to: accs.exchange_vault.to_account_info(),
            authority: accs.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }
}

impl<'info> From<&Deposit<'info>> for CpiContext<'_, '_, '_, 'info, token::MintTo<'info>> {
    fn from(accs: &Deposit<'info>) -> Self {
        let program = accs.token_program.to_account_info();
        let accounts = token::MintTo {
            mint: accs.voting_mint.to_account_info(),
            to: accs.voting_token.to_account_info(),
            authority: accs.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }
}
