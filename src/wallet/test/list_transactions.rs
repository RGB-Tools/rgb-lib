use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;

    let _guard = stop_mining_when_alone();
    let mut party = get_empty_party!();
    let mut rcv_party = get_empty_party!();

    send_to_address(party.get_address());
    send_to_address(rcv_party.get_address());
    force_mine_no_resume_when_alone(false);
    party.create_utxos_default();
    rcv_party.create_utxos_default();
    force_mine_no_resume_when_alone(false);

    // don't sync wallet without online
    let bak_info_before = party.db_backup_info();
    let transactions = party.list_transactions();
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    let rcv_transactions = party.list_transactions();
    assert_eq!(transactions.len(), 2);
    assert_eq!(rcv_transactions.len(), 2);
    assert!(
        transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::Incoming))
    );
    assert!(
        transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::CreateUtxos))
    );
    assert!(
        rcv_transactions
            .iter()
            .any(|t| matches!(t.transaction_type, TransactionType::Incoming))
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
    drop(_guard);
    let transactions = party.list_transactions_with_sync();
    let rcv_transactions = rcv_party.list_transactions_with_sync();
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
    assert!(
        rcv_transactions
            .iter()
            .all(|t| t.confirmation_time.is_some())
    );

    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.witness_receive();
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
    party.send_retry(&recipient_map);
    // settle the transfer so the tx gets broadcasted and receiver sees the new UTXO
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    let transactions = party.list_transactions_with_sync();
    let rcv_transactions = rcv_party.list_transactions_with_sync();
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

    party.drain_wallet();
    mine(false);
    let transactions = party.list_transactions_with_sync();
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

    let mut party = get_empty_party!();

    send_to_address(party.get_address());

    // transaction list doesn't report the TX if sync is skipped
    let transactions = party.list_transactions();
    assert_eq!(transactions.len(), 0);

    // transaction list reports the TX after manually syncing
    assert!(wait_for_function(
        || {
            party
                .wallet
                .sync(
                    party.online,
                    SyncOptions {
                        keychain: SyncKeychain::Vanilla {
                            lookback: INDEXER_SYNC_LOOKBACK as u32,
                        },
                        strategy: SyncStrategy::FastSync,
                    },
                )
                .unwrap();
            let transactions = party.list_transactions();
            transactions.len() == 1
        },
        check_timeout,
        check_interval,
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let mut offline_party = {
        let wallet = get_test_wallet(true, None);
        party!(wallet, Online { id: 0 })
    };
    let result = offline_party
        .wallet
        .list_transactions(Some(Online { id: 0 }), false);
    assert_matches!(result, Err(Error::Offline));
}
