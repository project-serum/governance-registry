use anchor_spl::token::TokenAccount;
use futures::FutureExt;
use program_test::*;
use solana_program_test::*;
use solana_sdk::{signature::Keypair, signer::Signer, transport::TransportError};

mod program_test;

#[allow(unaligned_references)]
#[tokio::test]
async fn test_reset_lockup() -> Result<(), TransportError> {
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

    let registrar = addin
        .create_registrar(&realm, &realm_authority, payer)
        .await;
    let mngo_rate = addin
        .create_exchange_rate(&registrar, &realm_authority, payer, 0, &context.mints[0], 1)
        .await;

    let voter = addin
        .create_voter(&registrar, &token_owner_record, &voter_authority, &payer)
        .await;

    let reference_account = context.users[1].token_accounts[0];
    let withdraw = |index: u8, amount: u64| {
        addin.withdraw(
            &registrar,
            &voter,
            &token_owner_record,
            &mngo_rate,
            &voter_authority,
            reference_account,
            index,
            amount,
        )
    };
    let deposit = |index: u8, amount: u64| {
        addin.deposit(
            &registrar,
            &voter,
            &mngo_rate,
            &voter_authority,
            reference_account,
            index,
            amount,
        )
    };
    let reset_lockup = |index: u8, periods: u32| {
        addin.reset_lockup(&registrar, &voter, &voter_authority, index, periods)
    };
    let lockup_status = |index: u8| {
        context
            .solana
            .get_account::<voter_stake_registry::state::Voter>(voter.address)
            .map(move |v| {
                let d = v.deposits[index as usize];
                (
                    d.lockup.end_ts - d.lockup.start_ts,
                    d.amount_initially_locked_native,
                    d.amount_deposited_native,
                )
            })
    };

    // tests for daily vesting
    addin
        .create_deposit_entry(
            &registrar,
            &voter,
            &voter_authority,
            &mngo_rate,
            7,
            voter_stake_registry::state::LockupKind::Daily,
            3,
            false,
        )
        .await
        .unwrap();
    deposit(7, 8000).await.unwrap();
    assert_eq!(lockup_status(7).await, (3 * 24 * 60 * 60, 8000, 8000));
    deposit(7, 1000).await.unwrap();
    assert_eq!(lockup_status(7).await, (3 * 24 * 60 * 60, 9000, 9000));
    reset_lockup(7, 2)
        .await
        .expect_err("can't relock for less periods");
    reset_lockup(7, 3).await.unwrap(); // just resets start to current timestamp
    assert_eq!(lockup_status(7).await, (3 * 24 * 60 * 60, 9000, 9000));

    // advance more than a day
    addin
        .set_time_offset(&registrar, &realm_authority, 25 * 60 * 60)
        .await;
    context.solana.advance_clock_by_slots(2).await;

    assert_eq!(lockup_status(7).await, (3 * 24 * 60 * 60, 9000, 9000));
    deposit(7, 1000).await.unwrap();
    assert_eq!(lockup_status(7).await, (2 * 24 * 60 * 60, 7000, 10000)); // 3000 vested
    reset_lockup(7, 10).await.unwrap();
    assert_eq!(lockup_status(7).await, (10 * 24 * 60 * 60, 10000, 10000));

    // advance four more days
    addin
        .set_time_offset(&registrar, &realm_authority, 5 * 25 * 60 * 60)
        .await;
    context.solana.advance_clock_by_slots(2).await;

    assert_eq!(lockup_status(7).await, (10 * 24 * 60 * 60, 10000, 10000));
    withdraw(7, 2000).await.unwrap(); // partially withdraw vested
    assert_eq!(lockup_status(7).await, (10 * 24 * 60 * 60, 10000, 8000));
    reset_lockup(7, 5)
        .await
        .expect_err("can't relock for less periods");
    reset_lockup(7, 6).await.unwrap();
    assert_eq!(lockup_status(7).await, (6 * 24 * 60 * 60, 8000, 8000));
    reset_lockup(7, 8).await.unwrap();
    assert_eq!(lockup_status(7).await, (8 * 24 * 60 * 60, 8000, 8000));

    // advance three more days
    addin
        .set_time_offset(&registrar, &realm_authority, 8 * 25 * 60 * 60)
        .await;
    context.solana.advance_clock_by_slots(2).await;

    assert_eq!(lockup_status(7).await, (8 * 24 * 60 * 60, 8000, 8000));
    deposit(7, 1000).await.unwrap();
    assert_eq!(lockup_status(7).await, (5 * 24 * 60 * 60, 6000, 9000)); // 3000 vested

    context.solana.advance_clock_by_slots(2).await; // avoid deposit and withdraw in one slot

    withdraw(7, 2000).await.unwrap(); // partially withdraw vested
    assert_eq!(lockup_status(7).await, (5 * 24 * 60 * 60, 6000, 7000));
    reset_lockup(7, 10).await.unwrap();
    assert_eq!(lockup_status(7).await, (10 * 24 * 60 * 60, 7000, 7000));

    // tests for cliff vesting
    addin.set_time_offset(&registrar, &realm_authority, 0).await;
    context.solana.advance_clock_by_slots(2).await;

    addin
        .create_deposit_entry(
            &registrar,
            &voter,
            &voter_authority,
            &mngo_rate,
            5,
            voter_stake_registry::state::LockupKind::Cliff,
            3,
            false,
        )
        .await
        .unwrap();
    deposit(5, 8000).await.unwrap();
    assert_eq!(lockup_status(5).await, (3 * 24 * 60 * 60, 8000, 8000));
    reset_lockup(5, 2)
        .await
        .expect_err("can't relock for less periods");
    reset_lockup(5, 3).await.unwrap(); // just resets start to current timestamp
    assert_eq!(lockup_status(5).await, (3 * 24 * 60 * 60, 8000, 8000));
    reset_lockup(5, 4).await.unwrap();
    assert_eq!(lockup_status(5).await, (4 * 24 * 60 * 60, 8000, 8000));

    // advance to end of cliff
    addin
        .set_time_offset(&registrar, &realm_authority, 4 * 25 * 60 * 60)
        .await;
    context.solana.advance_clock_by_slots(2).await;

    assert_eq!(lockup_status(5).await, (4 * 24 * 60 * 60, 8000, 8000));
    reset_lockup(5, 1).await.unwrap();
    assert_eq!(lockup_status(5).await, (1 * 24 * 60 * 60, 8000, 8000));
    withdraw(5, 1000).await.expect_err("nothing unlocked");

    // advance to end of cliff again
    addin
        .set_time_offset(&registrar, &realm_authority, 5 * 25 * 60 * 60)
        .await;
    context.solana.advance_clock_by_slots(2).await;

    withdraw(5, 1000).await.unwrap();
    assert_eq!(lockup_status(5).await, (1 * 24 * 60 * 60, 8000, 7000));
    deposit(5, 500).await.unwrap();
    assert_eq!(lockup_status(5).await, (0 * 24 * 60 * 60, 500, 7500));
    reset_lockup(5, 1).await.unwrap();
    assert_eq!(lockup_status(5).await, (1 * 24 * 60 * 60, 7500, 7500));
    deposit(5, 1500).await.unwrap();
    assert_eq!(lockup_status(5).await, (1 * 24 * 60 * 60, 9000, 9000));

    Ok(())
}
