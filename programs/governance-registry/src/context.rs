use crate::account::*;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};
use std::mem::size_of;

#[derive(Accounts)]
#[instruction(warmup_secs: i64, registrar_bump: u8)]
pub struct CreateRegistrar<'info> {
    #[account(
        init,
        seeds = [realm.key().as_ref()],
        bump = registrar_bump,
        payer = payer,
        space = 8 + size_of::<Registrar>()
    )]
    pub registrar: AccountLoader<'info, Registrar>,
    pub realm: UncheckedAccount<'info>,
    pub authority: UncheckedAccount<'info>,
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(voter_bump: u8)]
pub struct CreateVoter<'info> {
    #[account(
        init,
        seeds = [registrar.key().as_ref(), authority.key().as_ref()],
        bump = voter_bump,
        payer = authority,
        space = 8 + size_of::<Voter>(),
    )]
    pub voter: AccountLoader<'info, Voter>,
    pub registrar: AccountLoader<'info, Registrar>,
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(idx: u16, rate: ExchangeRateEntry)]
pub struct CreateExchangeRate<'info> {
    #[account(
        init,
        payer = authority,
        associated_token::authority = registrar,
        associated_token::mint = deposit_mint,
    )]
    pub exchange_vault: Account<'info, TokenAccount>,
    #[account(
        init,
        seeds = [registrar.key().as_ref(), deposit_mint.key().as_ref()],
        bump,
        payer = authority,
        mint::authority = registrar,
        mint::freeze_authority = registrar,
        mint::decimals = deposit_mint.decimals,
    )]
    pub voting_mint: Account<'info, Mint>,
    pub deposit_mint: Account<'info, Mint>,
    #[account(mut, has_one = authority)]
    pub registrar: AccountLoader<'info, Registrar>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreateDeposit<'info> {
    pub deposit: UpdateDeposit<'info>,
}

#[derive(Accounts)]
pub struct UpdateDeposit<'info> {
    pub registrar: AccountLoader<'info, Registrar>,
    #[account(mut, has_one = authority, has_one = registrar)]
    pub voter: AccountLoader<'info, Voter>,
    #[account(
        mut,
        associated_token::authority = registrar,
        associated_token::mint = deposit_mint,
    )]
    pub exchange_vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = deposit_token.mint == deposit_mint.key(),
    )]
    pub deposit_token: Account<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::authority = authority,
        associated_token::mint = voting_mint,
    )]
    pub voting_token: Account<'info, TokenAccount>,
    pub authority: Signer<'info>,
    pub deposit_mint: Account<'info, Mint>,
    #[account(
        mut,
        seeds = [registrar.key().as_ref(), deposit_token.mint.as_ref()],
        bump,
    )]
    pub voting_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

impl<'info> UpdateDeposit<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::Transfer<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::Transfer {
            from: self.deposit_token.to_account_info(),
            to: self.exchange_vault.to_account_info(),
            authority: self.authority.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }

    pub fn thaw_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::ThawAccount<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::ThawAccount {
            account: self.voting_token.to_account_info(),
            mint: self.voting_mint.to_account_info(),
            authority: self.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }

    pub fn mint_to_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::MintTo<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::MintTo {
            mint: self.voting_mint.to_account_info(),
            to: self.voting_token.to_account_info(),
            authority: self.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }

    pub fn freeze_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::FreezeAccount<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::FreezeAccount {
            account: self.voting_token.to_account_info(),
            mint: self.voting_mint.to_account_info(),
            authority: self.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub registrar: AccountLoader<'info, Registrar>,
    #[account(mut, has_one = registrar, has_one = authority)]
    pub voter: AccountLoader<'info, Voter>,
    #[account(
        mut,
        associated_token::authority = registrar,
        associated_token::mint = withdraw_mint,
    )]
    pub exchange_vault: Account<'info, TokenAccount>,
    pub withdraw_mint: Account<'info, Mint>,
    #[account(
        mut,
        associated_token::authority = authority,
        associated_token::mint = voting_mint,
    )]
    pub voting_token: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [registrar.load()?.realm.as_ref(), voting_token.mint.as_ref()],
        bump,
    )]
    pub voting_mint: Account<'info, Mint>,
    #[account(mut)]
    pub destination: Account<'info, TokenAccount>,
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> Withdraw<'info> {
    pub fn transfer_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::Transfer<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::Transfer {
            from: self.exchange_vault.to_account_info(),
            to: self.destination.to_account_info(),
            authority: self.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }

    pub fn thaw_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::ThawAccount<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::ThawAccount {
            account: self.voting_token.to_account_info(),
            mint: self.voting_mint.to_account_info(),
            authority: self.registrar.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }

    pub fn burn_ctx(&self) -> CpiContext<'_, '_, '_, 'info, token::Burn<'info>> {
        let program = self.token_program.to_account_info();
        let accounts = token::Burn {
            mint: self.voting_mint.to_account_info(),
            to: self.voting_token.to_account_info(),
            authority: self.authority.to_account_info(),
        };
        CpiContext::new(program, accounts)
    }
}

#[derive(Accounts)]
pub struct UpdateSchedule<'info> {
    #[account(mut, has_one = authority)]
    pub voter: AccountLoader<'info, Voter>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct DecayVotingPower<'info> {
    #[account(
        seeds = [vote_weight_record.realm.as_ref()],
        bump = registrar.load()?.bump,
    )]
    pub registrar: AccountLoader<'info, Registrar>,
    #[account(
        has_one = registrar,
        has_one = authority,
    )]
    pub voter: AccountLoader<'info, Voter>,
    #[account(
        mut,
        constraint = vote_weight_record.governing_token_owner == voter.load()?.authority,
    )]
    pub vote_weight_record: Account<'info, VoterWeightRecord>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct CloseVoter<'info> {
    #[account(mut, has_one = authority, close = sol_destination)]
    pub voter: AccountLoader<'info, Voter>,
    pub authority: Signer<'info>,
    pub voting_token: Account<'info, TokenAccount>,
    pub sol_destination: UncheckedAccount<'info>,
}
