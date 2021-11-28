use std::cell::RefCell;
use std::{str::FromStr, sync::Arc};

use solana_program::{program_option::COption, program_pack::Pack};
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use spl_token::{state::*, *};

pub use addin::*;
pub use cookies::*;
pub use governance::*;
pub use solana::*;
pub use utils::*;

pub mod addin;
pub mod cookies;
pub mod governance;
pub mod solana;
pub mod utils;

trait AddPacked {
    fn add_packable_account<T: Pack>(
        &mut self,
        pubkey: Pubkey,
        amount: u64,
        data: &T,
        owner: &Pubkey,
    );
}

impl AddPacked for ProgramTest {
    fn add_packable_account<T: Pack>(
        &mut self,
        pubkey: Pubkey,
        amount: u64,
        data: &T,
        owner: &Pubkey,
    ) {
        let mut account = solana_sdk::account::Account::new(amount, T::get_packed_len(), owner);
        data.pack_into_slice(&mut account.data);
        self.add_account(pubkey, account);
    }
}

pub struct TestContext {
    pub solana: Arc<SolanaCookie>,
    pub governance: GovernanceCookie,
    pub addin: AddinCookie,
    pub mints: Vec<MintCookie>,
    pub users: Vec<UserCookie>,
    pub quote_index: usize,
}

impl TestContext {
    pub async fn new() -> Self {
        let addin_program_id = governance_registry::id();

        let mut test = ProgramTest::new(
            "governance_registry",
            addin_program_id,
            processor!(governance_registry::entry),
        );
        test.set_bpf_compute_max_units(200000);

        let governance_program_id =
            Pubkey::from_str(&"GovernanceProgram11111111111111111111111111").unwrap();
        test.add_program(
            "spl_governance",
            governance_program_id,
            processor!(spl_governance::processor::process_instruction),
        );

        // Supress some of the logs
        solana_logger::setup_with_default(
            "solana_rbpf=trace,\
                solana_runtime::message_processor=debug,\
                solana_runtime::system_instruction_processor=trace,\
                solana_program_test=info",
        );
        // Disable all logs except error
        // solana_logger::setup_with("error");

        // Setup the environment

        // Mints
        let mut mints: Vec<MintCookie> = vec![
            MintCookie {
                index: 0,
                decimals: 6,
                unit: 10u64.pow(6) as f64,
                base_lot: 100 as f64,
                quote_lot: 10 as f64,
                pubkey: None, //Some(mngo_token::ID),
            }, // symbol: "MNGO".to_string()
            MintCookie {
                index: 1,
                decimals: 6,
                unit: 10u64.pow(6) as f64,
                base_lot: 0 as f64,
                quote_lot: 0 as f64,
                pubkey: None,
            }, // symbol: "USDC".to_string()
        ];
        // Add mints in loop
        for mint_index in 0..mints.len() {
            let mint_pk: Pubkey;
            if mints[mint_index].pubkey.is_none() {
                mint_pk = Pubkey::new_unique();
            } else {
                mint_pk = mints[mint_index].pubkey.unwrap();
            }

            test.add_packable_account(
                mint_pk,
                u32::MAX as u64,
                &Mint {
                    is_initialized: true,
                    mint_authority: COption::Some(Pubkey::new_unique()),
                    decimals: mints[mint_index].decimals,
                    ..Mint::default()
                },
                &spl_token::id(),
            );
            mints[mint_index].pubkey = Some(mint_pk);
        }
        let quote_index = mints.len() - 1;

        // Users
        let num_users = 4;
        let mut users = Vec::new();
        for _ in 0..num_users {
            let user_key = Keypair::new();
            test.add_account(
                user_key.pubkey(),
                solana_sdk::account::Account::new(
                    u32::MAX as u64,
                    0,
                    &solana_sdk::system_program::id(),
                ),
            );

            // give every user 10^18 (< 2^60) of every token
            // ~~ 1 trillion in case of 6 decimals
            let mut token_accounts = Vec::new();
            for mint_index in 0..mints.len() {
                let token_key = Pubkey::new_unique();
                test.add_packable_account(
                    token_key,
                    u32::MAX as u64,
                    &spl_token::state::Account {
                        mint: mints[mint_index].pubkey.unwrap(),
                        owner: user_key.pubkey(),
                        amount: 1_000_000_000_000_000_000,
                        state: spl_token::state::AccountState::Initialized,
                        ..spl_token::state::Account::default()
                    },
                    &spl_token::id(),
                );

                token_accounts.push(token_key);
            }
            users.push(UserCookie {
                key: user_key,
                token_accounts,
            });
        }

        let mut context = test.start_with_context().await;
        let rent = context.banks_client.get_rent().await.unwrap();

        let solana = Arc::new(SolanaCookie {
            context: RefCell::new(context),
            rent,
        });

        TestContext {
            solana: solana.clone(),
            governance: GovernanceCookie {
                solana: solana.clone(),
                program_id: governance_program_id,
            },
            addin: AddinCookie {
                solana: solana.clone(),
                program_id: addin_program_id,
            },
            mints,
            users,
            quote_index,
        }
    }
}
