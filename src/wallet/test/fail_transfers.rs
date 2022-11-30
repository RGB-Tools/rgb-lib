use super::*;

#[test]
fn success() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

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

    // fail single transfer
    let blind_data = rcv_wallet.blind(None, None, None).unwrap();
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blind_data.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    rcv_wallet
        .fail_transfers(rcv_online.clone(), Some(blind_data.blinded_utxo), None)
        .unwrap();

    // fail all WaitingCounterparty transfers
    let blind_data_1 = rcv_wallet.blind(None, None, None).unwrap();
    let blind_data_2 = rcv_wallet.blind(None, None, None).unwrap();
    let blind_data_3 = rcv_wallet.blind(None, None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            blinded_utxo: blind_data_3.blinded_utxo.clone(),
            amount,
        }],
    )]);
    let txid = wallet.send(online, recipient_map, false).unwrap();
    assert!(!txid.is_empty());
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blind_data_1.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blind_data_2.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blind_data_3.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    rcv_wallet.fail_transfers(rcv_online, None, None).unwrap();
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blind_data_1.blinded_utxo,
        TransferStatus::Failed
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blind_data_2.blinded_utxo,
        TransferStatus::Failed
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blind_data_3.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
}

#[test]
fn batch_success() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, rcv_online_1) = get_funded_wallet!();
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

    // transfer is in WaitingCounterparty status and can be failed, using both blinded_utxo + txid
    let blind_data_1 = rcv_wallet_1.blind(None, None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![
            Recipient {
                blinded_utxo: blind_data_1.blinded_utxo.clone(),
                amount,
            },
            Recipient {
                blinded_utxo: blind_data_2.blinded_utxo.clone(),
                amount,
            },
        ],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_1,
        &blind_data_1.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_2,
        &blind_data_2.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));
    wallet
        .fail_transfers(online.clone(), Some(blind_data_1.blinded_utxo), Some(txid))
        .unwrap();

    // ...and can be failed using txid only
    let blind_data_1 = rcv_wallet_1.blind(None, None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None, None).unwrap();
    let recipient_map = HashMap::from([(
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
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());
    wallet
        .fail_transfers(online.clone(), None, Some(txid))
        .unwrap();

    // transfer is still in WaitingCounterparty status after some recipients (but not all) replied with an ACK
    let blind_data_1 = rcv_wallet_1.blind(None, None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_id,
        vec![
            Recipient {
                blinded_utxo: blind_data_1.blinded_utxo.clone(),
                amount,
            },
            Recipient {
                blinded_utxo: blind_data_2.blinded_utxo.clone(),
                amount,
            },
        ],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());
    rcv_wallet_1.refresh(rcv_online_1, None).unwrap();
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_1,
        &blind_data_1.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_2,
        &blind_data_2.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));
    wallet
        .fail_transfers(online, Some(blind_data_2.blinded_utxo), Some(txid))
        .unwrap();
}

#[test]
fn fail() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

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
    // blind
    let blind_data = rcv_wallet.blind(None, None, None).unwrap();
    let blinded_utxo = blind_data.blinded_utxo;
    // send
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blinded_utxo.clone(),
            amount: 66,
        }],
    )]);
    wallet.send(online.clone(), recipient_map, false).unwrap();

    // check starting transfer status
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));

    // don't fail unknown blinded UTXO
    let result = rcv_wallet.fail_transfers(rcv_online.clone(), Some(s!("txob1inexistent")), None);
    assert!(matches!(result, Err(Error::TransferNotFound(_))));

    // don't fail incoming transfer: waiting counterparty -> confirmations
    let result = rcv_wallet.fail_transfers(rcv_online.clone(), Some(blinded_utxo.clone()), None);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    // don't fail outgoing transfer: waiting counterparty -> confirmations
    let result = wallet.fail_transfers(online.clone(), Some(blinded_utxo.clone()), None);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));

    // don't fail incoming transfer: waiting confirmations
    let result = rcv_wallet.fail_transfers(rcv_online.clone(), Some(blinded_utxo.clone()), None);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    // don't fail outgoing transfer: waiting confirmations
    let result = wallet.fail_transfers(online.clone(), Some(blinded_utxo.clone()), None);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));

    // mine and refresh so transfers can settle
    mine();
    wallet
        .refresh(online.clone(), Some(asset_id.clone()))
        .unwrap();
    rcv_wallet
        .refresh(rcv_online.clone(), Some(asset_id))
        .unwrap();

    // don't fail incoming transfer: settled
    let result = rcv_wallet.fail_transfers(rcv_online, Some(blinded_utxo.clone()), None);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::Settled
    ));
    // don't fail outgoing transfer: settled
    let result = wallet.fail_transfers(online, Some(blinded_utxo.clone()), None);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blinded_utxo,
        TransferStatus::Settled
    ));
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
            vec![AMOUNT, AMOUNT * 2, AMOUNT * 3],
        )
        .unwrap();
    let asset_id = asset.asset_id;

    // only blinded utxo given but multiple transfers in batch
    let blind_data_1 = rcv_wallet_1.blind(None, None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None, None).unwrap();
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
    let result = wallet.fail_transfers(online.clone(), Some(blind_data_1.blinded_utxo), None);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    // successfully fail transfer so asset can be spent again
    let result = wallet.fail_transfers(online.clone(), None, Some(txid));
    assert!(result.is_ok());

    // blinded utxo + txid given but blinded utxo transfer not part of batch transfer
    let blind_data_1 = rcv_wallet_1.blind(None, None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None, None).unwrap();
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
    let blind_data_3 = rcv_wallet_2.blind(None, None, None).unwrap();
    let recipient_map_2 = HashMap::from([(
        asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blind_data_3.blinded_utxo.clone(),
            amount,
        }],
    )]);
    let txid_2 = wallet.send(online.clone(), recipient_map_2, false).unwrap();
    let result = wallet.fail_transfers(
        online.clone(),
        Some(blind_data_3.blinded_utxo),
        Some(txid_1.clone()),
    );
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    // successfully fail transfers so asset can be spent again
    wallet
        .fail_transfers(online.clone(), None, Some(txid_1))
        .unwrap();
    wallet
        .fail_transfers(online.clone(), None, Some(txid_2))
        .unwrap();

    // batch send as donation (doesn't wait for recipient confirmations)
    let blind_data_1 = rcv_wallet_1.blind(None, None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_id,
        vec![
            Recipient {
                blinded_utxo: blind_data_1.blinded_utxo,
                amount,
            },
            Recipient {
                blinded_utxo: blind_data_2.blinded_utxo.clone(),
                amount,
            },
        ],
    )]);
    wallet.send(online.clone(), recipient_map, true).unwrap();

    // transfer is in WaitingConfirmations status and cannot be failed
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blind_data_2.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    let result = wallet.fail_transfers(online, Some(blind_data_2.blinded_utxo), None);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
}
