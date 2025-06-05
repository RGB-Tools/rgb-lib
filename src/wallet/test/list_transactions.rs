use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;

    stop_mining_when_alone();
    let (mut wallet, online) = get_empty_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    send_to_address(test_get_address(&mut wallet));
    send_to_address(test_get_address(&mut rcv_wallet));
    force_mine_no_resume_when_alone(false);
    test_create_utxos_default(&mut wallet, &online);
    test_create_utxos_default(&mut rcv_wallet, &rcv_online);
    force_mine_no_resume_when_alone(false);

    // don't sync wallet without online
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let transactions = test_list_transactions(&mut wallet, None);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    let rcv_transactions = test_list_transactions(&mut wallet, None);
    assert_eq!(transactions.len(), 2);
    assert_eq!(rcv_transactions.len(), 2);
    assert!(
        transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::User))
    );
    assert!(
        transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::CreateUtxos))
    );
    assert!(
        rcv_transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::User))
    );
    assert!(
        rcv_transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::CreateUtxos))
    );
    assert!(transactions.iter().any(|t| t.confirmation_time.is_none()));
    assert!(
        rcv_transactions
            .iter()
            .any(|t| t.confirmation_time.is_none())
    );
    // sync wallet when online is provided
    resume_mining();
    let transactions = test_list_transactions(&mut wallet, Some(&online));
    let rcv_transactions = test_list_transactions(&mut rcv_wallet, Some(&rcv_online));
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
    assert!(
        rcv_transactions
            .iter()
            .all(|t| t.confirmation_time.is_some())
    );

    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let receive_data = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send(&mut wallet, &online, &recipient_map);
    // settle the transfer so the tx gets broadcasted and receiver sees the new UTXO
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);
    let transactions = test_list_transactions(&mut wallet, Some(&online));
    let rcv_transactions = test_list_transactions(&mut rcv_wallet, Some(&rcv_online));
    assert_eq!(transactions.len(), 3);
    assert_eq!(rcv_transactions.len(), 3);
    assert!(
        transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::RgbSend))
    );
    assert!(
        rcv_transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::RgbSend))
    );
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
    assert!(
        rcv_transactions
            .iter()
            .all(|t| t.confirmation_time.is_some())
    );

    drain_wallet(&mut wallet, &online);
    mine(false, false);
    let transactions = test_list_transactions(&mut wallet, Some(&online));
    assert_eq!(transactions.len(), 4);
    assert!(
        transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::Drain))
    );
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let check_timeout = 10;
    let check_interval = 1000;

    let (mut wallet, online) = get_empty_wallet!();

    send_to_address(test_get_address(&mut wallet));

    // transaction list doesn't report the TX if sync is skipped
    let transactions = test_list_transactions(&mut wallet, None);
    assert_eq!(transactions.len(), 0);

    // transaction list reports the TX after manually syncing
    assert!(wait_for_function(
        || {
            wallet.sync(online.clone()).unwrap();
            let transactions = test_list_transactions(&mut wallet, None);
            transactions.len() == 1
        },
        check_timeout,
        check_interval,
    ));
}
