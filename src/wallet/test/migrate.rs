use super::*;

const LEGACY_FIXTURE_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

/// Extract the legacy wallet fixture into `data_dir`, returning the wallet keys.
fn extract_legacy_fixture(data_dir: &Path) -> Keys {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("legacy_nia.zip");
    let file = File::open(fixture_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    archive.extract(data_dir).unwrap();
    restore_keys(
        BitcoinNetwork::Regtest,
        LEGACY_FIXTURE_MNEMONIC.to_string(),
        WitnessVersion::Taproot,
    )
    .unwrap()
}

/// Test for the one-time migration of RGB data from the legacy filesystem store
/// (`<wallet_dir>/rgb/{stash,state,index}.dat`) into the rgb-lib database.
///
/// It uses a committed zipped wallet data dir, produced by a pre-storage build of rgb-lib.
/// Opening such a wallet is what triggers the migration.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn migrate_legacy_fs_store() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let data_dir = tmp_dir.path().to_path_buf();
    let keys = extract_legacy_fixture(&data_dir);
    let wallet_dir = data_dir.join(&keys.master_fingerprint);

    // sanity: the fixture really is a legacy (fs-store) wallet not yet migrated
    assert!(wallet_dir.join(RGB_RUNTIME_DIR).is_dir());

    // opening the wallet runs the fs -> DB migration inside `Wallet::new`
    let wallet_data = get_test_wallet_data(data_dir.to_str().unwrap());
    let wallet = Wallet::new(wallet_data, SinglesigKeys::from_keys(&keys, None)).unwrap();

    // the legacy store has been deleted, not re-run
    assert!(!wallet_dir.join(RGB_RUNTIME_DIR).exists());
    assert!(!wallet_dir.join(RGB_RUNTIME_DELETING_DIR).exists());

    // the migrated RGB data is present and queryable: the issued NIA asset (the only contract in
    // the assets dir) plus the ones received from the counterparty
    let assets_dir = wallet_dir.join(ASSETS_DIR);
    let asset_files: Vec<PathBuf> = fs::read_dir(&assets_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(asset_files.len(), 1);
    let assets = wallet.list_assets(vec![]).unwrap();
    let nia = assets.nia.unwrap();
    assert_eq!(nia.len(), 4, "issued asset + 3 received ones");
    let contract = rgbstd::containers::Contract::load_file(&asset_files[0]).unwrap();
    let issued_asset_id = contract.contract_id().to_string();
    let asset = nia
        .iter()
        .find(|a| a.asset_id == issued_asset_id)
        .expect("migrated wallet should expose its issued NIA asset");
    let balance = wallet.get_asset_balance(asset.asset_id.clone()).unwrap();
    assert!(
        balance.settled > 0,
        "migrated asset should carry its settled balance"
    );
    let transfers = wallet
        .list_transfers(AssetFilter::Id(asset.asset_id.clone()), None)
        .unwrap();
    assert!(
        !transfers.is_empty(),
        "migrated wallet should retain its transfers"
    );

    // every pending transfer of the legacy wallet is still pending, in its original status, but
    // the outgoing one waiting for the counterparty, which can no longer be completed and has
    // been failed (see the legacy_send_end test)
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    let mut pending: Vec<TransferStatus> = batch_transfers
        .into_iter()
        .filter(|bt| bt.waiting())
        .map(|bt| bt.status)
        .collect();
    pending.sort_by_key(|s| *s as u8);
    assert_eq!(
        pending,
        vec![
            TransferStatus::WaitingCounterparty,
            TransferStatus::WaitingConfirmations,
            TransferStatus::WaitingConfirmations,
            TransferStatus::WaitingConfirmations,
            TransferStatus::WaitingSafeHeight,
            TransferStatus::WaitingBroadcast,
        ]
    );

    // the legacy (V0) consignment files have been converted to the current container format:
    // the received consignments (their validated copies are dropped) and the sent consignments,
    // plus the fascia of each sent transfer
    let mut rcv_files = 0;
    let mut send_files = 0;
    let mut fascia_files = 0;
    for entry in fs::read_dir(wallet_dir.join(TRANSFERS_DIR)).unwrap() {
        let transfer_dir = entry.unwrap().path();
        let rcv_path = transfer_dir.join("rcv_compose.rgbc");
        if rcv_path.exists() {
            let consignment = RgbTransfer::load_file(&rcv_path).unwrap();
            assert!(!consignment.bundles.is_empty());
            assert!(!rcv_path.with_extension("valid.rgbc").exists());
            rcv_files += 1;
        }
        let fascia_path = transfer_dir.join(FASCIA_FILE);
        if fascia_path.exists() {
            // the legacy JSON fascia has been re-saved strict-encoded, like every other container
            assert_ne!(fs::read(&fascia_path).unwrap().first(), Some(&b'{'));
            let fascia = read_fascia(&fascia_path).expect("migrated fascia should deserialize");
            assert_eq!(
                fascia.witness_id().to_string(),
                transfer_dir.file_name().unwrap().to_str().unwrap()
            );
            fascia_files += 1;
            for entry in fs::read_dir(&transfer_dir).unwrap() {
                let out_path = entry.unwrap().path().join(CONSIGNMENT_FILE);
                if out_path.exists() {
                    let consignment = RgbTransfer::load_file(&out_path).unwrap();
                    let contract_id = consignment.contract_id().to_string();
                    assert!(nia.iter().any(|a| a.asset_id == contract_id));
                    send_files += 1;
                }
            }
        }
    }
    // 1 settled + 4 pending receives; 1 settled + 2 pending sends
    assert_eq!(rcv_files, 5);
    assert_eq!(fascia_files, 3);
    assert_eq!(send_files, 3);

    // opening again is a no-op (idempotent): the legacy dir is gone, so nothing re-runs, except
    // that a retired store left behind by a run that crashed mid-delete is swept up
    let deleting_dir = wallet_dir.join(RGB_RUNTIME_DELETING_DIR);
    fs::create_dir(&deleting_dir).unwrap();
    fs::write(deleting_dir.join("stash.dat"), b"leftover").unwrap();
    let wallet_data = get_test_wallet_data(data_dir.to_str().unwrap());
    Wallet::new(wallet_data, SinglesigKeys::from_keys(&keys, None)).unwrap();
    assert!(!deleting_dir.exists());

    // nor does such a leftover ride along in backups
    fs::create_dir(&deleting_dir).unwrap();
    fs::write(deleting_dir.join("stash.dat"), b"leftover").unwrap();
    let backup_file_path = tmp_dir.path().join("test_migrate.rgb-lib_backup");
    let backup_file = backup_file_path.to_str().unwrap();
    wallet.backup(backup_file, PASSWORD).unwrap();
    let restore_dir_path = tmp_dir.path().join("restored");
    let restore_dir: &str = restore_dir_path.to_str().unwrap();
    restore_backup(backup_file, PASSWORD, restore_dir).unwrap();
    let restored_wallet_dir = restore_dir_path.join(&keys.master_fingerprint);
    assert!(
        restored_wallet_dir.join(RGB_LIB_DB_NAME).exists(),
        "the backup should carry the DB, which is where the RGB data now lives"
    );
    assert!(
        !restored_wallet_dir.join(RGB_RUNTIME_DELETING_DIR).exists(),
        "a leftover retired legacy store should be excluded from backups"
    );
}

/// Collect the witness TXs carried by every consignment (received or sent) under `dir`.
fn collect_consignment_txs(dir: &Path, txs: &mut HashMap<String, BdkTransaction>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_consignment_txs(&path, txs);
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap();
        if name == CONSIGNMENT_RCV_FILE || name == CONSIGNMENT_FILE {
            let consignment = RgbTransfer::load_file(&path).unwrap();
            for witness_bundle in consignment.bundled_witnesses() {
                txs.insert(
                    witness_bundle.tx.compute_txid().to_string(),
                    witness_bundle.tx.clone(),
                );
            }
        }
    }
}

