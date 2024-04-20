use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (wallet, online) = get_funded_wallet!();

    // issue an NIA asset
    let asset = test_issue_asset_nia(&wallet, &online, None);

    // balances after issuance
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let asset_balance = test_get_asset_balance(&wallet, &asset.asset_id);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    assert_eq!(
        asset_balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT,
            spendable: AMOUNT,
        }
    );

    // issue an CFA asset
    let asset = test_issue_asset_cfa(&wallet, &online, None, None);

    // balances after issuance
    let asset_balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        asset_balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT,
            spendable: AMOUNT,
        }
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn transfer_balances() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    let (wallet_send, online_send) = get_funded_wallet!();
    let (wallet_recv, online_recv) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&wallet_send, &online_send, Some(&[AMOUNT, AMOUNT, AMOUNT]));

    show_unspent_colorings(&wallet_send, "send after issuance");
    show_unspent_colorings(&wallet_recv, "recv after issuance");

    // balances after issuance
    let asset_balance_send = test_get_asset_balance(&wallet_send, &asset.asset_id);
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3,
            spendable: AMOUNT * 3,
        }
    );
    // receiver side after issuance (no asset yet)
    let result = test_get_asset_balance_result(&wallet_recv, &asset.asset_id);
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    //
    // 1st transfer
    //

    // blind + fail to check failed blinds are not counted in balance
    let receive_data_fail = test_blind_receive(&wallet_recv);
    test_fail_transfers_single(
        &wallet_recv,
        &online_recv,
        receive_data_fail.batch_transfer_idx,
    );
    // send + fail to check failed inputs + changes are not counted in balance
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_fail.recipient_id.clone(),
            witness_data: None,
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let batch_transfer_idx = test_send_result(&wallet_send, &online_send, &recipient_map)
        .unwrap()
        .batch_transfer_idx;
    test_fail_transfers_single(&wallet_send, &online_send, batch_transfer_idx);
    // send some assets
    let receive_data_1 = test_blind_receive(&wallet_recv);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // actual send
    test_send(&wallet_send, &online_send, &recipient_map);

    show_unspent_colorings(&wallet_send, "send after 1st send");
    show_unspent_colorings(&wallet_recv, "recv after 1st send");

    // sender balance with transfer WaitingCounterparty
    let transfers = test_list_transfers(&wallet_send, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 3);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );
    let asset_balance_send = test_get_asset_balance(&wallet_send, &asset.asset_id);
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
    test_refresh_all(&wallet_recv, &online_recv);
    test_refresh_asset(&wallet_send, &online_send, &asset.asset_id);

    // balances with transfer WaitingConfirmations
    let transfers = test_list_transfers(&wallet_send, Some(&asset.asset_id));
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let asset_balance_send = test_get_asset_balance(&wallet_send, &asset.asset_id);
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3 - amount_1,
            spendable: AMOUNT * 2,
        }
    );
    let transfers_recv = test_list_transfers(&wallet_recv, Some(&asset.asset_id));
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let asset_balance_recv = test_get_asset_balance(&wallet_recv, &asset.asset_id);
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
    test_refresh_asset(&wallet_recv, &online_recv, &asset.asset_id);
    test_refresh_asset(&wallet_send, &online_send, &asset.asset_id);

    show_unspent_colorings(&wallet_send, "send after 1st send, settled");
    show_unspent_colorings(&wallet_recv, "recv after 1st send, settled");

    // balances with transfer Settled
    let transfers = test_list_transfers(&wallet_send, Some(&asset.asset_id));
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let asset_balance_send = test_get_asset_balance(&wallet_send, &asset.asset_id);
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3 - amount_1,
            future: AMOUNT * 3 - amount_1,
            spendable: AMOUNT * 3 - amount_1,
        }
    );
    let transfers_recv = test_list_transfers(&wallet_recv, Some(&asset.asset_id));
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::Settled
    );
    let asset_balance_recv = test_get_asset_balance(&wallet_recv, &asset.asset_id);
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
    let receive_data_2 = test_blind_receive(&wallet_recv);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            amount: amount_2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send(&wallet_send, &online_send, &recipient_map);

    show_unspent_colorings(&wallet_send, "send after 2nd send");
    show_unspent_colorings(&wallet_recv, "recv after 2nd send");

    // sender balance with transfer WaitingCounterparty
    let transfers = test_list_transfers(&wallet_send, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 4);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );
    let asset_balance_send = test_get_asset_balance(&wallet_send, &asset.asset_id);
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3 - amount_1,
            future: AMOUNT * 3 - amount_1 - amount_2,
            spendable: AMOUNT * 2,
        }
    );

    stop_mining();

    // take transfers from WaitingCounterparty to WaitingConfirmations
    test_refresh_all(&wallet_recv, &online_recv);
    test_refresh_asset(&wallet_send, &online_send, &asset.asset_id);

    // balances with transfer WaitingConfirmations
    let transfers = test_list_transfers(&wallet_send, Some(&asset.asset_id));
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let asset_balance_send = test_get_asset_balance(&wallet_send, &asset.asset_id);
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3 - amount_1,
            future: AMOUNT * 3 - amount_1 - amount_2,
            spendable: AMOUNT * 2,
        }
    );
    let asset_balance_recv = test_get_asset_balance(&wallet_recv, &asset.asset_id);
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
    test_refresh_asset(&wallet_recv, &online_recv, &asset.asset_id);
    test_refresh_asset(&wallet_send, &online_send, &asset.asset_id);

    show_unspent_colorings(&wallet_send, "send after 2nd send, settled");
    show_unspent_colorings(&wallet_recv, "recv after 2nd send, settled");

    // balances with transfer Settled
    let transfers = test_list_transfers(&wallet_send, Some(&asset.asset_id));
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let asset_balance_send = test_get_asset_balance(&wallet_send, &asset.asset_id);
    assert_eq!(
        asset_balance_send,
        Balance {
            settled: AMOUNT * 3 - amount_1 - amount_2,
            future: AMOUNT * 3 - amount_1 - amount_2,
            spendable: AMOUNT * 3 - amount_1 - amount_2,
        }
    );
    let asset_balance_recv = test_get_asset_balance(&wallet_recv, &asset.asset_id);
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
#[parallel]
fn fail() {
    let wallet = get_test_wallet(true, None);

    // bad asset_id returns an error
    let result = test_get_asset_balance_result(&wallet, "rgb1inexistent");
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
