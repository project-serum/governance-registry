use std::sync::Arc;

use solana_sdk::pubkey::Pubkey;
use solana_sdk::transport::TransportError;
use solana_sdk::{
    instruction::Instruction,
    signature::{Keypair, Signer},
};

use crate::*;

#[derive(Clone)]
pub struct AddinCookie {
    pub solana: Arc<solana::SolanaCookie>,
    pub program_id: Pubkey,
}

pub struct RegistrarCookie {
    pub address: Pubkey,
    pub authority: Pubkey,
    pub mint: MintCookie,
}

#[derive(Clone, Copy)]
pub struct ExchangeRateCookie {
    pub deposit_mint: MintCookie,
    pub exchange_vault: Pubkey,
    pub voting_mint: Pubkey,
}

pub struct VoterCookie {
    pub address: Pubkey,
    pub authority: Pubkey,
}

impl AddinCookie {
    pub async fn create_registrar(
        &self,
        realm: &GovernanceRealmCookie,
        payer: &Keypair,
    ) -> RegistrarCookie {
        let (registrar, registrar_bump) =
            Pubkey::find_program_address(&[&realm.realm.to_bytes()], &self.program_id);

        let community_token_mint = realm.community_token_mint.pubkey.unwrap();

        let rate_decimals = 6;
        let data = anchor_lang::InstructionData::data(
            &governance_registry::instruction::CreateRegistrar {
                rate_decimals,
                registrar_bump,
            },
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &governance_registry::accounts::CreateRegistrar {
                registrar,
                governance_program_id: realm.governance.program_id,
                realm: realm.realm,
                realm_community_mint: community_token_mint,
                authority: realm.authority,
                payer: payer.pubkey(),
                system_program: solana_sdk::system_program::id(),
                token_program: spl_token::id(),
                rent: solana_program::sysvar::rent::id(),
            },
            None,
        );

        let instructions = vec![Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }];

