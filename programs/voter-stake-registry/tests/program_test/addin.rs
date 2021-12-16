use anchor_lang::Key;
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

#[derive(Clone)]
pub struct VotingMintConfigCookie {
    pub mint: MintCookie,
}

pub struct VoterCookie {
    pub address: Pubkey,
    pub authority: Pubkey,
    pub voter_weight_record: Pubkey,
    pub token_owner_record: Pubkey,
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

        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::CreateRegistrar { registrar_bump },
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
            mint: realm.community_token_mint.clone(),
        }
    }

    pub async fn configure_voting_mint(
        &self,
        registrar: &RegistrarCookie,
        authority: &Keypair,
        payer: &Keypair,
        index: u16,
        mint: &MintCookie,
        digit_shift: i8,
        deposit_scaled_factor: f64,
        lockup_scaled_factor: f64,
        lockup_saturation_secs: u64,
        grant_authority: Option<Pubkey>,
    ) -> VotingMintConfigCookie {
        let deposit_mint = mint.pubkey.unwrap();

        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::ConfigureVotingMint {
                idx: index,
                digit_shift,
                deposit_scaled_factor: (deposit_scaled_factor * 1e9) as u64,
                lockup_scaled_factor: (lockup_scaled_factor * 1e9) as u64,
                lockup_saturation_secs,
                grant_authority,
            },
        );

        let mut accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::ConfigureVotingMint {
                mint: deposit_mint,
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
        accounts.push(anchor_lang::prelude::AccountMeta::new_readonly(
            deposit_mint,
            false,
        ));

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

        VotingMintConfigCookie { mint: mint.clone() }
    }

    pub async fn create_voter(
        &self,
        registrar: &RegistrarCookie,
        token_owner_record: &TokenOwnerRecordCookie,
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
                &registrar.address.to_bytes(),
                b"voter-weight-record".as_ref(),
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
            token_owner_record: token_owner_record.address,
        }
    }

    pub async fn create_deposit_entry(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        voter_authority: &Keypair,
        voting_mint: &VotingMintConfigCookie,
        deposit_entry_index: u8,
        lockup_kind: voter_stake_registry::state::LockupKind,
        periods: u32,
        allow_clawback: bool,
    ) -> std::result::Result<(), TransportError> {
        let vault = spl_associated_token_account::get_associated_token_address(
            &voter.address,
            &voting_mint.mint.pubkey.unwrap(),
        );

        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::CreateDepositEntry {
                deposit_entry_index,
                kind: lockup_kind,
                start_ts: None,
                periods,
                allow_clawback,
            },
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::CreateDepositEntry {
                vault,
                registrar: registrar.address,
                voter: voter.address,
                voter_authority: voter_authority.pubkey(),
                deposit_mint: voting_mint.mint.pubkey.unwrap(),
                system_program: solana_sdk::system_program::id(),
                token_program: spl_token::id(),
                associated_token_program: spl_associated_token_account::id(),
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
        let signer1 = Keypair::from_base58_string(&voter_authority.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer1]))
            .await
    }

    #[allow(dead_code)]
    pub async fn deposit(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        voting_mint: &VotingMintConfigCookie,
        authority: &Keypair,
        token_address: Pubkey,
        deposit_entry_index: u8,
        amount: u64,
    ) -> std::result::Result<(), TransportError> {
        let vault = spl_associated_token_account::get_associated_token_address(
            &voter.address,
            &voting_mint.mint.pubkey.unwrap(),
        );

        let data =
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::Deposit {
                deposit_entry_index,
                amount,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::Deposit {
                registrar: registrar.address,
                voter: voter.address,
                vault: vault,
                deposit_token: token_address,
                deposit_authority: authority.pubkey(),
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

    #[allow(dead_code)]
    pub async fn grant(
        &self,
        registrar: &RegistrarCookie,
        voter_authority: Pubkey,
        voting_mint: &VotingMintConfigCookie,
        lockup_kind: voter_stake_registry::state::LockupKind,
        periods: u32,
        allow_clawback: bool,
        amount: u64,
        deposit_token: Pubkey,
        authority: &Keypair,
    ) -> std::result::Result<VoterCookie, TransportError> {
        let (voter, voter_bump) = Pubkey::find_program_address(
            &[
                &registrar.address.to_bytes(),
                b"voter".as_ref(),
                &voter_authority.to_bytes(),
            ],
            &self.program_id,
        );
        let vault = spl_associated_token_account::get_associated_token_address(
            &voter.key(),
            &voting_mint.mint.pubkey.unwrap(),
        );
        let (voter_weight_record, voter_weight_record_bump) = Pubkey::find_program_address(
            &[
                &registrar.address.to_bytes(),
                b"voter-weight-record".as_ref(),
                &voter_authority.to_bytes(),
            ],
            &self.program_id,
        );

        let data = anchor_lang::InstructionData::data(&voter_stake_registry::instruction::Grant {
            voter_bump,
            voter_weight_record_bump,
            kind: lockup_kind,
            start_ts: None,
            periods,
            allow_clawback,
            amount,
        });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::Grant {
                registrar: registrar.address,
                voter,
                voter_authority,
                voter_weight_record,
                vault,
                deposit_token,
                authority: authority.pubkey(),
                payer: authority.pubkey(),
                deposit_mint: voting_mint.mint.pubkey.unwrap(),
                system_program: solana_sdk::system_program::id(),
                token_program: spl_token::id(),
                associated_token_program: spl_associated_token_account::id(),
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
        let signer1 = Keypair::from_base58_string(&authority.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer1]))
            .await?;

        Ok(VoterCookie {
            address: voter,
            authority: voter_authority,
            voter_weight_record,
            token_owner_record: Pubkey::new_unique(), // don't have it
        })
    }

    #[allow(dead_code)]
    pub async fn clawback(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        voting_mint: &VotingMintConfigCookie,
        clawback_authority: &Keypair,
        token_address: Pubkey,
        deposit_entry_index: u8,
    ) -> std::result::Result<(), TransportError> {
        let vault = spl_associated_token_account::get_associated_token_address(
            &voter.address,
            &voting_mint.mint.pubkey.unwrap(),
        );

        let data =
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::Clawback {
                deposit_entry_index,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::Clawback {
                registrar: registrar.address,
                voter: voter.address,
                token_owner_record: voter.token_owner_record,
                vault,
                destination: token_address,
                clawback_authority: clawback_authority.pubkey(),
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

    #[allow(dead_code)]
    pub async fn withdraw(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        voting_mint: &VotingMintConfigCookie,
        authority: &Keypair,
        token_address: Pubkey,
        deposit_entry_index: u8,
        amount: u64,
    ) -> std::result::Result<(), TransportError> {
        let vault = spl_associated_token_account::get_associated_token_address(
            &voter.address,
            &voting_mint.mint.pubkey.unwrap(),
        );

        let data =
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::Withdraw {
                deposit_entry_index,
                amount,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::Withdraw {
                registrar: registrar.address,
                voter: voter.address,
                token_owner_record: voter.token_owner_record,
                voter_weight_record: voter.voter_weight_record,
                vault,
                destination: token_address,
                voter_authority: authority.pubkey(),
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

    pub fn update_voter_weight_record_instruction(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
    ) -> Instruction {
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

        Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }
    }

    #[allow(dead_code)]
    pub async fn update_voter_weight_record(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
    ) -> std::result::Result<voter_stake_registry::state::VoterWeightRecord, TransportError> {
        let instructions = vec![self.update_voter_weight_record_instruction(registrar, voter)];

        self.solana.process_transaction(&instructions, None).await?;

        Ok(self
            .solana
            .get_account::<voter_stake_registry::state::VoterWeightRecord>(
                voter.voter_weight_record,
            )
            .await)
    }

    #[allow(dead_code)]
    pub async fn close_deposit_entry(
        &self,
        voter: &VoterCookie,
        authority: &Keypair,
        deposit_entry_index: u8,
    ) -> Result<(), TransportError> {
        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::CloseDepositEntry {
                deposit_entry_index,
            },
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::CloseDepositEntry {
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

    #[allow(dead_code)]
    pub async fn reset_lockup(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        authority: &Keypair,
        deposit_entry_index: u8,
        kind: voter_stake_registry::state::LockupKind,
        periods: u32,
    ) -> Result<(), TransportError> {
        let data =
            anchor_lang::InstructionData::data(&voter_stake_registry::instruction::ResetLockup {
                deposit_entry_index,
                kind,
                periods,
            });

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::ResetLockup {
                registrar: registrar.address,
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

    #[allow(dead_code)]
    pub async fn internal_transfer(
        &self,
        registrar: &RegistrarCookie,
        voter: &VoterCookie,
        authority: &Keypair,
        source_deposit_entry_index: u8,
        target_deposit_entry_index: u8,
        amount: u64,
    ) -> Result<(), TransportError> {
        let data = anchor_lang::InstructionData::data(
            &voter_stake_registry::instruction::InternalTransfer {
                source_deposit_entry_index,
                target_deposit_entry_index,
                amount,
            },
        );

        let accounts = anchor_lang::ToAccountMetas::to_account_metas(
            &voter_stake_registry::accounts::InternalTransfer {
                registrar: registrar.address,
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

    #[allow(dead_code)]
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

impl VotingMintConfigCookie {
    #[allow(dead_code)]
    pub async fn vault_balance(&self, solana: &SolanaCookie, voter: &VoterCookie) -> u64 {
        let vault = spl_associated_token_account::get_associated_token_address(
            &voter.address,
            &self.mint.pubkey.unwrap(),
        );
        solana.get_account::<TokenAccount>(vault).await.amount
    }
}

impl VoterCookie {
    #[allow(dead_code)]
    pub async fn deposit_amount(&self, solana: &SolanaCookie, deposit_id: u8) -> u64 {
        solana
            .get_account::<voter_stake_registry::state::Voter>(self.address)
            .await
            .deposits[deposit_id as usize]
            .amount_deposited_native
    }
}
