use super::*;

const LEGACY_FIXTURE_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

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
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("legacy_nia.zip");
    let file = File::open(fixture_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    archive.extract(tmp_dir.path()).unwrap();
    let data_dir = tmp_dir.path().to_path_buf();
    let keys = restore_keys(
        BitcoinNetwork::Regtest,
        LEGACY_FIXTURE_MNEMONIC.to_string(),
        WitnessVersion::Taproot,
    )
    .unwrap();
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

    // every pending transfer of the legacy wallet is still pending, in its original status
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
