use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;

    stop_mining();
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // don't sync wallet without online
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let transactions = test_list_transactions(&wallet, None);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    let rcv_transactions = test_list_transactions(&wallet, None);
    assert_eq!(transactions.len(), 2);
    assert_eq!(rcv_transactions.len(), 2);
    assert!(transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::User)));
    assert!(transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::CreateUtxos)));
    assert!(rcv_transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::User)));
    assert!(rcv_transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::CreateUtxos)));
    assert!(transactions.iter().any(|t| t.confirmation_time.is_none()));
    assert!(rcv_transactions
        .iter()
        .any(|t| t.confirmation_time.is_none()));
    // sync wallet when online is provided
    mine(true);
    let transactions = test_list_transactions(&wallet, Some(&online));
    let rcv_transactions = test_list_transactions(&rcv_wallet, Some(&rcv_online));
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
    assert!(rcv_transactions
        .iter()
        .all(|t| t.confirmation_time.is_some()));

    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let receive_data = test_witness_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
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
    test_refresh_all(&mut rcv_wallet, &rcv_online);
    wallet.refresh(online.clone(), None, vec![]).unwrap();
    mine(false);
    test_refresh_all(&mut rcv_wallet, &rcv_online);
    wallet.refresh(online.clone(), None, vec![]).unwrap();
    let transactions = test_list_transactions(&wallet, Some(&online));
    let rcv_transactions = test_list_transactions(&rcv_wallet, Some(&rcv_online));
    assert_eq!(transactions.len(), 3);
    assert_eq!(rcv_transactions.len(), 3);
    assert!(transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::RgbSend)));
    assert!(rcv_transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::RgbSend)));
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
    assert!(rcv_transactions
        .iter()
        .all(|t| t.confirmation_time.is_some()));

    drain_wallet(&wallet, &online);
    mine(false);
    let transactions = test_list_transactions(&wallet, Some(&online));
    assert_eq!(transactions.len(), 4);
    assert!(transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::Drain)));
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
}
