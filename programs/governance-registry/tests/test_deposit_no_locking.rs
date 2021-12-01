use anchor_spl::token::TokenAccount;
use solana_program_test::*;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer, transport::TransportError};

use program_test::*;

mod program_test;

struct Balances {
    token: u64,
    vault: u64,
    deposit: u64,
    voter_weight: u64,
}

async fn balances(
    context: &TestContext,
    registrar: &RegistrarCookie,
    address: Pubkey,
    voter: &VoterCookie,
    voter_authority: &Keypair,
    rate: &ExchangeRateCookie,
    deposit_id: u8,
) -> Balances {
    // Advance slots to avoid caching of the UpdateVoterWeightRecord call
    // TODO: Is this something that could be an issue on a live node?
    context.solana.advance_clock_by_slots(2).await;

    let token = context.solana.token_account_balance(address).await;
    let vault = rate.vault_balance(&context.solana).await;
    let deposit = voter.deposit_amount(&context.solana, deposit_id).await;
    let vwr = context
        .addin
        .update_voter_weight_record(&registrar, &voter, &voter_authority)
        .await
        .unwrap();
    Balances {
        token,
        vault,
        deposit,
        voter_weight: vwr.voter_weight,
    }
}

#[allow(unaligned_references)]
#[tokio::test]
async fn test_deposit_no_locking() -> Result<(), TransportError> {
    let context = TestContext::new().await;
    let addin = &context.addin;

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

    let registrar = addin.create_registrar(&realm, payer).await;
    let mngo_rate = addin
        .create_exchange_rate(&registrar, &realm_authority, payer, 0, &context.mints[0], 1)
        .await;

    let voter = addin
        .create_voter(&registrar, &voter_authority, &payer)
        .await;

    let voter2_authority = &context.users[2].key;
    let voter2 = addin
        .create_voter(&registrar, &voter2_authority, &payer)
        .await;

    let reference_account = context.users[1].token_accounts[0];
    let get_balances = |depot_id| {
        balances(
            &context,
            &registrar,
            reference_account,
            &voter,
            &voter_authority,
            &mngo_rate,
            depot_id,
        )
    };
    let withdraw = |amount: u64| {
        addin.withdraw(
            &registrar,
            &voter,
            &token_owner_record,
            &mngo_rate,
            &voter_authority,
            reference_account,
            0,
            amount,
        )
    };
    let update_deposit = |amount: u64| {
        addin.update_deposit(
            &registrar,
            &voter,
            &mngo_rate,
            &voter_authority,
            &voter_authority,
            reference_account,
            0,
            amount,
        )
    };
    // test deposit and withdraw

    let initial = get_balances(0).await;
    assert_eq!(initial.vault, 0);
    assert_eq!(initial.deposit, 0);

    addin
        .create_deposit(
            &registrar,
            &voter,
            voter_authority,
            &mngo_rate,
            &voter_authority,
            reference_account,
            governance_registry::account::LockupKind::None,
            10000,
            0,
            false,
        )
        .await
        .unwrap();

    let after_deposit = get_balances(0).await;
    assert_eq!(initial.token, after_deposit.token + after_deposit.vault);
    assert_eq!(after_deposit.voter_weight, after_deposit.vault);
    assert_eq!(after_deposit.vault, 10000);
    assert_eq!(after_deposit.deposit, 10000);

    // add to the existing deposit 0
    update_deposit(5000).await.unwrap();

    let after_deposit2 = get_balances(0).await;
    assert_eq!(initial.token, after_deposit2.token + after_deposit2.vault);
    assert_eq!(after_deposit2.voter_weight, after_deposit2.vault);
    assert_eq!(after_deposit2.vault, 15000);
    assert_eq!(after_deposit2.deposit, 15000);

    // create a separate deposit (index 1)
    addin
        .create_deposit(
            &registrar,
            &voter,
            voter_authority,
            &mngo_rate,
            &voter_authority,
            reference_account,
            governance_registry::account::LockupKind::None,
            7000,
            0,
            false,
        )
        .await
        .unwrap();

    withdraw(10000)
        .await
        .expect_err("deposit happened in the same slot");

    let after_deposit3 = get_balances(1).await;
    assert_eq!(initial.token, after_deposit3.token + after_deposit3.vault);
    assert_eq!(after_deposit3.voter_weight, after_deposit3.vault);
    assert_eq!(after_deposit3.vault, 22000);
    assert_eq!(after_deposit3.deposit, 7000);

    // Withdraw works now because some slots were advanced (in get_balances())
    withdraw(10000).await.unwrap();

    let after_withdraw1 = get_balances(0).await;
    assert_eq!(initial.token, after_withdraw1.token + after_withdraw1.vault);
    assert_eq!(after_withdraw1.voter_weight, after_withdraw1.vault);
    assert_eq!(after_withdraw1.vault, 12000);
    assert_eq!(after_withdraw1.deposit, 5000);

    withdraw(5001).await.expect_err("withdrew too much");

    withdraw(5000).await.unwrap();

    let after_withdraw2 = get_balances(0).await;
    assert_eq!(initial.token, after_withdraw2.token + after_withdraw2.vault);
    assert_eq!(after_withdraw2.voter_weight, after_withdraw2.vault);
    assert_eq!(after_withdraw2.vault, 7000);
    assert_eq!(after_withdraw2.deposit, 0);

    // Close the empty deposit (closing deposits 1 and 2 fails)
    addin
        .close_deposit(&voter, &voter_authority, 2)
        .await
        .expect_err("deposit not in use");
    addin
        .close_deposit(&voter, &voter_authority, 1)
        .await
        .expect_err("deposit not empty");
    addin
        .close_deposit(&voter, &voter_authority, 0)
        .await
        .unwrap();

    let after_close = get_balances(0).await;
    assert_eq!(initial.token, after_close.token + after_close.vault);
    assert_eq!(after_close.voter_weight, after_close.vault);
    assert_eq!(after_close.vault, 7000);
    assert_eq!(after_close.deposit, 0);

    // check that the voter2 account is still at 0
    let voter2_balances = balances(
        &context,
        &registrar,
        reference_account,
        &voter2,
        &voter2_authority,
        &mngo_rate,
        0,
    )
    .await;
    assert_eq!(voter2_balances.deposit, 0);
    assert_eq!(voter2_balances.voter_weight, 0);

    // now voter2 deposits
    addin
        .create_deposit(
            &registrar,
            &voter2,
            voter2_authority,
            &mngo_rate,
            &voter2_authority,
            context.users[2].token_accounts[0],
            governance_registry::account::LockupKind::None,
            1000,
            5,
            false,
        )
        .await?;

    let voter2_balances = balances(
        &context,
        &registrar,
        reference_account,
        &voter2,
        &voter2_authority,
        &mngo_rate,
        0,
    )
    .await;
    assert_eq!(voter2_balances.deposit, 1000);
    assert_eq!(voter2_balances.voter_weight, 1000);
    assert_eq!(voter2_balances.vault, 8000);

    // when voter1 deposits again, they can reuse deposit index 0
    addin
        .create_deposit(
            &registrar,
            &voter,
            voter_authority,
            &mngo_rate,
            &voter_authority,
            reference_account,
            governance_registry::account::LockupKind::Monthly,
            3000,
            1,
            false,
        )
        .await
        .unwrap();

    let after_reuse = get_balances(0).await;
    assert_eq!(initial.token, after_reuse.token + 7000 + 3000);
    assert_eq!(after_reuse.voter_weight, 7000 + 3000);
    assert_eq!(after_reuse.vault, 8000 + 3000);
    assert_eq!(after_reuse.deposit, 3000);

    Ok(())
}
