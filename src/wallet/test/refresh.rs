use super::*;

#[test]
fn success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    let filter_counter_in = RefreshFilter {
        status: RefreshTransferStatus::WaitingCounterparty,
        incoming: true,
    };
    let filter_counter_out = RefreshFilter {
        status: RefreshTransferStatus::WaitingCounterparty,
        incoming: false,
    };
    let filter_confirm_in = RefreshFilter {
        status: RefreshTransferStatus::WaitingConfirmations,
        incoming: true,
    };
    let filter_confirm_out = RefreshFilter {
        status: RefreshTransferStatus::WaitingConfirmations,
        incoming: false,
    };

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();

    // issue
    let asset_1 = wallet_1
        .issue_asset_rgb20(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
        )
        .unwrap();
    let asset_2 = wallet_2
        .issue_asset_rgb20(
            online_2.clone(),
            s!("TICKER2"),
            s!("NAME2"),
            PRECISION,
            vec![AMOUNT * 2, AMOUNT * 2],
        )
        .unwrap();

    // per each wallet prepare:
    // - 1 WaitingCounterparty + 1 WaitingConfirmations ountgoing
    // - 1 WaitingCounterparty + 1 WaitingConfirmations incoming

    stop_mining();

    // wallet 1 > wallet 2 WaitingConfirmations and vice versa
    let blind_data_2a = wallet_2.blind(None, None, None).unwrap();
    let recipient_map_1a = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            blinded_utxo: blind_data_2a.blinded_utxo.clone(),
        }],
    )]);
    let txid_1a = wallet_1
        .send(online_1.clone(), recipient_map_1a, false)
        .unwrap();
    assert!(!txid_1a.is_empty());
    let blind_data_1a = wallet_1.blind(None, None, None).unwrap();
    let recipient_map_2a = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            blinded_utxo: blind_data_1a.blinded_utxo.clone(),
        }],
    )]);
    let txid_2a = wallet_2
        .send(online_2.clone(), recipient_map_2a, false)
        .unwrap();
    assert!(!txid_2a.is_empty());
    wallet_1.refresh(online_1.clone(), None, vec![]).unwrap();
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1
        .refresh(
            online_1.clone(),
            Some(asset_1.asset_id.clone()),
            vec![filter_counter_out.clone()],
        )
        .unwrap();
    // wallet 1 > 2, WaitingCounterparty and vice versa
    let blind_data_2b = wallet_2.blind(None, None, None).unwrap();
    let recipient_map_1b = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            blinded_utxo: blind_data_2b.blinded_utxo.clone(),
        }],
    )]);
    let txid_1b = wallet_1
        .send(online_1.clone(), recipient_map_1b, false)
        .unwrap();
    assert!(!txid_1b.is_empty());
    // wallet 2 > 1, WaitingCounterparty
    let blind_data_1b = wallet_1.blind(None, None, None).unwrap();
    show_unspent_colorings(&wallet_1, "wallet 1 after blind 1b");
    let recipient_map_2b = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            blinded_utxo: blind_data_1b.blinded_utxo.clone(),
        }],
    )]);
    let txid_2b = wallet_2
        .send(online_2.clone(), recipient_map_2b, false)
        .unwrap();
    assert!(!txid_2b.is_empty());
    show_unspent_colorings(&wallet_2, "wallet 2 after send 2b");
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &blind_data_1a.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &blind_data_1b.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &blind_data_2a.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &blind_data_2b.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));

    // refresh incoming WaitingCounterparty only (wallet 1)
    wallet_1
        .refresh(online_1.clone(), None, vec![filter_counter_in])
        .unwrap();
    show_unspent_colorings(
        &wallet_1,
        "wallet 1 after refresh incoming WaitingCounterparty",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &blind_data_1a.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &blind_data_1b.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));

    // refresh outgoing WaitingCounterparty only (wallet 2)
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingCounterparty
    ));
    wallet_2
        .refresh(online_2.clone(), None, vec![filter_counter_out])
        .unwrap();
    show_unspent_colorings(
        &wallet_2,
        "wallet 2 after refresh outgoing WaitingCounterparty",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &blind_data_2a.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &blind_data_2b.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));

    mine(true);

    // refresh incoming WaitingConfirmations only (wallet 2)
    wallet_2
        .refresh(online_2.clone(), None, vec![filter_confirm_in])
        .unwrap();
    show_unspent_colorings(
        &wallet_2,
        "wallet 2 after refresh incoming WaitingConfirmations",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &blind_data_2a.blinded_utxo,
        TransferStatus::Settled
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &blind_data_2b.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));

    // refresh outgoing WaitingConfirmations only (wallet 1)
    wallet_1
        .refresh(online_1.clone(), None, vec![filter_confirm_out])
        .unwrap();
    show_unspent_colorings(
        &wallet_1,
        "wallet 1 after refresh outgoing WaitingConfirmations",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::Settled
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &blind_data_1a.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &blind_data_1b.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // asset not found
    let result = wallet.refresh(online, Some(s!("rgb1inexistent")), vec![]);
    assert!(matches!(result, Err(Error::AssetNotFound(_))));
}
