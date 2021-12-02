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
}

pub struct VoterCookie {
    pub address: Pubkey,
    pub authority: Pubkey,
    pub voter_weight_record: Pubkey,
}

impl AddinCookie {
    pub async fn create_registrar(
        &self,
        realm: &GovernanceRealmCookie,
        authority: &Keypair,
        payer: &Keypair,
    ) -> RegistrarCookie {
        let community_token_mint = realm.community_token_mint.pubkey.unwrap();

        let (registrar, registrar_bump) = Pubkey::find_program_address(
            &[
                &realm.realm.to_bytes(),
                b"registrar".as_ref(),
                &community_token_mint.to_bytes(),
            ],
            &self.program_id,
        );

        let vote_weight_decimals = 6;
        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::CreateRegistrar {
                vote_weight_decimals,
                registrar_bump,
            },
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::CreateRegistrar {
                registrar,
                governance_program_id: realm.governance.program_id,
                realm: realm.realm,
                clawback_authority: realm.authority,
                realm_governing_token_mint: community_token_mint,
                realm_authority: realm.authority,
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
        let signer1 = Keypair::from_base58_string(&payer.to_base58_string());
        let signer2 = Keypair::from_base58_string(&authority.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer1, &signer2]))
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

        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::CreateExchangeRate {
                idx: index,
                mint: deposit_mint,
                rate,
                decimals: mint.decimals,
            },
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::CreateExchangeRate {
                exchange_vault,
                deposit_mint,
                registrar: registrar.address,
                realm_authority: authority.pubkey(),
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
                b"voter".as_ref(),
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
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::CreateVoter {
                voter_bump,
                voter_weight_record_bump,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::CreateVoter {
                voter,
                voter_weight_record,
                registrar: registrar.address,
                voter_authority: authority.pubkey(),
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
            voter_weight_record,
        }
    }

    pub async fn create_deposit_entry(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        voter_authority: &Keypair,
        exchange_rate: &ExchangeRateCookie,
        lockup_kind: voter_stake_registry::account::LockupKind,
        periods: i32,
        allow_clawback: bool,
    ) -> std::result::Result<(), TransportError> {
        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::CreateDepositEntry {
                kind: lockup_kind,
                periods,
                allow_clawback,
            },
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::CreateDepositEntry {
                registrar: registrar.address,
                voter: voter.address,
                voter_authority: voter_authority.pubkey(),
                deposit_mint: exchange_rate.deposit_mint.pubkey.unwrap(),
            },
            None,
        );

        let instructions = vec![Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }];

        // clone the secrets
        let signer1 = Keypair::from_base58_string(&voter_authority.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer1]))
            .await
    }

    pub async fn deposit(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        exchange_rate: &ExchangeRateCookie,
        authority: &Keypair,
        token_address: Pubkey,
        id: u8,
        amount: u64,
    ) -> std::result::Result<(), TransportError> {
        let data =
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::Deposit {
                id,
                amount,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::Deposit {
                registrar: registrar.address,
                voter: voter.address,
                exchange_vault: exchange_rate.exchange_vault,
                deposit_token: token_address,
                deposit_authority: authority.pubkey(),
                deposit_mint: exchange_rate.deposit_mint.pubkey.unwrap(),
                token_program: spl_token::id(),
                associated_token_program: spl_associated_token_account::id(),
                system_program: solana_sdk::system_program::id(),
                rent: solana_program::sysvar::rent::id(),
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

    pub async fn clawback(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        token_owner_record: &TokenOwnerRecordCookie,
        exchange_rate: &ExchangeRateCookie,
        clawback_authority: &Keypair,
        token_address: Pubkey,
        deposit_id: u8,
    ) -> std::result::Result<(), TransportError> {
        let data =
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::Clawback {
                deposit_id,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::WithdrawOrClawback {
                registrar: registrar.address,
                voter: voter.address,
                token_owner_record: token_owner_record.address,
                exchange_vault: exchange_rate.exchange_vault,
                withdraw_mint: exchange_rate.deposit_mint.pubkey.unwrap(),
                destination: token_address,
                authority: clawback_authority.pubkey(),
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
        let signer = Keypair::from_base58_string(&clawback_authority.to_base58_string());

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
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::Withdraw {
                deposit_id,
                amount,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::WithdrawOrClawback {
                registrar: registrar.address,
                voter: voter.address,
                token_owner_record: token_owner_record.address,
                exchange_vault: exchange_rate.exchange_vault,
                withdraw_mint: exchange_rate.deposit_mint.pubkey.unwrap(),
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

    pub async fn update_voter_weight_record(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
    ) -> std::result::Result<voter_stake_registry::account::VoterWeightRecord, TransportError> {
        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::UpdateVoterWeightRecord {},
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::UpdateVoterWeightRecord {
                registrar: registrar.address,
                voter: voter.address,
                voter_weight_record: voter.voter_weight_record,
                system_program: solana_sdk::system_program::id(),
            },
            None,
        );

        let instructions = vec![Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }];

        self.solana.process_transaction(&instructions, None).await?;

        Ok(self
            .solana
            .get_account::<voter_stake_registry::account::VoterWeightRecord>(
                voter.voter_weight_record,
            )
            .await)
    }

    pub async fn close_deposit(
        &self,
        voter: &VoterCookie,
        authority: &Keypair,
        deposit_id: u8,
    ) -> Result<(), TransportError> {
        let data =
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::CloseDeposit {
                deposit_id,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::CloseDeposit {
                voter: voter.address,
                voter_authority: authority.pubkey(),
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

    pub async fn set_time_offset(
        &self,
        registrar: &RegistrarCookie,
        authority: &Keypair,
        time_offset: i64,
    ) {
        let data =
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::SetTimeOffset {
                time_offset,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::SetTimeOffset {
                registrar: registrar.address,
                realm_authority: authority.pubkey(),
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
            .unwrap();
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
            .get_account::<voter_stake_registry::account::Voter>(self.address)
            .await
            .deposits[deposit_id as usize]
            .amount_deposited_native
    }
}
