use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // issue an RGB20 asset
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // balances after issuance
    let asset_balance = wallet.get_asset_balance(asset.asset_id).unwrap();
    assert_eq!(
        asset_balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT,
            spendable: AMOUNT,
        }
    );

    // issue an RGB25 asset
    let asset = wallet
        .issue_asset_cfa(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT],
            None,
        )
        .unwrap();

    // balances after issuance
    let asset_balance = wallet.get_asset_balance(asset.asset_id).unwrap();
    assert_eq!(
        asset_balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT,
            spendable: AMOUNT,
        }
    );
}

#[test]
fn transfer_balances() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    let (mut wallet_send, online_send) = get_funded_wallet!();
    let (mut wallet_recv, online_recv) = get_funded_wallet!();

    // issue
    let asset = wallet_send
        .issue_asset_nia(
            online_send.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT, AMOUNT, AMOUNT],
        )
        .unwrap();

    show_unspent_colorings(&wallet_send, "send after issuance");
    show_unspent_colorings(&wallet_recv, "recv after issuance");

    // balances after issuance
    let asset_balance_send = wallet_send
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3,
            spendable: AMOUNT * 3,
        }
    );
    // receiver side after issuance (no asset yet)
    let result = wallet_recv.get_asset_balance(asset.asset_id.clone());
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    //
    // 1st transfer
    //

    // blind + fail to check failed blinds are not counted in balance
    let receive_data_fail = wallet_recv
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    wallet_recv
        .fail_transfers(
            online_recv.clone(),
            Some(receive_data_fail.recipient_id.clone()),
            None,
            false,
        )
        .unwrap();
    // send + fail to check failed inputs + changes are not counted in balance
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_fail.recipient_id).unwrap(),
            ),
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet_send, &online_send, recipient_map);
    wallet_send
        .fail_transfers(online_send.clone(), None, Some(txid), false)
        .unwrap();
    // send some assets
    let receive_data_1 = wallet_recv
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // actual send
    test_send_default(&mut wallet_send, &online_send, recipient_map);

    show_unspent_colorings(&wallet_send, "send after 1st send");
    show_unspent_colorings(&wallet_recv, "recv after 1st send");

    // sender balance with transfer WaitingCounterparty
    let transfers = wallet_send.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(transfers.len(), 3);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );
    let asset_balance_send = wallet_send
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3 - amount_1,
            spendable: AMOUNT * 2,
        }
    );

    stop_mining();

    // take transfers from WaitingCounterparty to WaitingConfirmations
    wallet_recv
        .refresh(online_recv.clone(), None, vec![])
        .unwrap();
    wallet_send
        .refresh(online_send.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // balances with transfer WaitingConfirmations
    let transfers = wallet_send.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let asset_balance_send = wallet_send
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3 - amount_1,
            spendable: AMOUNT * 2,
        }
    );
    let transfers_recv = wallet_recv.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let asset_balance_recv = wallet_recv
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_recv,
        Balance {
            settled: 0,
            future: amount_1,
            spendable: 0,
        }
    );

    // take transfers from WaitingConfirmations to Settled
    mine(true);
    wallet_recv
        .refresh(online_recv.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    wallet_send
        .refresh(online_send.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    show_unspent_colorings(&wallet_send, "send after 1st send, settled");
    show_unspent_colorings(&wallet_recv, "recv after 1st send, settled");

    // balances with transfer Settled
    let transfers = wallet_send.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let asset_balance_send = wallet_send
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3 - amount_1,
            future: AMOUNT * 3 - amount_1,
            spendable: AMOUNT * 3 - amount_1,
        }
    );
    let transfers_recv = wallet_recv.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::Settled
    );
    let asset_balance_recv = wallet_recv
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_recv,
        Balance {
            settled: amount_1,
            future: amount_1,
            spendable: amount_1,
        }
    );

    //
    // a 2nd transfer
    //

    // send some assets
    let receive_data_2 = wallet_recv
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
            ),
            amount: amount_2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send_default(&mut wallet_send, &online_send, recipient_map);

    show_unspent_colorings(&wallet_send, "send after 2nd send");
    show_unspent_colorings(&wallet_recv, "recv after 2nd send");

    // sender balance with transfer WaitingCounterparty
    let transfers = wallet_send.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(transfers.len(), 4);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );
    let asset_balance_send = wallet_send
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3 - amount_1,
            future: AMOUNT * 3 - amount_1 - amount_2,
            spendable: AMOUNT * 2 - amount_1,
        }
    );

    stop_mining();

    // take transfers from WaitingCounterparty to WaitingConfirmations
    wallet_recv
        .refresh(online_recv.clone(), None, vec![])
        .unwrap();
    wallet_send
        .refresh(online_send.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // balances with transfer WaitingConfirmations
    let transfers = wallet_send.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let asset_balance_send = wallet_send
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3 - amount_1,
            future: AMOUNT * 3 - amount_1 - amount_2,
            spendable: AMOUNT * 2 - amount_1,
        }
    );
    let asset_balance_recv = wallet_recv
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_recv,
        Balance {
            settled: amount_1,
            future: amount_1 + amount_2,
            spendable: amount_1,
        }
    );

    // take transfers from WaitingConfirmations to Settled
    mine(true);
    wallet_recv
        .refresh(online_recv, Some(asset.asset_id.clone()), vec![])
        .unwrap();
    wallet_send
        .refresh(online_send, Some(asset.asset_id.clone()), vec![])
        .unwrap();

    show_unspent_colorings(&wallet_send, "send after 2nd send, settled");
    show_unspent_colorings(&wallet_recv, "recv after 2nd send, settled");

    // balances with transfer Settled
    let transfers = wallet_send.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let asset_balance_send = wallet_send
        .get_asset_balance(asset.asset_id.clone())
        .unwrap();
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3 - amount_1 - amount_2,
            future: AMOUNT * 3 - amount_1 - amount_2,
            spendable: AMOUNT * 3 - amount_1 - amount_2,
        }
    );
    let asset_balance_recv = wallet_recv.get_asset_balance(asset.asset_id).unwrap();
    assert_eq!(
        asset_balance_recv,
        Balance {
            settled: amount_1 + amount_2,
            future: amount_1 + amount_2,
            spendable: amount_1 + amount_2,
        }
    );
}

#[test]
fn fail() {
    let (wallet, _online) = get_empty_wallet!();

    // bad asset_id returns an error
    let result = wallet.get_asset_balance("rgb1inexistent".to_string());
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
