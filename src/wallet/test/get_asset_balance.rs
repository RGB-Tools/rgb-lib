use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_funded_party!();

    // issue an NIA asset
    let asset = party.issue_asset_nia(None);

    // balances after issuance
    let bak_info_before = party.db_backup_info();
    let asset_balance = party.get_asset_balance(&asset.asset_id);
    let bak_info_after = party.db_backup_info();
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
    let asset = party.issue_asset_cfa(None, None);

    // balances after issuance
    let asset_balance = party.get_asset_balance(&asset.asset_id);
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
    // recipient wallet with a single UTXO
    let mut send_party = get_funded_noutxo_party!();
    send_party.create_utxos(true, Some(3), None, FEE_RATE, None);
    let mut recv_party = get_funded_noutxo_party!();
    recv_party.create_utxos(true, Some(1), None, FEE_RATE, None);

    // issue
    let asset_1 = send_party.issue_asset_nia(Some(&[AMOUNT, AMOUNT, AMOUNT]));
    let asset_2 = send_party.issue_asset_cfa(Some(&[AMOUNT, AMOUNT, AMOUNT]), None);

    // create 2 more UTXOs on the sender wallet
    send_party.create_utxos(false, Some(2), None, FEE_RATE, None);

    // balances after issuance
    send_party.show_unspent_colorings("send after issuance");
    recv_party.show_unspent_colorings("recv after issuance");
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance_1);
    send_party.wait_for_asset_balance(&asset_2.asset_id, &expected_balance_2);
    // receiver side after issuance (no asset yet)
    let result_1 = recv_party.get_asset_balance_result(&asset_1.asset_id);
    let result_2 = recv_party.get_asset_balance_result(&asset_2.asset_id);
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

    let receive_data_fail = recv_party.blind_receive();
    recv_party.fail_transfers_single(receive_data_fail.batch_transfer_idx);

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
    let batch_transfer_idx = send_party
        .send_result(&recipient_map)
        .unwrap()
        .batch_transfer_idx;
    // sender balances after send / before fail
    send_party.show_unspent_colorings("send after send / before fail");
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance_1);
    send_party.wait_for_asset_balance(&asset_2.asset_id, &expected_balance_2);
    // fail the transfer
    send_party.fail_transfers_single(batch_transfer_idx);
    // sender balances after fail
    send_party.show_unspent_colorings("send after fail");
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance_1);
    send_party.wait_for_asset_balance(&asset_2.asset_id, &expected_balance_2);

    //
    // a 1st transfer (blinded)
    //

    // send
    let receive_data_1 = recv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount_1),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    send_party.send_retry(&recipient_map);
    // sender balance with transfer WaitingCounterparty (recipient doesn't know the asset yet)
    send_party.show_unspent_colorings("send after 1st send");
    recv_party.show_unspent_colorings("recv after 1st send");
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance_1);
    send_party.wait_for_asset_balance(&asset_2.asset_id, &expected_balance_2);
    let transfers_recv = recv_party.list_transfers(None);
    assert_eq!(transfers_recv.len(), 2);
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::WaitingCounterparty
    );

    // asset_2 (extra) allocation should be recognized as unspendable
    let receive_data_fail = recv_party.blind_receive();
    let recipient_map_fail = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_fail.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(AMOUNT * 3),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = send_party.send_begin_result(&recipient_map_fail);
    let assignments_collection = AssignmentsCollection {
        fungible: 1332,
        non_fungible: false,
        inflation: 0,
    };
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id, available }) if asset_id == asset_2.asset_id && available == assignments_collection );
    // fail + delete blind receive
    recv_party.fail_transfers_single(receive_data_fail.batch_transfer_idx);
    recv_party.delete_transfers(Some(receive_data_fail.batch_transfer_idx), false);

    // take transfers from WaitingCounterparty to WaitingConfirmations
    recv_party.wait_for_refresh(None);
    send_party.wait_for_refresh(Some(&asset_1.asset_id));
    // balances with transfer WaitingConfirmations
    send_party.show_unspent_colorings("send after 1st send, WaitingConfirmations");
    recv_party.show_unspent_colorings("recv after 1st send, WaitingConfirmations");
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance_1);
    send_party.wait_for_asset_balance(&asset_2.asset_id, &expected_balance_2);
    let transfers_recv = recv_party.list_transfers(Some(&asset_1.asset_id));
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
    recv_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance_1);

    // take transfers from WaitingConfirmations to Settled
    mine(false);
    recv_party.wait_for_refresh(Some(&asset_1.asset_id));
    send_party.wait_for_refresh(Some(&asset_1.asset_id));
    // balances with transfer Settled
    send_party.show_unspent_colorings("send after 1st send, settled");
    recv_party.show_unspent_colorings("recv after 1st send, settled");
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance_1);
    send_party.wait_for_asset_balance(&asset_2.asset_id, &expected_balance_2);
    let transfers_recv = recv_party.list_transfers(Some(&asset_1.asset_id));
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::Settled
    );
    let expected_balance_1 = Balance {
        settled: amount_1,
        future: amount_1,
        spendable: amount_1,
    };
    recv_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance_1);

    //
    // a 2nd transfer (blinded)
    //

    // send some assets
    let receive_data_2 = recv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount_2),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    send_party.send_retry(&recipient_map);

    send_party.show_unspent_colorings("send after 2nd send");
    recv_party.show_unspent_colorings("recv after 2nd send");

    // balances with transfer WaitingCounterparty
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);
    let transfers = recv_party.list_transfers(None);
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
    recv_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);

    // take transfers from WaitingCounterparty to WaitingConfirmations
    recv_party.wait_for_refresh(None);
    send_party.wait_for_refresh(Some(&asset_1.asset_id));

    // balances with transfer WaitingConfirmations
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1,
        future: AMOUNT * 3 - amount_1 - amount_2,
        spendable: AMOUNT * 2,
    };
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);
    let transfers_recv = recv_party.list_transfers(Some(&asset_1.asset_id));
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
    recv_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);

    // take transfers from WaitingConfirmations to Settled
    mine(false);
    recv_party.wait_for_refresh(Some(&asset_1.asset_id));
    send_party.wait_for_refresh(Some(&asset_1.asset_id));

    send_party.show_unspent_colorings("send after 2nd send, settled");
    recv_party.show_unspent_colorings("recv after 2nd send, settled");

    // balances with transfer Settled
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1 - amount_2,
        future: AMOUNT * 3 - amount_1 - amount_2,
        spendable: AMOUNT * 3 - amount_1 - amount_2,
    };
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);
    let transfers_recv = recv_party.list_transfers(Some(&asset_1.asset_id));
    assert_eq!(
        transfers_recv.last().unwrap().status,
        TransferStatus::Settled
    );
    let expected_balance = Balance {
        settled: amount_1 + amount_2,
        future: amount_1 + amount_2,
        spendable: amount_1 + amount_2,
    };
    recv_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);

    //
    // a 3rd transfer (witness, donation)
    //

    // send some assets, donation = true to broadcast right away
    let receive_data_3 = recv_party.witness_receive();
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
    send_party
        .wallet
        .send(
            send_party.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap();

    // sync the wallets
    send_party
        .wallet
        .sync(
            send_party.online,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();
    recv_party
        .wallet
        .sync(
            recv_party.online,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();

    send_party.show_unspent_colorings("send after 3rd send");
    recv_party.show_unspent_colorings("recv after 3rd send");

    // balances after sync but before refresh
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
    assert_eq!(transfers.len(), 5);
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations // due to donation = true
    );
    let transfers = recv_party.list_transfers(None);
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);
    let expected_balance = Balance {
        settled: amount_1 + amount_2,
        future: amount_1 + amount_2,
        spendable: amount_1 + amount_2,
    };
    recv_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);

    // take recipient transfer from WaitingCounterparty to WaitingConfirmations
    recv_party.wait_for_refresh(None);

    // balances with transfer WaitingConfirmations
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
    assert_eq!(
        transfers.last().unwrap().status,
        TransferStatus::WaitingConfirmations
    );
    let transfers = recv_party.list_transfers(Some(&asset_1.asset_id));
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
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);
    let expected_balance = Balance {
        settled: amount_1 + amount_2,
        future: amount_1 + amount_2 + amount_3,
        spendable: amount_1 + amount_2,
    };
    recv_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);

    // take transfers from WaitingConfirmations to Settled
    mine(false);
    recv_party.wait_for_refresh(Some(&asset_1.asset_id));
    send_party.wait_for_refresh(Some(&asset_1.asset_id));

    send_party.show_unspent_colorings("send after 3rd send, settled");
    recv_party.show_unspent_colorings("recv after 3rd send, settled");

    // balances with transfer Settled
    let transfers = send_party.list_transfers(Some(&asset_1.asset_id));
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    let expected_balance = Balance {
        settled: AMOUNT * 3 - amount_1 - amount_2 - amount_3,
        future: AMOUNT * 3 - amount_1 - amount_2 - amount_3,
        spendable: AMOUNT * 3 - amount_1 - amount_2 - amount_3,
    };
    send_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);
    let expected_balance = Balance {
        settled: amount_1 + amount_2 + amount_3,
        future: amount_1 + amount_2 + amount_3,
        spendable: amount_1 + amount_2 + amount_3,
    };
    recv_party.wait_for_asset_balance(&asset_1.asset_id, &expected_balance);
}

#[test]
#[parallel]
fn fail() {
    let party = offline_party!(get_test_wallet(true, None));

    // bad asset_id returns an error
    let result = party.get_asset_balance_result("rgb1inexistent");
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
