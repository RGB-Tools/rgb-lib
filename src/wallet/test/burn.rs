use super::*;

#[cfg(feature = "electrum")]
fn assert_burn_unspents(
    wallet: &mut Wallet,
    asset_id: &str,
    expected_change: Option<(&Outpoint, u64)>,
) {
    let unspents = test_list_unspents(wallet, None, false);

    // the burnt allocation (Fungible(0)) must not surface anywhere in list_unspents
    let zero_count = unspents
        .iter()
        .flat_map(|u| u.rgb_allocations.iter())
        .filter(|a| {
            a.asset_id.as_deref() == Some(asset_id)
                && matches!(a.assignment, Assignment::Fungible(0))
        })
        .count();
    assert_eq!(zero_count, 0);

    // when there's change, the change_utxo must hold exactly the change allocation (no Fungible(0))
    if let Some((outpoint, amount)) = expected_change {
        let change_unspent = unspents
            .iter()
            .find(|u| &u.utxo.outpoint == outpoint)
            .expect("change_utxo missing from list_unspents");
        let asset_assignments: Vec<&Assignment> = change_unspent
            .rgb_allocations
            .iter()
            .filter(|a| a.asset_id.as_deref() == Some(asset_id))
            .map(|a| &a.assignment)
            .collect();
        assert_eq!(asset_assignments, vec![&Assignment::Fungible(amount)]);
    }
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let issue_amounts = [AMOUNT, AMOUNT];
    let asset = test_issue_asset_ifa(&mut wallet, online, Some(&issue_amounts), None, None);
    show_unspent_colorings(&mut wallet, "after issue");
    let initial_supply = issue_amounts.iter().sum::<u64>();
    assert_eq!(asset.initial_supply, initial_supply);
    assert_eq!(asset.known_circulating_supply, initial_supply);
    let transfers = test_list_transfers(&wallet, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);
    let unspents: Vec<Unspent> = test_list_unspents(&mut wallet, None, false)
        .into_iter()
        .filter(|u| u.utxo.colorable)
        .collect();
    assert_eq!(unspents.len(), 5);

    // burn
    test_create_utxos_default(&mut wallet, online);
    let burn_amount = 199;
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_before = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    let res = test_burn(&mut wallet, online, &asset.asset_id, burn_amount);
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_after = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    show_unspent_colorings(&mut wallet, "after burn");

    // check updated balance
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    let burn_change = AMOUNT - burn_amount;
    let remaining_after_burn = initial_supply - burn_amount;
    assert_eq!(
        balance,
        Balance {
            settled: initial_supply,
            future: remaining_after_burn,
            spendable: AMOUNT,
        }
    );

    // mine and refresh
    mine(false);
    assert!(test_refresh_asset(&mut wallet, online, &asset.asset_id));

    // check updated balance
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_after_burn,
            future: remaining_after_burn,
            spendable: remaining_after_burn,
        }
    );

    // check transfer info
    let transfers = test_list_transfers(&wallet, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 2);
    let transfer = transfers.get(1).unwrap();
    assert_eq!(transfer.batch_transfer_idx, 2);
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(
        transfer.requested_assignment.as_ref().unwrap(),
        &Assignment::Fungible(burn_amount)
    );
    assert_eq!(
        transfer.assignments,
        vec![Assignment::Fungible(burn_change)]
    );
    assert_eq!(transfer.kind, TransferKind::Burn);
    assert_eq!(transfer.txid.as_ref().unwrap(), &res.txid);
    assert!(transfer.recipient_id.is_none());
    assert!(transfer.receive_utxo.is_none());
    assert!(transfer.change_utxo.is_some());
    assert!(transfer.expiration_timestamp.is_none());
    assert!(transfer.transport_endpoints.is_empty());
    assert!(transfer.invoice_string.is_none());
    assert!(transfer.consignment_path.is_some());

    assert_burn_unspents(
        &mut wallet,
        &asset.asset_id,
        transfer.change_utxo.as_ref().map(|o| (o, burn_change)),
    );

    // inflate using all the default inflation rights, producing a new Fungible allocation
    // smaller than AMOUNT
    test_create_utxos_default(&mut wallet, online);
    let inflated_amount = AMOUNT_INFLATION;
    assert!(inflated_amount < AMOUNT);
    test_inflate(&mut wallet, online, &asset.asset_id, &[inflated_amount]);
    show_unspent_colorings(&mut wallet, "after inflate");

    let amount_after_inflate = remaining_after_burn + inflated_amount;
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_after_burn,
            future: amount_after_inflate,
            spendable: remaining_after_burn,
        }
    );

    mine(false);
    assert!(test_refresh_asset(&mut wallet, online, &asset.asset_id));
    show_unspent_colorings(&mut wallet, "after inflate mine + refresh");

    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: amount_after_inflate,
            future: amount_after_inflate,
            spendable: amount_after_inflate,
        }
    );

    // partial burn requiring both the burn change and the inflated allocation
    // pick an amount higher than each individually but lower than their sum
    // input selection sorts by main amount ascending, so it should pick the inflated allocation
    // first, then the burn change, leaving the untouched AMOUNT allocation as the only spendable
    // one before the burn tx is mined
    test_create_utxos_default(&mut wallet, online);
    let burn_amount_2 = burn_change + inflated_amount - 100;
    assert!(burn_amount_2 > burn_change);
    assert!(burn_amount_2 > inflated_amount);
    assert!(burn_amount_2 < burn_change + inflated_amount);
    let res_burn_2 = test_burn(&mut wallet, online, &asset.asset_id, burn_amount_2);
    show_unspent_colorings(&mut wallet, "after second burn");

    let remaining_amount = amount_after_inflate - burn_amount_2;
    let burn_change_2 = burn_change + inflated_amount - burn_amount_2;
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: amount_after_inflate,
            future: remaining_amount,
            spendable: AMOUNT,
        }
    );

    mine(false);
    assert!(test_refresh_asset(&mut wallet, online, &asset.asset_id));
    show_unspent_colorings(&mut wallet, "after second burn mine + refresh");

    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_amount,
            future: remaining_amount,
            spendable: remaining_amount,
        }
    );

    // check second burn transfer info
    let transfers = test_list_transfers(&wallet, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 4);
    let transfer = transfers.get(3).unwrap();
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(
        transfer.requested_assignment.as_ref().unwrap(),
        &Assignment::Fungible(burn_amount_2)
    );
    assert_eq!(
        transfer.assignments,
        vec![Assignment::Fungible(burn_change_2)]
    );
    assert_eq!(transfer.kind, TransferKind::Burn);
    assert_eq!(transfer.txid.as_ref().unwrap(), &res_burn_2.txid);
    assert!(transfer.recipient_id.is_none());
    assert!(transfer.receive_utxo.is_none());
    assert!(transfer.change_utxo.is_some());

    assert_burn_unspents(
        &mut wallet,
        &asset.asset_id,
        transfer.change_utxo.as_ref().map(|o| (o, burn_change_2)),
    );

    // send all
    let receive_data = test_blind_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(remaining_amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, online, &recipient_map);
    assert!(!txid.is_empty());
    show_unspent_colorings(&mut wallet, "after send");
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let txn = wallet.database().begin_transaction().unwrap();
    let tte_data = txn
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
    txn.commit().unwrap();
    assert_eq!(tte_data.len(), 1);
    let ce = tte_data.first().unwrap();
    assert_eq!(ce.1.endpoint, PROXY_URL);
    assert!(ce.0.used);

    // check balance (no assets left)
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_amount,
            future: 0,
            spendable: 0,
        }
    );

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _rcv_asset_transfer) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // asset has been received correctly
    let rcv_assets = test_list_assets(&rcv_wallet, &[]);
    let ifa_assets = rcv_assets.ifa.unwrap();
    assert_eq!(ifa_assets.len(), 1);
    let rcv_asset = ifa_assets.last().unwrap();
    assert_eq!(rcv_asset.asset_id, asset.asset_id);
    assert_eq!(rcv_asset.ticker, TICKER);
    assert_eq!(rcv_asset.name, NAME);
    assert_eq!(rcv_asset.precision, PRECISION);
    assert_eq!(
        rcv_asset.balance,
        Balance {
            settled: 0,
            future: remaining_amount,
            spendable: 0,
        }
    );
    assert_eq!(rcv_asset.initial_supply, initial_supply);
    show_unspent_colorings(&mut wallet, "after send refresh 1");

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
    show_unspent_colorings(&mut wallet, "after send mine + refresh 2");

    // check balance (no assets left)
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.change_utxo, None);

    let asset_metadata = test_get_asset_metadata(&rcv_wallet, &asset.asset_id);
    assert_eq!(asset_metadata.initial_supply, initial_supply);

    // check there's no change (sent all)
    assert!(transfer_data.change_utxo.is_none());

    // the receiving wallet now holds all remaining assets: burn part of them, then burn the rest
    test_create_utxos_default(&mut rcv_wallet, rcv_online);
    let rcv_burn_amount = 50;
    let res_rcv_burn = test_burn(
        &mut rcv_wallet,
        rcv_online,
        &asset.asset_id,
        rcv_burn_amount,
    );
    show_unspent_colorings(&mut rcv_wallet, "after rcv partial burn");

    let rcv_remaining = remaining_amount - rcv_burn_amount;
    let balance = test_get_asset_balance(&rcv_wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_amount,
            future: rcv_remaining,
            spendable: 0,
        }
    );

    mine(false);
    assert!(test_refresh_asset(
        &mut rcv_wallet,
        rcv_online,
        &asset.asset_id
    ));
    show_unspent_colorings(&mut rcv_wallet, "after rcv partial burn mine + refresh");

    let balance = test_get_asset_balance(&rcv_wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: rcv_remaining,
            future: rcv_remaining,
            spendable: rcv_remaining,
        }
    );

    let rcv_transfers = test_list_transfers(&rcv_wallet, Some(&asset.asset_id));
    let rcv_burn_transfer = rcv_transfers.last().unwrap();
    assert_eq!(rcv_burn_transfer.status, TransferStatus::Settled);
    assert_eq!(rcv_burn_transfer.kind, TransferKind::Burn);
    assert_eq!(
        rcv_burn_transfer.requested_assignment.as_ref().unwrap(),
        &Assignment::Fungible(rcv_burn_amount)
    );
    assert_eq!(
        rcv_burn_transfer.assignments,
        vec![Assignment::Fungible(rcv_remaining)]
    );
    assert_eq!(rcv_burn_transfer.txid.as_ref().unwrap(), &res_rcv_burn.txid);

    assert_burn_unspents(
        &mut rcv_wallet,
        &asset.asset_id,
        rcv_burn_transfer
            .change_utxo
            .as_ref()
            .map(|o| (o, rcv_remaining)),
    );

    // burn everything from the receiving wallet
    test_create_utxos_default(&mut rcv_wallet, rcv_online);
    let res_rcv_burn_all = test_burn(&mut rcv_wallet, rcv_online, &asset.asset_id, rcv_remaining);
    show_unspent_colorings(&mut rcv_wallet, "after rcv burn all");

    let balance = test_get_asset_balance(&rcv_wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: rcv_remaining,
            future: 0,
            spendable: 0,
        }
    );

    mine(false);
    assert!(test_refresh_asset(
        &mut rcv_wallet,
        rcv_online,
        &asset.asset_id
    ));
    show_unspent_colorings(&mut rcv_wallet, "after rcv burn all mine + refresh");

    let balance = test_get_asset_balance(&rcv_wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );

    let rcv_transfers = test_list_transfers(&rcv_wallet, Some(&asset.asset_id));
    let rcv_burn_all_transfer = rcv_transfers.last().unwrap();
    assert_eq!(rcv_burn_all_transfer.status, TransferStatus::Settled);
    assert_eq!(rcv_burn_all_transfer.kind, TransferKind::Burn);
    assert_eq!(
        rcv_burn_all_transfer.requested_assignment.as_ref().unwrap(),
        &Assignment::Fungible(rcv_remaining)
    );
    assert_eq!(
        rcv_burn_all_transfer.txid.as_ref().unwrap(),
        &res_rcv_burn_all.txid
    );
    // nothing left to burn: no fungible change, no change_utxo
    assert_eq!(rcv_burn_all_transfer.assignments, vec![]);
    assert!(rcv_burn_all_transfer.change_utxo.is_none());
    assert_burn_unspents(&mut rcv_wallet, &asset.asset_id, None);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    let asset_ifa = test_issue_asset_ifa(&mut wallet, online, None, None, None);

    // don't check burn input params (checked in _begin/_end sections below)

    // burn errors
    // - watch-only (_check_xprv)
    let mut wallet_wo = get_test_wallet(false, None);
    let online_wo = wallet_wo.go_online(test_go_online_options(None)).unwrap();
    let result = test_burn_result(&mut wallet_wo, online_wo, &asset_ifa.asset_id, 10);
    assert_matches!(result, Err(Error::WatchOnly));

    // - wrong online
    let wrong_online = Online { id: 1 };

    // burn_begin input params
    // - check online is correct
    let result = test_burn_begin_result(&mut wallet, wrong_online, &asset_ifa.asset_id, 10);
    assert_matches!(result, Err(Error::CannotChangeOnline));
    // - invalid asset_id
    let result = test_burn_begin_result(&mut wallet, online, "malformed", 10);
    assert_matches!(result, Err(Error::AssetNotFound { asset_id: _ }));
    // - check zero burn amount
    let result = test_burn_begin_result(&mut wallet, online, &asset_ifa.asset_id, 0);
    assert_matches!(result, Err(Error::NoBurnAmount));
    // - check fee_rate
    //   - low
    let result = wallet.burn_begin(
        online,
        asset_ifa.asset_id.clone(),
        10,
        0,
        MIN_CONFIRMATIONS,
        false,
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW);
    //   - overflow
    let result = wallet.burn_begin(
        online,
        asset_ifa.asset_id.clone(),
        10,
        u64::MAX,
        MIN_CONFIRMATIONS,
        false,
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER);

    // burn_begin errors
    // - inexistent asset
    let result = test_burn_begin_result(&mut wallet, online, "rgb1nexistent", 10);
    assert_matches!(result, Err(Error::AssetNotFound { asset_id: _ }));
    // - schema not supported
    create_test_data_dir();
    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);
    let mut wallet_nia = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![AssetSchema::Nia, AssetSchema::Ifa],
        },
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let online_nia = wallet_nia
        .go_online(test_go_online_options(Some(ELECTRUM_URL)))
        .unwrap();
    fund_wallet(wallet_nia.get_address().unwrap());
    test_create_utxos_default(&mut wallet_nia, online_nia);
    let receive_data = test_blind_receive(&mut wallet_nia);
    let recipient_map = HashMap::from([(
        asset_ifa.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, online, &recipient_map);
    assert!(!txid.is_empty());
    wait_for_refresh(&mut wallet_nia, online_nia, None, None);
    wait_for_refresh(&mut wallet, online, None, None);
    mine(false);
    wait_for_refresh(&mut wallet_nia, online_nia, None, None);
    wait_for_refresh(&mut wallet, online, None, None);
    let transfer_recv = get_test_transfer_recipient(&wallet_nia, &receive_data.recipient_id);
    let (transfer_send, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data_recv, _) = get_test_transfer_data(&wallet_nia, &transfer_recv);
    let (transfer_data_send, _) = get_test_transfer_data(&wallet, &transfer_send);
    assert_eq!(transfer_data_recv.status, TransferStatus::Settled);
    assert_eq!(transfer_data_send.status, TransferStatus::Settled);
    drop(wallet_nia);
    let mut wallet_nia = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![AssetSchema::Nia],
        },
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let online_nia = wallet_nia
        .go_online(test_go_online_options(Some(ELECTRUM_URL)))
        .unwrap();
    let result = test_burn_begin_result(&mut wallet_nia, online_nia, &asset_ifa.asset_id, 10);
    assert_matches!(result, Err(Error::UnsupportedSchema { asset_schema: _ }));
    // - burn not supported
    let asset_nia = test_issue_asset_nia(&mut wallet, online, None);
    let asset_cfa = test_issue_asset_cfa(&mut wallet, online, None, None);
    let asset_uda = test_issue_asset_uda(&mut wallet, online, None, None, vec![]);
    let unsupported_asset_ids = [
        (asset_nia.asset_id, AssetSchema::Nia),
        (asset_cfa.asset_id, AssetSchema::Cfa),
        (asset_uda.asset_id, AssetSchema::Uda),
    ];
    for (asset_id, schema) in unsupported_asset_ids {
        let result = test_burn_result(&mut wallet, online, &asset_id, 10);
        assert_matches!(result, Err(Error::UnsupportedBurn { asset_schema }) if asset_schema == schema);
    }
    // - burn zero amount
    let result = test_burn_begin_result(&mut wallet, online, &asset_ifa.asset_id, 0);
    assert_matches!(result, Err(Error::NoBurnAmount));

    // burn_end input params
    let address = test_get_address(&mut wallet);
    let unsigned_psbt = wallet
        .send_btc_begin(online, address, 1000, FEE_RATE, false, true)
        .unwrap();
    let signed_psbt = wallet.sign_psbt(unsigned_psbt, None).unwrap();
    // - check online is correct
    let result = test_burn_end_result(&mut wallet, wrong_online, &signed_psbt);
    assert_matches!(result, Err(Error::CannotChangeOnline));
    // - check signed_psbt is valid
    let result = test_burn_end_result(&mut wallet, online, "");
    assert_matches!(result, Err(Error::InvalidPsbt { details: _ }));

    // burn_end errors
    // - no prior burn_begin
    test_create_utxos(&mut wallet, online, false, Some(1), None, FEE_RATE, None);
    let unsigned_psbt = test_burn_begin(&mut wallet, online, &asset_ifa.asset_id, 10);
    let signed_psbt = wallet.sign_psbt(unsigned_psbt, None).unwrap();
    let psbt_txid = Psbt::from_str(&signed_psbt)
        .unwrap()
        .extract_tx()
        .unwrap()
        .compute_txid()
        .to_string();
    let (mut wallet_2, online_2) = get_empty_wallet!();
    let result = test_burn_end_result(&mut wallet_2, online_2, &signed_psbt);
    assert_matches!(result, Err(Error::UnknownTransfer { txid }) if txid == psbt_txid);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_end() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let asset = test_issue_asset_ifa(&mut wallet, online, None, None, None);

    // begin does not update backup_info with dry_run=true
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_before = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    let _res = wallet
        .burn_begin(
            online,
            asset.asset_id.clone(),
            AMOUNT,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            true,
        )
        .unwrap();
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_after = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // begin does update backup_info with dry_run=false
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_before = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    let res = wallet
        .burn_begin(
            online,
            asset.asset_id.clone(),
            AMOUNT,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap();
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_after = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    let signed_psbt = wallet.sign_psbt(res.psbt, None).unwrap();

    // end updates backup_info
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_before = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    wallet.burn_end(online, signed_psbt).unwrap();
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_after = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
}