/// A wallet upgraded with transfers still pending must bring every one of them to a final status
/// without errors (nor panics): the ones whose TX is on chain settle, the others are failed since,
/// the proxy protocol being not backward compatible, they can no longer be completed.
///
/// The fixture holds one batch transfer per pending status (see the storage migration test), the
/// ones that cannot complete already expired. The outgoing one waiting for the counterparty is
/// failed by the upgrade itself (see the legacy_send_end test), so it's not among them. The regtest knows nothing of the fixture's TXs, so
/// the chain is mocked: every TX carried by the wallet's consignments counts as mined in the
/// current tip block, except for the transfers whose counterparty never broadcast (the ones not
/// yet past the ACK), which stay unknown.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn legacy_pending_transfers() {
    initialize();

    let tmp_dir = tempfile::tempdir().unwrap();
    let data_dir = tmp_dir.path().to_path_buf();
    let keys = extract_legacy_fixture(&data_dir);
    let wallet_dir = data_dir.join(&keys.master_fingerprint);
    let wallet_data = get_test_wallet_data(data_dir.to_str().unwrap());
    let mut wallet = Wallet::new(wallet_data, SinglesigKeys::from_keys(&keys, None)).unwrap();
    let online = wallet.go_online(test_go_online_options(None)).unwrap();
    let mut w = party!(wallet, online);

    let pending: Vec<DbBatchTransfer> = w
        .db_batch_transfers()
        .into_iter()
        .filter(|bt| bt.waiting())
        .collect();
    assert_eq!(pending.len(), 6);
    assert!(pending.iter().all(|bt| bt.is_legacy()));
    let status_of = |w: &SinglesigParty, idx: i32| {
        w.db_batch_transfers()
            .into_iter()
            .find(|bt| bt.idx == idx)
            .unwrap()
            .status
    };

    // mock the chain
    let never_broadcast: HashSet<String> = pending
        .iter()
        .filter(|bt| {
            matches!(
                bt.status,
                TransferStatus::WaitingCounterparty
                    | TransferStatus::WaitingSafeHeight
                    | TransferStatus::WaitingBroadcast
            )
        })
        .filter_map(|bt| bt.txid.clone())
        .collect();
    let tip = w.wallet.indexer().get_latest_block_height().unwrap();
    let mut txs = HashMap::new();
    collect_consignment_txs(&wallet_dir.join(TRANSFERS_DIR), &mut txs);
    MOCK_CHAIN_TXS.with_borrow_mut(|m| {
        *m = txs
            .into_iter()
            .filter(|(txid, _)| !never_broadcast.contains(txid))
            .map(|(txid, tx)| (txid, (tx, tip)))
            .collect();
    });

    // refresh (no chain sync: the regtest doesn't know the wallet) and check that no transfer
    // errored, that the ones whose TX is on chain settled and that the others failed
    let refresh_and_check = |w: &mut SinglesigParty| {
        let refresh_res = w.wallet.refresh(w.online, None, vec![], true).unwrap();
        let mut errors = vec![];
        for bt in &pending {
            // a settled or failed transfer is no longer refreshed
            let failure = refresh_res.get(&bt.idx).and_then(|r| r.failure.as_ref());
            let status = status_of(w, bt.idx);
            println!(
                "batch transfer {} ({}, was {:?}): now {status:?}, failure: {failure:?}",
                bt.idx,
                if bt.incoming { "incoming" } else { "outgoing" },
                bt.status,
            );
            if let Some(e) = failure {
                errors.push(format!("batch transfer {} ({:?}): {e}", bt.idx, bt.status));
            }
            let expected = match bt.status {
                TransferStatus::WaitingConfirmations => TransferStatus::Settled,
                _ => TransferStatus::Failed,
            };
            if status != expected {
                errors.push(format!(
                    "batch transfer {} ({:?}): expected {expected:?}, got {status:?}",
                    bt.idx, bt.status
                ));
            }
        }
        assert!(
            errors.is_empty(),
            "refresh problems:\n{}",
            errors.join("\n")
        );
    };
    refresh_and_check(&mut w);

    // nothing is left pending: a further refresh (and a fail of the expired transfers) is a no-op
    refresh_and_check(&mut w);
    assert!(!w.fail_transfers(None, false, true).unwrap());

    MOCK_CHAIN_TXS.with_borrow_mut(|m| m.clear());
}

