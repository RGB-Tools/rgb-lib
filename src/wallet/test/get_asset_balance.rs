use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // issue an NIA asset
    let asset = test_issue_asset_nia(&mut wallet, online, None);

    // balances after issuance
    let bak_info_before = wallet.database().get_backup_info().unwrap().unwrap();
    let asset_balance = test_get_asset_balance(&wallet, &asset.asset_id);
    let bak_info_after = wallet.database().get_backup_info().unwrap().unwrap();
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
    let asset = test_issue_asset_cfa(&mut wallet, online, None, None);

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
    let amount_3: u64 = 11;

    // sender wallet with 3 UTXOs (both assets issued to the same UTXOs)
    let (mut wallet_send, online_send) = get_funded_noutxo_wallet!();
    test_create_utxos(
        &mut wallet_send,
        online_send,
        true,
        Some(3),
        None,
        FEE_RATE,
        None,
    );
    // recipient wallet with a single UTXO
    let (mut wallet_recv, online_recv) = get_funded_noutxo_wallet!();
    test_create_utxos(
        &mut wallet_recv,
        online_recv,
        true,
        Some(1),
        None,
        FEE_RATE,
        None,
    );

    // issue
    let asset_1 = test_issue_asset_nia(
        &mut wallet_send,
        online_send,
        Some(&[AMOUNT, AMOUNT, AMOUNT]),
    );
    let asset_2 = test_issue_asset_cfa(
        &mut wallet_send,
        online_send,
        Some(&[AMOUNT, AMOUNT, AMOUNT]),
        None,
    );

    // create 2 more UTXOs on the sender wallet
    test_create_utxos(
        &mut wallet_send,
        online_send,
        false,
        Some(2),
        None,
        FEE_RATE,
        None,
    );

    // balances after issuance
    show_unspent_colorings(&mut wallet_send, "send after issuance");
    show_unspent_colorings(&mut wallet_recv, "recv after issuance");
    let expected_balance_1 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3,
        spendable: AMOUNT * 3,
    };
    let expected_balance_2 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3,
        spendable: AMOUNT * 3,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance_1);
    wait_for_asset_balance(&wallet_send, &asset_2.asset_id, &expected_balance_2);
    // receiver side after issuance (no asset yet)
    let result_1 = test_get_asset_balance_result(&wallet_recv, &asset_1.asset_id);
    let result_2 = test_get_asset_balance_result(&wallet_recv, &asset_2.asset_id);
    assert!(matches!(
        result_1,
        Err(Error::AssetNotFound { asset_id: _ })
    ));
    assert!(matches!(
        result_2,
        Err(Error::AssetNotFound { asset_id: _ })
    ));

    //
    // blind + fail to check failed blinds are not counted in balance
    //

    let receive_data_fail = test_blind_receive(&mut wallet_recv);
    test_fail_transfers_single(
        &mut wallet_recv,
        online_recv,
        receive_data_fail.batch_transfer_idx,
    );

    //
    // send + fail to check failed inputs + changes are not counted in balance
    //

    // send
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_fail.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount_1),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let batch_transfer_idx = test_send_result(&mut wallet_send, online_send, &recipient_map)
        .unwrap()
        .batch_transfer_idx;
    // sender balances after send / before fail
    show_unspent_colorings(&mut wallet_send, "send after send / before fail");
    let expected_balance_1 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3 - amount_1,
        spendable: AMOUNT * 2,
    };
    let expected_balance_2 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3,
        spendable: AMOUNT * 2,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance_1);
    wait_for_asset_balance(&wallet_send, &asset_2.asset_id, &expected_balance_2);
    // fail the transfer
    test_fail_transfers_single(&mut wallet_send, online_send, batch_transfer_idx);
    // sender balances after fail
    show_unspent_colorings(&mut wallet_send, "send after fail");
    let expected_balance_1 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3,
        spendable: AMOUNT * 3,
    };
    let expected_balance_2 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3,
        spendable: AMOUNT * 3,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance_1);
    wait_for_asset_balance(&wallet_send, &asset_2.asset_id, &expected_balance_2);

    //
    // a 1st transfer (blinded)
    //

    // send
    let receive_data_1 = test_blind_receive(&mut wallet_recv);
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount_1),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send(&mut wallet_send, online_send, &recipient_map);
    // sender balance with transfer WaitingCounterparty (recipient doesn't know the asset yet)
    show_unspent_colorings(&mut wallet_send, "send after 1st send");
    show_unspent_colorings(&mut wallet_recv, "recv after 1st send");
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(transfers.len(), 3);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );
    let expected_balance_1 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3 - amount_1,
        spendable: AMOUNT * 2,
    };
    let expected_balance_2 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3,
        spendable: AMOUNT * 2,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance_1);
    wait_for_asset_balance(&wallet_send, &asset_2.asset_id, &expected_balance_2);
    let transfers_recv = test_list_transfers(&wallet_recv, None);
    assert_eq!(transfers_recv.len(), 2);
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );

    // asset_2 (extra) allocation should be recognized as unspendable
    let receive_data_fail = test_blind_receive(&mut wallet_recv);
    let recipient_map_fail = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_fail.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(AMOUNT * 3),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet_send, online_send, &recipient_map_fail);
    let assignments_collection = AssignmentsCollection {
        fungible: 1332,
        non_fungible: false,
        inflation: 0,
    };
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id, available }) if asset_id == asset_2.asset_id && available == assignments_collection );
    // fail + delete blind receive
    test_fail_transfers_single(
        &mut wallet_recv,
        online_recv,
        receive_data_fail.batch_transfer_idx,
    );
    test_delete_transfers(
        &wallet_recv,
        Some(receive_data_fail.batch_transfer_idx),
        false,
    );

    // take transfers from WaitingCounterparty to WaitingConfirmations
    wait_for_refresh(&mut wallet_recv, online_recv, None, None);
    wait_for_refresh(&mut wallet_send, online_send, Some(&asset_1.asset_id), None);
    // balances with transfer WaitingConfirmations
    show_unspent_colorings(
        &mut wallet_send,
        "send after 1st send, WaitingConfirmations",
    );
    show_unspent_colorings(
        &mut wallet_recv,
        "recv after 1st send, WaitingConfirmations",
    );
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let expected_balance_1 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3 - amount_1,
        spendable: AMOUNT * 2,
    };
    let expected_balance_2 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3,
        spendable: AMOUNT * 2,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance_1);
    wait_for_asset_balance(&wallet_send, &asset_2.asset_id, &expected_balance_2);
    let transfers_recv = test_list_transfers(&wallet_recv, Some(&asset_1.asset_id));
    assert_eq!(transfers_recv.len(), 1);
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let expected_balance_1 = Balance {
        settled: 0,
        future: amount_1,
        spendable: 0,
    };
    wait_for_asset_balance(&wallet_recv, &asset_1.asset_id, &expected_balance_1);

    // take transfers from WaitingConfirmations to Settled
    mine(false, false);
    wait_for_refresh(&mut wallet_recv, online_recv, Some(&asset_1.asset_id), None);
    wait_for_refresh(&mut wallet_send, online_send, Some(&asset_1.asset_id), None);
    // balances with transfer Settled
    show_unspent_colorings(&mut wallet_send, "send after 1st send, settled");
    show_unspent_colorings(&mut wallet_recv, "recv after 1st send, settled");
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let expected_balance_1 = Balance {
        settled: AMOUNT * 3 - amount_1,
        future: AMOUNT * 3 - amount_1,
        spendable: AMOUNT * 3 - amount_1,
    };
    let expected_balance_2 = Balance {
        settled: AMOUNT * 3,
        future: AMOUNT * 3,
        spendable: AMOUNT * 3,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance_1);
    wait_for_asset_balance(&wallet_send, &asset_2.asset_id, &expected_balance_2);
    let transfers_recv = test_list_transfers(&wallet_recv, Some(&asset_1.asset_id));
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::Settled
    );
    let expected_balance_1 = Balance {
        settled: amount_1,
        future: amount_1,
        spendable: amount_1,
    };
    wait_for_asset_balance(&wallet_recv, &asset_1.asset_id, &expected_balance_1);

    //
    // a 2nd transfer (blinded)
    //

    // send some assets
    let receive_data_2 = test_blind_receive(&mut wallet_recv);
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount_2),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send(&mut wallet_send, online_send, &recipient_map);

    show_unspent_colorings(&mut wallet_send, "send after 2nd send");
    show_unspent_colorings(&mut wallet_recv, "recv after 2nd send");

    // balances with transfer WaitingCounterparty
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(transfers.len(), 4);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1,
        future: AMOUNT * 3 - amount_1 - amount_2,
        spendable: AMOUNT * 2,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance);
    let transfers = test_list_transfers(&wallet_recv, None);
    assert_eq!(transfers.len(), 2);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );
    let expected_balance = Balance {
        settled: amount_1,
        future: amount_1,
        spendable: 0,
    };
    wait_for_asset_balance(&wallet_recv, &asset_1.asset_id, &expected_balance);

    // take transfers from WaitingCounterparty to WaitingConfirmations
    wait_for_refresh(&mut wallet_recv, online_recv, None, None);
    wait_for_refresh(&mut wallet_send, online_send, Some(&asset_1.asset_id), None);

    // balances with transfer WaitingConfirmations
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1,
        future: AMOUNT * 3 - amount_1 - amount_2,
        spendable: AMOUNT * 2,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance);
    let transfers_recv = test_list_transfers(&wallet_recv, Some(&asset_1.asset_id));
    assert_eq!(transfers_recv.len(), 2);
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let expected_balance = Balance {
        settled: amount_1,
        future: amount_1 + amount_2,
        spendable: 0,
    };
    wait_for_asset_balance(&wallet_recv, &asset_1.asset_id, &expected_balance);

    // take transfers from WaitingConfirmations to Settled
    mine(false, false);
    wait_for_refresh(&mut wallet_recv, online_recv, Some(&asset_1.asset_id), None);
    wait_for_refresh(&mut wallet_send, online_send, Some(&asset_1.asset_id), None);

    show_unspent_colorings(&mut wallet_send, "send after 2nd send, settled");
    show_unspent_colorings(&mut wallet_recv, "recv after 2nd send, settled");

    // balances with transfer Settled
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1 - amount_2,
        future: AMOUNT * 3 - amount_1 - amount_2,
        spendable: AMOUNT * 3 - amount_1 - amount_2,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance);
    let transfers_recv = test_list_transfers(&wallet_recv, Some(&asset_1.asset_id));
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::Settled
    );
    let expected_balance = Balance {
        settled: amount_1 + amount_2,
        future: amount_1 + amount_2,
        spendable: amount_1 + amount_2,
    };
    wait_for_asset_balance(&wallet_recv, &asset_1.asset_id, &expected_balance);

    //
    // a 3rd transfer (witness, donation)
    //

    // send some assets, donation = true to broadcast right away
    let receive_data_3 = test_witness_receive(&mut wallet_recv);
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            assignment: Assignment::Fungible(amount_3),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    wallet_send
        .send(
            online_send,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
            false,
        )
        .unwrap();

    // sync the wallets
    wallet_send.sync(online_send).unwrap();
    wallet_recv.sync(online_recv).unwrap();

    show_unspent_colorings(&mut wallet_send, "send after 3rd send");
    show_unspent_colorings(&mut wallet_recv, "recv after 3rd send");

    // balances after sync but before refresh
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(transfers.len(), 5);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations // due to donation = true
    );
    let transfers = test_list_transfers(&wallet_recv, None);
    assert_eq!(transfers.len(), 2);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1 - amount_2,
        future: AMOUNT * 3 - amount_1 - amount_2 - amount_3,
        spendable: AMOUNT * 2,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance);
    let expected_balance = Balance {
        settled: amount_1 + amount_2,
        future: amount_1 + amount_2,
        spendable: amount_1 + amount_2,
    };
    wait_for_asset_balance(&wallet_recv, &asset_1.asset_id, &expected_balance);

    // take recipient transfer from WaitingCounterparty to WaitingConfirmations
    wait_for_refresh(&mut wallet_recv, online_recv, None, None);

    // balances with transfer WaitingConfirmations
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let transfers = test_list_transfers(&wallet_recv, Some(&asset_1.asset_id));
    assert_eq!(transfers.len(), 3);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1 - amount_2,
        future: AMOUNT * 3 - amount_1 - amount_2 - amount_3,
        spendable: AMOUNT * 2,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance);
    let expected_balance = Balance {
        settled: amount_1 + amount_2,
        future: amount_1 + amount_2 + amount_3,
        spendable: amount_1 + amount_2,
    };
    wait_for_asset_balance(&wallet_recv, &asset_1.asset_id, &expected_balance);

    // take transfers from WaitingConfirmations to Settled
    mine(false, false);
    wait_for_refresh(&mut wallet_recv, online_recv, Some(&asset_1.asset_id), None);
    wait_for_refresh(&mut wallet_send, online_send, Some(&asset_1.asset_id), None);

    show_unspent_colorings(&mut wallet_send, "send after 3rd send, settled");
    show_unspent_colorings(&mut wallet_recv, "recv after 3rd send, settled");

    // balances with transfer Settled
    let transfers = test_list_transfers(&wallet_send, Some(&asset_1.asset_id));
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1 - amount_2 - amount_3,
        future: AMOUNT * 3 - amount_1 - amount_2 - amount_3,
        spendable: AMOUNT * 3 - amount_1 - amount_2 - amount_3,
    };
    wait_for_asset_balance(&wallet_send, &asset_1.asset_id, &expected_balance);
    let expected_balance = Balance {
        settled: amount_1 + amount_2 + amount_3,
        future: amount_1 + amount_2 + amount_3,
        spendable: amount_1 + amount_2 + amount_3,
    };
    wait_for_asset_balance(&wallet_recv, &asset_1.asset_id, &expected_balance);
}

#[test]
#[parallel]
fn fail() {
    let wallet = get_test_wallet(true, None);

    // bad asset_id returns an error
    let result = test_get_asset_balance_result(&wallet, "rgb1inexistent");
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
