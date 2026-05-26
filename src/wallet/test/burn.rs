use super::*;

#[cfg(feature = "electrum")]
fn assert_burn_unspents(
    party: &mut SinglesigParty,
    asset_id: &str,
    expected_change: Option<(&Outpoint, u64)>,
) {
    let unspents = party.list_unspents(false);

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
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let issue_amounts = [AMOUNT, AMOUNT];
    let asset = party.issue_asset_ifa(Some(&issue_amounts), None, None);
    party.show_unspent_colorings("after issue");
    let initial_supply = issue_amounts.iter().sum::<u64>();
    assert_eq!(asset.initial_supply, initial_supply);
    assert_eq!(asset.known_circulating_supply, initial_supply);
    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);
    let unspents: Vec<Unspent> = party
        .list_unspents(false)
        .into_iter()
        .filter(|u| u.utxo.colorable)
        .collect();
    assert_eq!(unspents.len(), 5);

    // burn
    party.create_utxos_default();
    let burn_amount = 199;
    let bak_info_before = party.db_backup_info();
    let res = party.burn(&asset.asset_id, burn_amount);
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    party.show_unspent_colorings("after burn");

    // check updated balance
    let balance = party.get_asset_balance(&asset.asset_id);
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
    assert!(party.refresh_asset(&asset.asset_id));

    // check updated balance
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_after_burn,
            future: remaining_after_burn,
            spendable: remaining_after_burn,
        }
    );

    // check transfer info
    let transfers = party.list_transfers(Some(&asset.asset_id));
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
        &mut party,
        &asset.asset_id,
        transfer.change_utxo.as_ref().map(|o| (o, burn_change)),
    );

    // inflate using all the default inflation rights, producing a new Fungible allocation
    // smaller than AMOUNT
    party.create_utxos_default();
    let inflated_amount = AMOUNT_INFLATION;
    assert!(inflated_amount < AMOUNT);
    party.inflate(&asset.asset_id, &[inflated_amount]);
    party.show_unspent_colorings("after inflate");

    let amount_after_inflate = remaining_after_burn + inflated_amount;
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_after_burn,
            future: amount_after_inflate,
            spendable: remaining_after_burn,
        }
    );

    mine(false);
    assert!(party.refresh_asset(&asset.asset_id));
    party.show_unspent_colorings("after inflate mine + refresh");

    let balance = party.get_asset_balance(&asset.asset_id);
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
    party.create_utxos_default();
    let burn_amount_2 = burn_change + inflated_amount - 100;
    assert!(burn_amount_2 > burn_change);
    assert!(burn_amount_2 > inflated_amount);
    assert!(burn_amount_2 < burn_change + inflated_amount);
    let res_burn_2 = party.burn(&asset.asset_id, burn_amount_2);
    party.show_unspent_colorings("after second burn");

    let remaining_amount = amount_after_inflate - burn_amount_2;
    let burn_change_2 = burn_change + inflated_amount - burn_amount_2;
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: amount_after_inflate,
            future: remaining_amount,
            spendable: AMOUNT,
        }
    );

    mine(false);
    assert!(party.refresh_asset(&asset.asset_id));
    party.show_unspent_colorings("after second burn mine + refresh");

    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_amount,
            future: remaining_amount,
            spendable: remaining_amount,
        }
    );

    // check second burn transfer info
    let transfers = party.list_transfers(Some(&asset.asset_id));
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
        &mut party,
        &asset.asset_id,
        transfer.change_utxo.as_ref().map(|o| (o, burn_change_2)),
    );

    // send all
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(remaining_amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    party.show_unspent_colorings("after send");
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let tte_data = party.db_transfer_transport_endpoints_data(transfer.idx);
    assert_eq!(tte_data.len(), 1);
    let ce = tte_data.first().unwrap();
    assert_eq!(ce.1.endpoint, PROXY_URL);
    assert!(ce.0.used);

    // check balance (no assets left)
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_amount,
            future: 0,
            spendable: 0,
        }
    );

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_party.wait_for_refresh(None);
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _rcv_asset_transfer) = rcv_party.get_test_transfer_data(&rcv_transfer);
    party.wait_for_refresh(Some(&asset.asset_id));
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // asset has been received correctly
    let rcv_assets = rcv_party.list_assets(&[]);
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
    party.show_unspent_colorings("after send refresh 1");

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    party.show_unspent_colorings("after send mine + refresh 2");

    // check balance (no assets left)
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.change_utxo, None);

    let asset_metadata = rcv_party.get_asset_metadata(&asset.asset_id);
    assert_eq!(asset_metadata.initial_supply, initial_supply);

    // check there's no change (sent all)
    assert!(transfer_data.change_utxo.is_none());

    // the receiving wallet now holds all remaining assets: burn part of them, then burn the rest
    rcv_party.create_utxos_default();
    let rcv_burn_amount = 50;
    let res_rcv_burn = rcv_party.burn(&asset.asset_id, rcv_burn_amount);
    rcv_party.show_unspent_colorings("after rcv partial burn");

    let rcv_remaining = remaining_amount - rcv_burn_amount;
    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: remaining_amount,
            future: rcv_remaining,
            spendable: 0,
        }
    );

    mine(false);
    assert!(rcv_party.refresh_asset(&asset.asset_id));
    rcv_party.show_unspent_colorings("after rcv partial burn mine + refresh");

    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: rcv_remaining,
            future: rcv_remaining,
            spendable: rcv_remaining,
        }
    );

    let rcv_transfers = rcv_party.list_transfers(Some(&asset.asset_id));
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
        &mut rcv_party,
        &asset.asset_id,
        rcv_burn_transfer
            .change_utxo
            .as_ref()
            .map(|o| (o, rcv_remaining)),
    );

    // burn everything from the receiving wallet
    rcv_party.create_utxos_default();
    let res_rcv_burn_all = rcv_party.burn(&asset.asset_id, rcv_remaining);
    rcv_party.show_unspent_colorings("after rcv burn all");

    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: rcv_remaining,
            future: 0,
            spendable: 0,
        }
    );

    mine(false);
    assert!(rcv_party.refresh_asset(&asset.asset_id));
    rcv_party.show_unspent_colorings("after rcv burn all mine + refresh");

    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );

    let rcv_transfers = rcv_party.list_transfers(Some(&asset.asset_id));
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
    assert_burn_unspents(&mut rcv_party, &asset.asset_id, None);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // === offline tests

    let mut offline_party = {
        let wallet = get_test_wallet(true, None);
        party!(wallet, Online { id: 0 })
    };
    let result =
        offline_party
            .wallet
            .burn(Online { id: 0 }, s!(""), 0, FEE_RATE, MIN_CONFIRMATIONS);
    assert_matches!(result, Err(Error::Offline));
    let result = offline_party.wallet.burn_begin(
        Online { id: 0 },
        s!(""),
        0,
        FEE_RATE,
        MIN_CONFIRMATIONS,
        false,
    );
    assert_matches!(result, Err(Error::Offline));
    let result = offline_party.wallet.burn_end(Online { id: 0 }, s!(""));
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    let mut party = get_funded_party!();

    let asset_ifa = party.issue_asset_ifa(None, None, None);

    // don't check burn input params (checked in _begin/_end sections below)

    // burn errors
    // - watch-only (_check_xprv)
    let mut wallet_wo = get_test_wallet(false, None);
    let online_wo = wallet_wo.go_online(test_go_online_options(None)).unwrap();
    let mut party_wo = party!(wallet_wo, online_wo);
    let result = party_wo.burn_result(&asset_ifa.asset_id, 10);
    assert_matches!(result, Err(Error::WatchOnly));

    // - wrong online
    let wrong_online = Online { id: 1 };

    // burn_begin input params
    // - check online is correct
    let good_online = party.online;
    party.online = wrong_online;
    let result = party.burn_begin_result(&asset_ifa.asset_id, 10);
    party.online = good_online;
    assert_matches!(result, Err(Error::CannotChangeOnline));
    // - invalid asset_id
    let result = party.burn_begin_result("malformed", 10);
    assert_matches!(result, Err(Error::AssetNotFound { asset_id: _ }));
    // - check zero burn amount
    let result = party.burn_begin_result(&asset_ifa.asset_id, 0);
    assert_matches!(result, Err(Error::NoBurnAmount));
    // - check fee_rate
    //   - low
    let result = party.wallet.burn_begin(
        party.online,
        asset_ifa.asset_id.clone(),
        10,
        0,
        MIN_CONFIRMATIONS,
        false,
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW);
    //   - overflow
    let result = party.wallet.burn_begin(
        party.online,
        asset_ifa.asset_id.clone(),
        10,
        u64::MAX,
        MIN_CONFIRMATIONS,
        false,
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER);

    // burn_begin errors
    // - inexistent asset
    let result = party.burn_begin_result("rgb1nexistent", 10);
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
    let mut party_nia = party!(wallet_nia, online_nia);
    fund_wallet(party_nia.get_address());
    party_nia.create_utxos_default();
    let receive_data = party_nia.blind_receive();
    let recipient_map = HashMap::from([(
        asset_ifa.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    party_nia.wait_for_refresh(None);
    party.wait_for_refresh(None);
    mine(false);
    party_nia.wait_for_refresh(None);
    party.wait_for_refresh(None);
    let transfer_recv = party_nia.get_test_transfer_recipient(&receive_data.recipient_id);
    let (transfer_send, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data_recv, _) = party_nia.get_test_transfer_data(&transfer_recv);
    let (transfer_data_send, _) = party.get_test_transfer_data(&transfer_send);
    assert_eq!(transfer_data_recv.status, TransferStatus::Settled);
    assert_eq!(transfer_data_send.status, TransferStatus::Settled);
    drop(party_nia);
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
    let mut party_nia = party!(wallet_nia, online_nia);
    let result = party_nia.burn_begin_result(&asset_ifa.asset_id, 10);
    assert_matches!(result, Err(Error::UnsupportedSchema { asset_schema: _ }));
    // - burn not supported
    let asset_nia = party.issue_asset_nia(None);
    let asset_cfa = party.issue_asset_cfa(None, None);
    let asset_uda = party.issue_asset_uda(None, None, vec![]);
    let unsupported_asset_ids = [
        (asset_nia.asset_id, AssetSchema::Nia),
        (asset_cfa.asset_id, AssetSchema::Cfa),
        (asset_uda.asset_id, AssetSchema::Uda),
    ];
    for (asset_id, schema) in unsupported_asset_ids {
        let result = party.burn_result(&asset_id, 10);
        assert_matches!(result, Err(Error::UnsupportedBurn { asset_schema }) if asset_schema == schema);
    }
    // - burn zero amount
    let result = party.burn_begin_result(&asset_ifa.asset_id, 0);
    assert_matches!(result, Err(Error::NoBurnAmount));

    // burn_end input params
    let address = party.get_address();
    let unsigned_psbt = party
        .wallet
        .send_btc_begin(party.online, address, 1000, FEE_RATE, false, true)
        .unwrap();
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt, None).unwrap();
    // - check online is correct
    let good_online = party.online;
    party.online = wrong_online;
    let result = party.burn_end_result(&signed_psbt);
    party.online = good_online;
    assert_matches!(result, Err(Error::CannotChangeOnline));
    // - check signed_psbt is valid
    let result = party.burn_end_result("");
    assert_matches!(result, Err(Error::InvalidPsbt { details: _ }));

    // burn_end errors
    // - no prior burn_begin
    party.create_utxos(false, Some(1), None, FEE_RATE, None);
    let unsigned_psbt = party.burn_begin(&asset_ifa.asset_id, 10);
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt, None).unwrap();
    let psbt_txid = Psbt::from_str(&signed_psbt)
        .unwrap()
        .extract_tx()
        .unwrap()
        .compute_txid()
        .to_string();
    let mut party_2 = get_empty_party!();
    let result = party_2.burn_end_result(&signed_psbt);
    assert_matches!(result, Err(Error::UnknownTransfer { txid }) if txid == psbt_txid);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_end() {
    initialize();

    let mut party = get_funded_party!();
    let asset = party.issue_asset_ifa(None, None, None);

    // begin does not update backup_info with dry_run=true
    let bak_info_before = party.db_backup_info();
    let _res = party
        .wallet
        .burn_begin(
            party.online,
            asset.asset_id.clone(),
            AMOUNT,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            true,
        )
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // begin does update backup_info with dry_run=false
    let bak_info_before = party.db_backup_info();
    let res = party
        .wallet
        .burn_begin(
            party.online,
            asset.asset_id.clone(),
            AMOUNT,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();

    // end updates backup_info
    let bak_info_before = party.db_backup_info();
    party.wallet.burn_end(party.online, signed_psbt).unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
}
