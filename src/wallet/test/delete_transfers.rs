use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // delete single transfer
    let blind_data = wallet.blind(None, None).unwrap();
    wallet
        .fail_transfers(online.clone(), Some(blind_data.blinded_utxo.clone()), None)
        .unwrap();
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blind_data.blinded_utxo,
        TransferStatus::Failed
    ));
    wallet
        .delete_transfers(Some(blind_data.blinded_utxo), None)
        .unwrap();

    // delete all Failed transfers
    let blind_data_1 = wallet.blind(None, None).unwrap();
    let blind_data_2 = wallet.blind(None, None).unwrap();
    let blind_data_3 = wallet.blind(None, None).unwrap();
    wallet
        .fail_transfers(
            online.clone(),
            Some(blind_data_1.blinded_utxo.clone()),
            None,
        )
        .unwrap();
    wallet
        .fail_transfers(online, Some(blind_data_2.blinded_utxo.clone()), None)
        .unwrap();
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blind_data_1.blinded_utxo,
        TransferStatus::Failed
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blind_data_2.blinded_utxo,
        TransferStatus::Failed
    ));
    wallet.delete_transfers(None, None).unwrap();
    let transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(transfers.len(), 1);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blind_data_3.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
}

#[test]
fn batch_success() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_id = asset.asset_id;

    // failed transfer can be deleted, using both blinded_utxo + txid
    let blind_data_1 = rcv_wallet_1.blind(None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![
            Recipient {
                blinded_utxo: blind_data_1.blinded_utxo.clone(),
                amount,
            },
            Recipient {
                blinded_utxo: blind_data_2.blinded_utxo,
                amount,
            },
        ],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());
    wallet
        .fail_transfers(online.clone(), None, Some(txid.clone()))
        .unwrap();
    wallet
        .delete_transfers(Some(blind_data_1.blinded_utxo), Some(txid))
        .unwrap();

    // ...and can be deleted using txid only
    let blind_data_1 = rcv_wallet_1.blind(None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_id,
        vec![
            Recipient {
                blinded_utxo: blind_data_1.blinded_utxo,
                amount,
            },
            Recipient {
                blinded_utxo: blind_data_2.blinded_utxo,
                amount,
            },
        ],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());
    wallet
        .fail_transfers(online, None, Some(txid.clone()))
        .unwrap();
    wallet.delete_transfers(None, Some(txid)).unwrap();
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, _online) = get_funded_wallet!();

    let blind_data = wallet.blind(None, None).unwrap();

    // don't delete transfer not in Failed status
    assert!(!check_test_transfer_status_recipient(
        &wallet,
        &blind_data.blinded_utxo,
        TransferStatus::Failed
    ));
    let result = wallet.delete_transfers(Some(blind_data.blinded_utxo), None);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));

    // don't delete unknown blinded utxo
    let result = wallet.delete_transfers(Some(s!("txob1inexistent")), None);
    assert!(matches!(result, Err(Error::TransferNotFound(_))));
}

#[test]
fn batch_fail() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_id = asset.asset_id;

    // only blinded utxo given but multiple transfers in batch
    let blind_data_1 = rcv_wallet_1.blind(None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![
            Recipient {
                blinded_utxo: blind_data_1.blinded_utxo.clone(),
                amount,
            },
            Recipient {
                blinded_utxo: blind_data_2.blinded_utxo,
                amount,
            },
        ],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    wallet
        .fail_transfers(online.clone(), None, Some(txid.clone()))
        .unwrap();
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::Failed
    ));
    let result = wallet.delete_transfers(Some(blind_data_1.blinded_utxo), None);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));

    // blinded utxo + txid given but blinded utxo transfer not part of batch transfer
    let blind_data_1 = rcv_wallet_1.blind(None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None).unwrap();
    let recipient_map_1 = HashMap::from([(
        asset_id.clone(),
        vec![
            Recipient {
                blinded_utxo: blind_data_1.blinded_utxo,
                amount,
            },
            Recipient {
                blinded_utxo: blind_data_2.blinded_utxo,
                amount,
            },
        ],
    )]);
    let txid_1 = wallet.send(online.clone(), recipient_map_1, false).unwrap();
    wallet
        .fail_transfers(online.clone(), None, Some(txid_1.clone()))
        .unwrap();
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid_1,
        TransferStatus::Failed
    ));
    let blind_data_3 = rcv_wallet_2.blind(None, None).unwrap();
    let recipient_map_2 = HashMap::from([(
        asset_id,
        vec![Recipient {
            blinded_utxo: blind_data_3.blinded_utxo.clone(),
            amount,
        }],
    )]);
    let txid_2 = wallet.send(online.clone(), recipient_map_2, false).unwrap();
    wallet
        .fail_transfers(online, None, Some(txid_2.clone()))
        .unwrap();
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid_2,
        TransferStatus::Failed
    ));
    let result = wallet.delete_transfers(Some(blind_data_3.blinded_utxo), Some(txid_1));
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));
}
