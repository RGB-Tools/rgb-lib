use super::*;

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
    let inflation_rights = [100, 500, 200];
    let asset = test_issue_asset_ifa(
        &mut wallet,
        online,
        Some(&issue_amounts),
        Some(&inflation_rights),
        None,
    );
    show_unspent_colorings(&mut wallet, "after issue");
    let initial_supply = issue_amounts.iter().sum::<u64>();
    let total_inflatable = inflation_rights.iter().sum::<u64>();
    let max_supply = initial_supply + total_inflatable;
    assert_eq!(asset.initial_supply, initial_supply);
    assert_eq!(asset.known_circulating_supply, initial_supply);
    assert_eq!(asset.max_supply, max_supply);
    let transfers = test_list_transfers(&wallet, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);
    let unspents: Vec<Unspent> = test_list_unspents(&mut wallet, None, false)
        .into_iter()
        .filter(|u| u.utxo.colorable)
        .collect();
    assert_eq!(unspents.len(), 5);
    assert!(unspents.iter().all(|u| u.rgb_allocations.len() == 1));

    // inflate
    test_create_utxos_default(&mut wallet, online);
    let inflation_amounts = [199, 42];
    let bak_info_before = wallet.database().get_backup_info().unwrap().unwrap();
    let res = test_inflate(&mut wallet, online, &asset.asset_id, &inflation_amounts);
    let bak_info_after = wallet.database().get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    show_unspent_colorings(&mut wallet, "after inflate");
    let total_inflated = inflation_amounts.iter().sum::<u64>();
    let total_issued = initial_supply + total_inflated;
    // check updated balance
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: initial_supply,
            future: total_issued,
            spendable: initial_supply,
        }
    );

    mine(false, false);

    assert!(test_refresh_asset(&mut wallet, online, &asset.asset_id));
    show_unspent_colorings(&mut wallet, "after inflate mine + refresh");

    let transfers = test_list_transfers(&wallet, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 3);
    let mut created_at = None;
    let mut updated_at = None;
    let mut recipient_id_set = HashSet::new();
    let mut receive_utxo_set = HashSet::new();
    let mut change_utxo = None;
    let mut inflation_rights_sorted = inflation_rights;
    inflation_rights_sorted.sort();
    let inflation_change = inflation_rights_sorted.iter().take(2).sum::<u64>() - total_inflated;
    for (i, amt) in inflation_amounts.iter().enumerate() {
        let transfer = transfers.get(i + 1).unwrap();
        assert_eq!(transfer.batch_transfer_idx, 2);
        if created_at.is_none() {
            created_at = Some(transfer.created_at);
        } else {
            assert_eq!(created_at, Some(transfer.created_at));
        }
        if updated_at.is_none() {
            updated_at = Some(transfer.updated_at);
        } else {
            assert_eq!(updated_at, Some(transfer.updated_at));
        }
        assert_eq!(transfer.status, TransferStatus::Settled);
        assert_eq!(
            transfer.requested_assignment.as_ref().unwrap(),
            &Assignment::Fungible(*amt)
        );
        assert_eq!(
            transfer.assignments,
            vec![
                Assignment::InflationRight(inflation_change),
                Assignment::Fungible(inflation_amounts[0]),
                Assignment::Fungible(inflation_amounts[1])
            ]
        );
        assert_eq!(transfer.kind, TransferKind::Inflation);
        assert_eq!(transfer.txid.as_ref().unwrap(), &res.txid);
        assert!(transfer.recipient_id.is_some());
        recipient_id_set.insert(&transfer.recipient_id);
        assert!(transfer.receive_utxo.is_some());
        receive_utxo_set.insert(&transfer.receive_utxo);
        assert!(transfer.change_utxo.is_some());
        if change_utxo.is_none() {
            change_utxo = transfer.change_utxo.clone();
        } else {
            assert_eq!(change_utxo, transfer.change_utxo)
        }
        assert!(transfer.expiration_timestamp.is_none());
        assert!(transfer.transport_endpoints.is_empty());
        assert!(transfer.invoice_string.is_none());
        assert!(transfer.consignment_path.is_some());
    }
    assert_eq!(recipient_id_set.len(), inflation_amounts.len());
    assert_eq!(receive_utxo_set.len(), inflation_amounts.len());

    // check balance + remaining inflation rights
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: total_issued,
            future: total_issued,
            spendable: total_issued,
        }
    );
    let unspents = test_list_unspents(&mut wallet, Some(online), false);
    let inflation_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::InflationRight(_)))
    });
    let sum = inflation_allocations
        .map(|a| a.assignment.inflation_amount())
        .sum::<u64>();
    assert_eq!(sum, total_inflatable - total_inflated);

    // send
    let receive_data = test_blind_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(total_issued),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, online, &recipient_map);
    assert!(!txid.is_empty());
    show_unspent_colorings(&mut wallet, "after send");
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let tte_data = wallet
        .database()
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
    assert_eq!(tte_data.len(), 1);
    let ce = tte_data.first().unwrap();
    assert_eq!(ce.1.endpoint, PROXY_URL);
    assert!(ce.0.used);

    // check balance (no assets left) + remaining inflation rights
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: total_issued,
            future: 0,
            spendable: 0,
        }
    );
    let unspents = test_list_unspents(&mut wallet, None, false);
    let inflation_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::InflationRight(_)))
    });
    let sum = inflation_allocations
        .map(|a: &RgbAllocation| a.assignment.inflation_amount())
        .sum::<u64>();
    assert_eq!(sum, total_inflatable - total_inflated);

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
            future: total_issued,
            spendable: 0,
        }
    );
    assert_eq!(rcv_asset.initial_supply, initial_supply);
    assert_eq!(rcv_asset.max_supply, max_supply);
    assert_eq!(
        rcv_asset.known_circulating_supply,
        initial_supply + total_inflated
    );
    show_unspent_colorings(&mut wallet, "after send refresh 1");

    // transfers progress to status Settled after tx mining + refresh
    mine(false, false);
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
    show_unspent_colorings(&mut wallet, "after send mine + refresh 2");

    // check balance (no assets left) + remaining inflation rights
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );
    let unspents = test_list_unspents(&mut wallet, None, false);
    let inflation_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::InflationRight(_)))
    });
    let sum = inflation_allocations
        .map(|a: &RgbAllocation| a.assignment.inflation_amount())
        .sum::<u64>();
    assert_eq!(sum, total_inflatable - total_inflated);

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.change_utxo, None);

    let asset_metadata = test_get_asset_metadata(&rcv_wallet, &asset.asset_id);
    assert_eq!(asset_metadata.initial_supply, initial_supply);
    assert_eq!(asset_metadata.max_supply, max_supply);
    assert_eq!(
        asset_metadata.known_circulating_supply,
        initial_supply + total_inflated
    );

    // check there's no change (sent all)
    assert!(transfer_data.change_utxo.is_none());

    // exhaust all inflation rights by doing a last call to inflate
    test_create_utxos_default(&mut wallet, online);
    let remaining_inflatable = total_inflatable - total_inflated;
    let last_inflation_amounts = [remaining_inflatable];
    test_inflate(
        &mut wallet,
        online,
        &asset.asset_id,
        &last_inflation_amounts,
    );
    mine(false, false);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
    show_unspent_colorings(&mut wallet, "after last inflate mine + refresh");

    // check all inflation rights are exhausted
    let unspents = test_list_unspents(&mut wallet, None, false);
    let inflation_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::InflationRight(_)))
    });
    let sum = inflation_allocations
        .map(|a: &RgbAllocation| a.assignment.inflation_amount())
        .sum::<u64>();
    assert_eq!(sum, 0);

    // check balance reflects the last inflate
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_inflatable,
            future: remaining_inflatable,
            spendable: remaining_inflatable,
        }
    );

    // check known circulating supply equals max supply (all rights exhausted)
    let asset_metadata = test_get_asset_metadata(&wallet, &asset.asset_id);
    assert_eq!(asset_metadata.known_circulating_supply, max_supply);

    // send the last inflated tokens to rcv_wallet to validate the full consignment history,
    // including the inflate transition that had no inflation change
    let receive_data_2 = test_blind_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(remaining_inflatable),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid2 = test_send(&mut wallet, online, &recipient_map);
    assert!(!txid2.is_empty());

    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);

    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data_2, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer_2);
    assert_eq!(rcv_transfer_data_2.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let max_inflation = 1000;

    let (mut wallet, online) = get_funded_wallet!();

    let asset_ifa = test_issue_asset_ifa(&mut wallet, online, None, Some(&[max_inflation]), None);

    // don't check inflate input params (checked in _begin/_end sections below)

    // inflate errors
    // - watch-only (_check_xprv)
    let mut wallet_wo = get_test_wallet(false, None);
    let online_wo = wallet_wo
        .go_online(false, ELECTRUM_URL.to_string())
        .unwrap();
    let result = test_inflate_result(&mut wallet_wo, online_wo, &asset_ifa.asset_id, &[1]);
    assert_matches!(result, Err(Error::WatchOnly));

    // - wrong online
    let wrong_online = Online { id: 1 };

    // inflate_begin input params
    // - check online is correct
    let result = test_inflate_begin_result(&mut wallet, wrong_online, &asset_ifa.asset_id, &[1]);
    assert_matches!(result, Err(Error::CannotChangeOnline));
    // - invalid asset_id
    let result = test_inflate_begin_result(&mut wallet, online, "malformed", &[]);
    assert_matches!(result, Err(Error::AssetNotFound { asset_id: _ }));
    // - check empty inflation_amounts
    let result = test_inflate_begin_result(&mut wallet, online, &asset_ifa.asset_id, &[]);
    assert_matches!(result, Err(Error::NoInflationAmounts));
    // - check inflation_amounts sum > u64 max
    let result =
        test_inflate_begin_result(&mut wallet, online, &asset_ifa.asset_id, &[u64::MAX, 1]);
    assert_matches!(result, Err(Error::TooHighInflationAmounts));
    // - check inflation_amounts sum > max inflation
    let result = test_inflate_begin_result(
        &mut wallet,
        online,
        &asset_ifa.asset_id,
        &[max_inflation + 1],
    );
    assert_matches!(
        result,
        Err(Error::InsufficientAssignments {
            asset_id: _,
            available: _
        })
    );
    // - check fee_rate
    //   - low
    let result = wallet.inflate_begin(
        online,
        asset_ifa.asset_id.clone(),
        vec![1],
        0,
        MIN_CONFIRMATIONS,
        false,
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW);
    //   - overflow
    let result = wallet.inflate_begin(
        online,
        asset_ifa.asset_id.clone(),
        vec![1],
        u64::MAX,
        MIN_CONFIRMATIONS,
        false,
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER);

    // inflate_begin errors
    // - inexistent asset
    let result = test_inflate_begin_result(&mut wallet, online, "rgb1nexistent", &[]);
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
        .go_online(true, ELECTRUM_URL.to_string())
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
    mine(false, false);
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
        .go_online(true, ELECTRUM_URL.to_string())
        .unwrap();
    let result = test_inflate_begin_result(&mut wallet_nia, online_nia, &asset_ifa.asset_id, &[1]);
    assert_matches!(result, Err(Error::UnsupportedSchema { asset_schema: _ }));
    // - inflation not supported
    let asset_nia = test_issue_asset_nia(&mut wallet, online, None);
    let asset_cfa = test_issue_asset_cfa(&mut wallet, online, None, None);
    let asset_uda = test_issue_asset_uda(&mut wallet, online, None, None, vec![]);
    let unsupported_asset_ids = [
        (asset_nia.asset_id, AssetSchema::Nia),
        (asset_cfa.asset_id, AssetSchema::Cfa),
        (asset_uda.asset_id, AssetSchema::Uda),
    ];
    for (asset_id, schema) in unsupported_asset_ids {
        let inflation_amounts = vec![200, 42];
        let result = test_inflate_result(&mut wallet, online, &asset_id, &inflation_amounts);
        assert_matches!(result, Err(Error::UnsupportedInflation { asset_schema }) if asset_schema == schema);
    }
    // - inflation amounts (none, zero)
    let result = test_inflate_begin_result(&mut wallet, online, &asset_ifa.asset_id, &[]);
    assert_matches!(result, Err(Error::NoInflationAmounts));
    let result = test_inflate_begin_result(&mut wallet, online, &asset_ifa.asset_id, &[1, 0, 2]);
    assert_matches!(result, Err(Error::InvalidAmountZero));

    // inflate_end input params
    let address = test_get_address(&mut wallet);
    let unsigned_psbt = wallet
        .send_btc_begin(online, address, 1000, FEE_RATE, false, true)
        .unwrap();
    let signed_psbt = wallet.sign_psbt(unsigned_psbt, None).unwrap();
    // - check online is correct
    let result = test_inflate_end_result(&mut wallet, wrong_online, &signed_psbt);
    assert_matches!(result, Err(Error::CannotChangeOnline));
    // - check signed_psbt is valid
    let result = test_inflate_end_result(&mut wallet, online, "");
    assert_matches!(result, Err(Error::InvalidPsbt { details: _ }));

    // inflate_end errors
    // - no prior inflate_begin
    test_create_utxos(&mut wallet, online, false, Some(1), None, FEE_RATE, None);
    let unsigned_psbt = test_inflate_begin(&mut wallet, online, &asset_ifa.asset_id, &[1]);
    let signed_psbt = wallet.sign_psbt(unsigned_psbt, None).unwrap();
    let psbt_txid = Psbt::from_str(&signed_psbt)
        .unwrap()
        .extract_tx()
        .unwrap()
        .compute_txid()
        .to_string();
    let (mut wallet_2, online_2) = get_empty_wallet!();
    let result = test_inflate_end_result(&mut wallet_2, online_2, &signed_psbt);
    assert_matches!(result, Err(Error::UnknownTransfer { txid }) if txid == psbt_txid);
}
