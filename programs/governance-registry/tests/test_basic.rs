use anchor_spl::token::TokenAccount;
use solana_program_test::*;
use solana_sdk::{signature::Keypair, signer::Signer, transport::TransportError};

use program_test::*;

mod program_test;

#[allow(unaligned_references)]
#[tokio::test]
async fn test_basic() -> Result<(), TransportError> {
    let context = TestContext::new().await;

    let payer = &context.users[0].key;
    let realm_authority = Keypair::new();
    let realm = context
        .governance
        .create_realm(
            "testrealm",
            realm_authority.pubkey(),
            &context.mints[0],
            &payer,
            &context.addin.program_id,
        )
        .await;

    let voter_authority = &context.users[1].key;
    let token_owner_record = realm
        .create_token_owner_record(voter_authority.pubkey(), &payer)
        .await;

    let registrar = context.addin.create_registrar(&realm, &realm_authority, payer).await;
    let mngo_rate = context
        .addin
        .create_exchange_rate(&registrar, &realm_authority, payer, 0, &context.mints[0], 1)
        .await;

    let voter = context
        .addin
        .create_voter(&registrar, &voter_authority, &payer)
        .await;

    // test deposit and withdraw

    let reference_account = context.users[1].token_accounts[0];
    let reference_initial = context
        .solana
        .token_account_balance(reference_account)
        .await;
    let vault_initial = mngo_rate.vault_balance(&context.solana).await;
    assert_eq!(vault_initial, 0);
    let balance_initial = voter.deposit_amount(&context.solana, 0).await;
    assert_eq!(balance_initial, 0);

    context
        .addin
        .create_deposit(
            &registrar,
            &voter,
            voter_authority,
            &mngo_rate,
            &voter_authority,
            reference_account,
            governance_registry::account::LockupKind::Cliff,
            10000,
            0,
            false,
        )
        .await?;

    let reference_after_deposit = context
        .solana
        .token_account_balance(reference_account)
        .await;
    assert_eq!(reference_initial, reference_after_deposit + 10000);
    let vault_after_deposit = mngo_rate.vault_balance(&context.solana).await;
    assert_eq!(vault_after_deposit, 10000);
    let balance_after_deposit = voter.deposit_amount(&context.solana, 0).await;
    assert_eq!(balance_after_deposit, 10000);

    context
        .addin
        .withdraw(
            &registrar,
            &voter,
            &token_owner_record,
            &mngo_rate,
            &voter_authority,
            reference_account,
            0,
            10000,
        )
        .await
        .expect_err("fails because a deposit happened in the same slot");

    // Must advance slots because withdrawing in the same slot as the deposit is forbidden
    context.solana.advance_clock_by_slots(2).await;

    context
        .addin
        .withdraw(
            &registrar,
            &voter,
            &token_owner_record,
            &mngo_rate,
            &voter_authority,
            reference_account,
            0,
            10000,
        )
        .await?;

    let reference_after_withdraw = context
        .solana
        .token_account_balance(reference_account)
        .await;
    assert_eq!(reference_initial, reference_after_withdraw);
    let vault_after_withdraw = mngo_rate.vault_balance(&context.solana).await;
    assert_eq!(vault_after_withdraw, 0);
    let balance_after_withdraw = voter.deposit_amount(&context.solana, 0).await;
    assert_eq!(balance_after_withdraw, 0);

    Ok(())
}