/// A send begun before the upgrade can no longer be ended: its transfer data predates the proxy
/// protocol update (so it doesn't parse anymore) and the protocol keeps no backward compatibility.
/// The upgrade fails it, as it does with a send already ended but still waiting for the
/// counterparty, and ending either of them (again) is refused.
///
/// The fixture has no send begun and not ended, so one is made out of its outgoing transfer
/// waiting for the counterparty, by setting it back to initiated before the upgrade.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn legacy_send_end() {
    initialize();

    // the fixture's outgoing transfer waiting for the counterparty
    let txid = "cef09634f935441ea8ee74c5324558c964de9b68b275e52e6dc939a7f56d13fb";

    for status in [
        TransferStatus::Initiated,
        TransferStatus::WaitingCounterparty,
    ] {
        let tmp_dir = tempfile::tempdir().unwrap();
        let data_dir = tmp_dir.path().to_path_buf();
        let keys = extract_legacy_fixture(&data_dir);
        let wallet_dir = data_dir.join(&keys.master_fingerprint);
        let db_path = wallet_dir.join(RGB_LIB_DB_NAME);
        let db = block_on(sea_orm::Database::connect(format!(
            "sqlite:{}?mode=rw",
            db_path.display()
        )))
        .unwrap();
        block_on(sea_orm::ConnectionTrait::execute_unprepared(
            &db,
            &format!(
                "UPDATE batch_transfer SET status = {} WHERE txid = '{txid}' AND NOT incoming",
                status as u8
            ),
        ))
        .unwrap();
        block_on(db.close()).unwrap();

        // the upgrade fails the transfer
        let wallet_data = get_test_wallet_data(data_dir.to_str().unwrap());
        let mut wallet = Wallet::new(wallet_data, SinglesigKeys::from_keys(&keys, None)).unwrap();
        let online = wallet.go_online(test_go_online_options(None)).unwrap();
        let mut w = party!(wallet, online);
        let batch_transfer = w
            .db_batch_transfers()
            .into_iter()
            .find(|bt| bt.txid.as_deref() == Some(txid) && !bt.incoming)
            .unwrap();
        assert_eq!(batch_transfer.status, TransferStatus::Failed);

        // ending it is refused
        let signed_psbt = fs::read_to_string(
            wallet_dir
                .join(TRANSFERS_DIR)
                .join(txid)
                .join("signed.psbt"),
        )
        .unwrap();
        let result = w.wallet.send_end(w.online, signed_psbt);
        assert_matches!(result, Err(Error::CannotEndTransfer { txid: t }) if t == txid);
    }
}
