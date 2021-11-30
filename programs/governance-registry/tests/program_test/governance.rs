use std::sync::Arc;

use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};

use crate::*;

#[derive(Clone)]
pub struct GovernanceCookie {
    pub solana: Arc<solana::SolanaCookie>,
    pub program_id: Pubkey,
}

#[derive(Clone)]
pub struct GovernanceRealmCookie {
    pub governance: GovernanceCookie,
    pub authority: Pubkey,
    pub authority_token: Pubkey,
    pub realm: Pubkey,
    pub realm_config: Pubkey,
    pub community_token_mint: MintCookie,
    pub community_token_account: Pubkey,
}

#[derive(Clone)]
pub struct TokenOwnerRecordCookie {
    pub address: Pubkey,
}

impl GovernanceCookie {
    pub async fn create_realm(
        &self,
        name: &str,
        realm_authority: Pubkey,
        realm_authority_token: Pubkey,
        community_token_mint: &MintCookie,
        payer: &Keypair,
        voter_weight_addin: &Pubkey,
    ) -> GovernanceRealmCookie {
        let realm = Pubkey::find_program_address(
            &[b"governance".as_ref(), name.as_ref()],
            &self.program_id,
        )
        .0;
        let community_token_account = Pubkey::find_program_address(
            &[
                b"governance".as_ref(),
                &realm.to_bytes(),
                &community_token_mint.pubkey.unwrap().to_bytes(),
            ],
            &self.program_id,
        )
        .0;
        let realm_config = Pubkey::find_program_address(
            &[b"realm-config".as_ref(), &realm.to_bytes()],
            &self.program_id,
        )
        .0;

        let instructions = vec![spl_governance::instruction::create_realm(
            &self.program_id,
            &realm_authority,
            &community_token_mint.pubkey.unwrap(),
            &payer.pubkey(),
            None,
            Some(*voter_weight_addin),
            name.to_string(),
            0,
            spl_governance::state::enums::MintMaxVoteWeightSource::SupplyFraction(10000000000),
        )];

        let signer = Keypair::from_base58_string(&payer.to_base58_string());

        self.solana
            .process_transaction(&instructions, Some(&[&signer]))
            .await
            .unwrap();

        GovernanceRealmCookie {
            governance: self.clone(),
            authority: realm_authority,
            authority_token: realm_authority_token,
            realm,
            realm_config,
            community_token_mint: community_token_mint.clone(),
            community_token_account,
        }
    }
}

impl GovernanceRealmCookie {
    pub async fn create_token_owner_record(
        &self,
        owner: Pubkey,
        payer: &Keypair,
    ) -> TokenOwnerRecordCookie {
        let record = Pubkey::find_program_address(
            &[
                b"governance".as_ref(),
                &self.realm.to_bytes(),
                &self.community_token_mint.pubkey.unwrap().to_bytes(),
                &owner.to_bytes(),
            ],
            &self.governance.program_id,
        )
        .0;

        let instructions = vec![spl_governance::instruction::create_token_owner_record(
            &self.governance.program_id,
            &self.realm,
            &owner,
            &self.community_token_mint.pubkey.unwrap(),
            &payer.pubkey(),
        )];

        let signer = Keypair::from_base58_string(&payer.to_base58_string());

        self.governance
            .solana
            .process_transaction(&instructions, Some(&[&signer]))
            .await
            .unwrap();

        TokenOwnerRecordCookie { address: record }
    }
}