        // clone the user secret
        let signer = Keypair::from_base58_string(&payer.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer]))
            .await
            .unwrap();

        RegistrarCookie {
            address: registrar,
            authority: realm.authority,
            mint: realm.community_token_mint,
        }
    }

    pub async fn create_exchange_rate(
        &self,
        registrar: &RegistrarCookie,
        authority: &Keypair,
        payer: &Keypair,
        index: u16,
        mint: &MintCookie,
        rate: u64,
    ) -> ExchangeRateCookie {
        let deposit_mint = mint.pubkey.unwrap();
        let exchange_vault = spl_associated_token_account::get_associated_token_address(
            &registrar.address,
            &deposit_mint,
        );
        let (voting_mint, _voting_mint_bump) = Pubkey::find_program_address(
            &[&registrar.address.to_bytes(), &deposit_mint.to_bytes()],
            &self.program_id,
        );

        let data = anchor_lang::InstructionData::data(
            &governance_registry::instruction::CreateExchangeRate {
                idx: index,
                mint: deposit_mint,
                rate,
                decimals: mint.decimals,
            },
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &governance_registry::accounts::CreateExchangeRate {
                exchange_vault,
                voting_mint,
                deposit_mint,
                registrar: registrar.address,
                authority: authority.pubkey(),
                payer: payer.pubkey(),
                rent: solana_program::sysvar::rent::id(),
                token_program: spl_token::id(),
                associated_token_program: spl_associated_token_account::id(),
                system_program: solana_sdk::system_program::id(),
            },
            None,
        );

        let instructions = vec![Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }];

        // clone the user secret
        let signer1 = Keypair::from_base58_string(&payer.to_base58_string());
        let signer2 = Keypair::from_base58_string(&authority.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer1, &signer2]))
            .await
            .unwrap();

        ExchangeRateCookie {
            deposit_mint: mint.clone(),
            exchange_vault,
            voting_mint,
        }
    }

    pub async fn create_voter(
        &self,
        registrar: &RegistrarCookie,
        authority: &Keypair,
        payer: &Keypair,
    ) -> VoterCookie {
        let (voter, voter_bump) = Pubkey::find_program_address(
            &[
                &registrar.address.to_bytes(),
                &authority.pubkey().to_bytes(),
            ],
            &self.program_id,
        );
        let (voter_weight_record, voter_weight_record_bump) = Pubkey::find_program_address(
            &[
                b"voter-weight-record".as_ref(),
                &registrar.address.to_bytes(),
                &authority.pubkey().to_bytes(),
            ],
            &self.program_id,
        );

        let data =
            anchor_lang::InstructionData::data(&governance_registry::instruction::CreateVoter {
                voter_bump,
                voter_weight_record_bump,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &governance_registry::accounts::CreateVoter {
                voter,
                voter_weight_record,
                registrar: registrar.address,
                authority: authority.pubkey(),
                payer: payer.pubkey(),
                token_program: spl_token::id(),
                associated_token_program: spl_associated_token_account::id(),
                system_program: solana_sdk::system_program::id(),
                rent: solana_program::sysvar::rent::id(),
                instructions: solana_program::sysvar::instructions::id(),
            },
            None,
        );

        let instructions = vec![Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }];

        // clone the secrets
        let signer1 = Keypair::from_base58_string(&payer.to_base58_string());
        let signer2 = Keypair::from_base58_string(&authority.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer1, &signer2]))
            .await
            .unwrap();

        VoterCookie {
            address: voter,
            authority: authority.pubkey(),
        }
    }

    pub async fn create_deposit(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        exchange_rate: &ExchangeRateCookie,
        authority: &Keypair,
        token_address: Pubkey,
        lockup_kind: governance_registry::account::LockupKind,
        amount: u64,
        days: i32,
    ) -> std::result::Result<(), TransportError> {
        let data =
            anchor_lang::InstructionData::data(&governance_registry::instruction::CreateDeposit {
                kind: lockup_kind,
                amount,
                days,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &governance_registry::accounts::CreateDeposit {
                deposit: governance_registry::accounts::UpdateDeposit {
                    registrar: registrar.address,
                    voter: voter.address,
                    exchange_vault: exchange_rate.exchange_vault,
                    deposit_token: token_address,
                    voting_token: voter.voting_token(exchange_rate),
                    authority: authority.pubkey(),
                    deposit_mint: exchange_rate.deposit_mint.pubkey.unwrap(),
                    voting_mint: exchange_rate.voting_mint,
                    token_program: spl_token::id(),
                    associated_token_program: spl_associated_token_account::id(),
                    system_program: solana_sdk::system_program::id(),
                    rent: solana_program::sysvar::rent::id(),
                },
            },
            None,
        );

        let instructions = vec![Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }];

        // clone the secrets
        let signer = Keypair::from_base58_string(&authority.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer]))
            .await
    }

    pub async fn withdraw(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        token_owner_record: &TokenOwnerRecordCookie,
        exchange_rate: &ExchangeRateCookie,
        authority: &Keypair,
        token_address: Pubkey,
        deposit_id: u8,
        amount: u64,
    ) -> std::result::Result<(), TransportError> {
        let data =
            anchor_lang::InstructionData::data(&governance_registry::instruction::Withdraw {
                deposit_id,
                amount,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &governance_registry::accounts::Withdraw {
                registrar: registrar.address,
                voter: voter.address,
                token_owner_record: token_owner_record.address,
                exchange_vault: exchange_rate.exchange_vault,
                withdraw_mint: exchange_rate.deposit_mint.pubkey.unwrap(),
                voting_token: voter.voting_token(exchange_rate),
                voting_mint: exchange_rate.voting_mint,
                destination: token_address,
                authority: authority.pubkey(),
                token_program: spl_token::id(),
            },
            None,
        );

        let instructions = vec![Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }];

        // clone the secrets
        let signer = Keypair::from_base58_string(&authority.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer]))
            .await
    }
}

impl ExchangeRateCookie {
    pub async fn vault_balance(&self, solana: &SolanaCookie) -> u64 {
        solana
            .get_account::<TokenAccount>(self.exchange_vault)
            .await
            .amount
    }
}

impl VoterCookie {
    pub async fn deposit_amount(&self, solana: &SolanaCookie, deposit_id: u8) -> u64 {
        solana
            .get_account::<governance_registry::account::Voter>(self.address)
            .await
            .deposits[deposit_id as usize]
            .amount_deposited_native
    }

    pub fn voting_token(&self, rate: &ExchangeRateCookie) -> Pubkey {
        spl_associated_token_account::get_associated_token_address(
            &self.authority,
            &rate.voting_mint,
        )
    }
}
