use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod governance_registry {
    use super::*;

    /// Creates a new voting registrar.
    pub fn init_registrar(
        ctx: Context<InitRegistrar>,
        _voting_mint_decimals: u8,
        registrar_bump: u8,
        voting_mint_bump: u8,
    ) -> Result<()> {
        let registrar = &mut ctx.accounts.registrar;
        registrar.registrar_bump = registrar_bump;
        registrar.voting_mint_bump = voting_mint_bump;
        registrar.realm = ctx.accounts.realm.key();
        registrar.voting_mint = ctx.accounts.voting_mint.key();
        registrar.authority = ctx.accounts.authority.key();
        if true {
            panic!("HELLO WORLD");
        }
        Ok(())
    }

    /// Creates a new voter account.
    pub fn init_voter(ctx: Context<InitVoter>, voter_bump: u8) -> Result<()> {
        let voter = &mut ctx.accounts.voter;
        voter.voter_bump = voter_bump;
        voter.authority = ctx.accounts.authority.key();
        voter.registrar = ctx.accounts.registrar.key();
        voter.rights_outstanding = 0;

        Ok(())
    }

    /// Creates a new exchange rate for a given mint. The exchange rate
    /// allows one to deposit one token into the registrar and receive voting
    /// tokens in response. Only the registrar authority can invoke this.
    pub fn add_exchange_rate(
        ctx: Context<AddExchangeRate>,
        rate: u64,
        rate_bump: u8,
        vault_bump: u8,
    ) -> Result<()> {
        require!(rate > 0, InvalidRate);

        let rate = &mut ctx.accounts.exchange_rate;
        rate.deposit_mint = ctx.accounts.deposit_mint.key();
        rate.rate_bump = rate_bump;
        rate.vault_bump = vault_bump;

        Ok(())
    }

    /// Deposits tokens into the registrar in exchange for voting rights that
    /// can be used with a DAO.
    pub fn mint_voting_rights(ctx: Context<MintVotingRights>, amount: u64) -> Result<()> {
        // Deposit tokens into the registrar.
        token::transfer((&*ctx.accounts).into(), amount)?;

        // Calculate the amount of voting tokens to mint.
        let scaled_amount = { amount };

        // Mint vote tokens to the depositor.
        token::mint_to((&*ctx.accounts).into(), scaled_amount)?;

        Ok(())
    }

    /// Updates the Voter's voting rights by decaying locked deposits.
    pub fn decay_voting_rights(ctx: Context<DecayVotingRights>) -> Result<()> {
        // todo
        Ok(())
    }
}

// Contexts.

#[derive(Accounts)]
#[instruction(voting_mint_decimals: u8, registrar_bump: u8, voting_mint_bump: u8)]
pub struct InitRegistrar<'info> {
    #[account(
        init,
        seeds = [realm.key().as_ref()],
        bump = registrar_bump,
        payer = payer,
    )]
    registrar: Account<'info, Registrar>,
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
    )]
    voter: Account<'info, Voter>,
    registrar: Account<'info, Registrar>,
    authority: Signer<'info>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(rate_bump: u8, vault_bump: u8)]
pub struct AddExchangeRate<'info> {
    #[account(
        init,
        seeds = [b"exchange-rate", registrar.key().as_ref(), deposit_mint.key().as_ref()],
        bump = rate_bump,
        payer = payer,
    )]
    exchange_rate: Account<'info, ExchangeRate>,
    #[account(
        init,
        seeds = [b"exchange-vault", exchange_rate.key().as_ref()],
        bump = vault_bump,
        payer = payer,
        token::authority = registrar,
        token::mint = deposit_mint,
    )]
    exchange_vault: Account<'info, TokenAccount>,
    deposit_mint: Account<'info, Mint>,
    #[account(has_one = authority)]
    registrar: Account<'info, Registrar>,
    authority: Signer<'info>,
    payer: Signer<'info>,
    rent: Sysvar<'info, Rent>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MintVotingRights<'info> {
    #[account(
        seeds = [b"exchange-rate", registrar.key().as_ref(), deposit_mint.key().as_ref()],
        bump = exchange_rate.rate_bump,
    )]
    exchange_rate: Account<'info, ExchangeRate>,
    #[account(
        seeds = [b"exchange-vault", exchange_rate.key().as_ref()],
        bump = exchange_rate.vault_bump,
    )]
    exchange_vault: Account<'info, TokenAccount>,
    #[account(
        constraint = deposit_token.mint == deposit_mint.key(),
    )]
    deposit_token: Account<'info, TokenAccount>,
    #[account(
        constraint = registrar.voting_mint == voting_token.mint,
    )]
    voting_token: Account<'info, TokenAccount>,
    authority: Signer<'info>,
    registrar: Account<'info, Registrar>,
    deposit_mint: Account<'info, Mint>,
    voting_mint: Account<'info, Mint>,
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct DecayVotingRights<'info> {
    voter: Account<'info, Voter>,
    deposit: Account<'info, VoterDeposit>,
}

// Accounts.

/// Instance of a voting rights distributor.
#[account]
#[derive(Default)]
pub struct Registrar {
    pub authority: Pubkey,
    pub realm: Pubkey,
    pub voting_mint: Pubkey,
    pub voting_mint_bump: u8,
    pub registrar_bump: u8,
}

/// User account for minting voting rights.
#[account]
#[derive(Default)]
pub struct Voter {
    pub authority: Pubkey,
    pub registrar: Pubkey,
    pub rights_outstanding: u64,
    pub voter_bump: u8,
}

pub struct Deposit {
    pub mint_idx: u8,
    pub lockup_years: u8,
    pub amount: u64,
}

/// Sub account for a
#[account]
pub struct VoterDeposit {
    amount: u64,
    exchange_rate: Pubkey,
}

/// Exchange rate for an asset that can be used to mint voting rights.
#[account]
#[derive(Default)]
pub struct ExchangeRate {
    deposit_mint: Pubkey,
    rate: u64,
    rate_bump: u8,
    vault_bump: u8,

    // Locked state.
    period_count: u64,
    start_ts: i64,
    end_ts: i64,
}

// Error.

#[error]
pub enum ErrorCode {
    #[msg("Exchange rate must be greater than zero")]
    InvalidRate,
}

// CpiContext.

impl<'info> From<&MintVotingRights<'info>>
    for CpiContext<'_, '_, '_, 'info, token::Transfer<'info>>
{
    fn from(accs: &MintVotingRights<'info>) -> Self {
        let program = accs.token_program.to_account_info();
        let accounts = token::Transfer {
            from: accs.deposit_token.to_account_info(),
            to: accs.exchange_vault.to_account_info(),
            authority: accs.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }
}

impl<'info> From<&MintVotingRights<'info>> for CpiContext<'_, '_, '_, 'info, token::MintTo<'info>> {
    fn from(accs: &MintVotingRights<'info>) -> Self {
        let program = accs.token_program.to_account_info();
        let accounts = token::MintTo {
            mint: accs.voting_mint.to_account_info(),
            to: accs.voting_token.to_account_info(),
            authority: accs.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }
}
