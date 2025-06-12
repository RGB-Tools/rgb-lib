use super::*;

#[cfg(all(feature = "electrum", feature = "esplora"))]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut wallet = get_test_wallet(true, None);

    // go online
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_before.is_none());
    let result_1 = test_go_online_result(&mut wallet, false, None);
    let bak_info_after = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_after.is_none());
    assert!(result_1.is_ok());

    // can go online again with the same electrum URL
    let result_2 = test_go_online_result(&mut wallet, false, None);
    assert!(result_2.is_ok());
    assert_eq!(result_1.unwrap(), result_2.unwrap());

    // can go online again with a different electrum URL
    let result_3 = test_go_online_result(&mut wallet, false, Some(ELECTRUM_2_URL));
    assert!(result_3.is_ok());

    // can go online again with esplora URL
    let result_4 = test_go_online_result(&mut wallet, false, Some(ESPLORA_URL));
    assert!(result_4.is_ok());
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let mut wallet = get_test_wallet(true, None);
    let mut wallet_testnet = get_test_wallet_with_net(true, None, BitcoinNetwork::Testnet);

    // cannot go online with an invalid indexer URL
    let result = test_go_online_result(&mut wallet, false, Some("other:50001"));
    let details = "not a valid electrum nor esplora server";
    assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details ));

    // cannot go online again with an invalid indexer URL
    let indexer_url = if cfg!(feature = "electrum") {
        ELECTRUM_URL
    } else {
        ESPLORA_URL
    };
    test_go_online(&mut wallet, false, Some(indexer_url));
    let result = test_go_online_result(&mut wallet, false, Some("other:50001"));
    assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details ));

    #[cfg(feature = "electrum")]
    {
        // electrs wrong network
        let result = test_go_online_result(&mut wallet_testnet, false, None);
        let details = "indexer is for a network different from the wallet's one";
        assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details ));

        // unsupported electrs variant
        let result = test_go_online_result(&mut wallet, false, Some(ELECTRUM_BLOCKSTREAM_URL));
        let details = "verbose transactions are currently unsupported";
        assert!(matches!(result, Err(Error::InvalidElectrum { details: m }) if m == details ));
    }

    #[cfg(feature = "esplora")]
    {
        // esplora wrong network
        let result = test_go_online_result(&mut wallet_testnet, false, Some(ESPLORA_URL));
        let details = "indexer is for a network different from the wallet's one";
        assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details ));
    }

    // bad online object
    let wrong_online = Online {
        id: 1,
        indexer_url: indexer_url.to_string(),
    };
    let result = wallet.check_online(wrong_online);
    assert!(matches!(result, Err(Error::CannotChangeOnline)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn consistency_check_fail_bitcoins() {
    initialize();

    // prepare test wallet with UTXOs + an asset
    let (mut wallet_orig, online_orig) = get_funded_wallet!();
    let wallet_data_orig = test_get_wallet_data(&wallet_orig);
    test_issue_asset_nia(&mut wallet_orig, &online_orig, None);

    // get wallet data
    let wallet_dir_orig = test_get_wallet_dir(&wallet_orig);
    let fingerprint = wallet_data_orig.master_fingerprint.clone();
    // prepare directories
    let data_dir_prefill_1 = get_test_data_dir_path().join("test_consistency.bitcoin.prefill_1");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.bitcoin.prefill_2");
    let data_dir_prefill_3 = get_test_data_dir_path().join("test_consistency.bitcoin.prefill_3");
    let wallet_dir_prefill = PathBuf::from(&data_dir_prefill_2).join(&fingerprint);
    let wallet_dir_prefill_2 = PathBuf::from(&data_dir_prefill_3).join(&fingerprint);
    for dir in [
        &data_dir_prefill_1,
        &data_dir_prefill_2,
        &wallet_dir_prefill,
        &wallet_dir_prefill_2,
    ] {
        if PathBuf::from(dir).is_dir() {
            fs::remove_dir_all(dir.clone()).unwrap();
        }
        fs::create_dir_all(dir).unwrap();
    }
    // prepare wallet data objects
    let wallet_data_empty = get_test_wallet_data(
        data_dir_prefill_1.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    let wallet_data_prefill = get_test_wallet_data(
        data_dir_prefill_2.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    let wallet_data_prefill_2 = get_test_wallet_data(
        data_dir_prefill_3.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    // copy original wallet's db data to prefilled wallet data dir
    let db_files: Vec<OsString> = fs::read_dir(&wallet_dir_orig)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .into_string()
                .unwrap()
                .starts_with(RGB_LIB_DB_NAME)
        })
        .map(|e| e.as_ref().unwrap().file_name())
        .collect();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_orig).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill).join(file);
        fs::copy(src, dst).unwrap();
    }

    // introduce asset inconsistency by spending UTXOs from other instance of the same wallet,
    // simulating a wallet used on multiple devices (which needs to be avoided to prevent asset
    // loss)
    let mut wallet_empty = Wallet::new(wallet_data_empty).unwrap();
    let online_empty = test_go_online(&mut wallet_empty, false, None);
    let request = wallet_empty.bdk_wallet.start_full_scan();
    let update = wallet_empty.indexer().full_scan(request).unwrap();
    wallet_empty.bdk_wallet.apply_update(update).unwrap();
    wallet_empty
        .bdk_wallet
        .persist(&mut wallet_empty.bdk_database)
        .unwrap();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();
    test_drain_to_destroy(
        &mut wallet_empty,
        &online_empty,
        &test_get_address(&mut rcv_wallet),
    );

    // detect asset inconsistency
    let err = "spent bitcoins with another wallet";
    let mut wallet_prefill = Wallet::new(wallet_data_prefill).unwrap();
    let result = test_go_online_result(&mut wallet_prefill, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e.contains(err)));

    // make sure detection works multiple times (doesn't get reset on first failed check)
    let mut wallet_prefill_2 = Wallet::new(wallet_data_prefill_2).unwrap();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_prefill).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill_2).join(file);
        fs::copy(src, dst).unwrap();
    }
    let result = test_go_online_result(&mut wallet_prefill_2, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e.contains(err)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn consistency_check_fail_utxos() {
    initialize();

    // prepare test wallet with UTXOs + an asset
    let (mut wallet_orig, online_orig) = get_funded_wallet!();
    let wallet_data_orig = test_get_wallet_data(&wallet_orig);
    test_issue_asset_nia(&mut wallet_orig, &online_orig, None);

    // get wallet data
    let wallet_dir_orig = test_get_wallet_dir(&wallet_orig);
    let fingerprint = wallet_data_orig.master_fingerprint.clone();
    // prepare directories
    let data_dir_prefill_1 = get_test_data_dir_path().join("test_consistency.utxos.prefill_1");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.utxos.prefill_2");
    let data_dir_prefill_3 = get_test_data_dir_path().join("test_consistency.utxos.prefill_3");
    let wallet_dir_prefill = PathBuf::from(&data_dir_prefill_2).join(&fingerprint);
    let wallet_dir_prefill_2 = PathBuf::from(&data_dir_prefill_3).join(&fingerprint);
    for dir in [
        &data_dir_prefill_1,
        &data_dir_prefill_2,
        &wallet_dir_prefill,
        &wallet_dir_prefill_2,
    ] {
        if PathBuf::from(dir).is_dir() {
            fs::remove_dir_all(dir.clone()).unwrap();
        }
        fs::create_dir_all(dir).unwrap();
    }
    // prepare wallet data objects
    let wallet_data_empty = get_test_wallet_data(
        data_dir_prefill_1.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    let wallet_data_prefill = get_test_wallet_data(
        data_dir_prefill_2.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    let wallet_data_prefill_2 = get_test_wallet_data(
        data_dir_prefill_3.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    // copy original wallet's db data to prefilled wallet data dir
    let db_files: Vec<OsString> = fs::read_dir(&wallet_dir_orig)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .into_string()
                .unwrap()
                .starts_with(RGB_LIB_DB_NAME)
        })
        .map(|e| e.as_ref().unwrap().file_name())
        .collect();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_orig).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill).join(file);
        fs::copy(src, dst).unwrap();
    }

    // introduce asset inconsistency by spending UTXOs from other instance of the same wallet,
    // simulating a wallet used on multiple devices (which needs to be avoided to prevent asset
    // loss)
    let mut wallet_empty = Wallet::new(wallet_data_empty).unwrap();
    let online_empty = test_go_online(&mut wallet_empty, false, None);
    let request = wallet_empty.bdk_wallet.start_full_scan();
    let update = wallet_empty.indexer().full_scan(request).unwrap();
    wallet_empty.bdk_wallet.apply_update(update).unwrap();
    wallet_empty
        .bdk_wallet
        .persist(&mut wallet_empty.bdk_database)
        .unwrap();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();
    test_drain_to_keep(
        &mut wallet_empty,
        &online_empty,
        &test_get_address(&mut rcv_wallet),
    );

    // detect asset inconsistency
    let err = "DB assets do not match with ones stored in RGB";
    let mut wallet_prefill = Wallet::new(wallet_data_prefill).unwrap();
    let result = test_go_online_result(&mut wallet_prefill, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e == err));

    // make sure detection works multiple times (doesn't get reset on first failed check)
    let mut wallet_prefill_2 = Wallet::new(wallet_data_prefill_2).unwrap();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_prefill).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill_2).join(file);
        fs::copy(src, dst).unwrap();
    }
    let result = test_go_online_result(&mut wallet_prefill_2, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e == err));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn consistency_check_fail_asset_ids() {
    initialize();

    // prepare test wallet with UTXOs + an asset
    let (mut wallet_orig, online_orig) = get_funded_wallet!();
    let wallet_data_orig = test_get_wallet_data(&wallet_orig);
    test_issue_asset_nia(&mut wallet_orig, &online_orig, None);

    // get wallet data
    let wallet_dir_orig = test_get_wallet_dir(&wallet_orig);
    let fingerprint = wallet_data_orig.master_fingerprint.clone();
    // prepare directories
    let data_dir_prefill_1 = get_test_data_dir_path().join("test_consistency.assets.prefill_1");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.assets.prefill_2");
    let data_dir_prefill_3 = get_test_data_dir_path().join("test_consistency.assets.prefill_3");
    let wallet_dir_prefill_1 = PathBuf::from(&data_dir_prefill_1).join(&fingerprint);
    let wallet_dir_prefill_2 = PathBuf::from(&data_dir_prefill_2).join(&fingerprint);
    let wallet_dir_prefill_3 = PathBuf::from(&data_dir_prefill_3).join(&fingerprint);
    for dir in [
        &data_dir_prefill_1,
        &data_dir_prefill_2,
        &data_dir_prefill_3,
    ] {
        if PathBuf::from(dir).is_dir() {
            fs::remove_dir_all(dir.clone()).unwrap();
        }
        fs::create_dir_all(dir).unwrap();
    }
    // prepare wallet data objects
    let wallet_data_prefill_1 = get_test_wallet_data(
        data_dir_prefill_1.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    let wallet_data_prefill_2 = get_test_wallet_data(
        data_dir_prefill_2.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    let wallet_data_prefill_3 = get_test_wallet_data(
        data_dir_prefill_3.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    // copy original wallet's data to prefilled wallets 1 + 2 data dir
    for destination in [&wallet_dir_prefill_1, &wallet_dir_prefill_2] {
        let result = copy_dir::copy_dir(&wallet_dir_orig, destination);
        assert!(result.unwrap().is_empty());
    }

    // check the first wallet copy works ok
    let mut wallet_prefill_1 = Wallet::new(wallet_data_prefill_1).unwrap();
    let result = test_go_online_result(&mut wallet_prefill_1, false, None);
    assert!(result.is_ok());

    // introduce asset id inconsistency by removing RGB data from wallet dir
    fs::remove_dir_all(wallet_dir_prefill_2.join(RGB_RUNTIME_DIR)).unwrap();

    // detect inconsistency
    let err = "DB assets do not match with ones stored in RGB";
    let mut wallet_prefill_2 = Wallet::new(wallet_data_prefill_2).unwrap();
    let result = test_go_online_result(&mut wallet_prefill_2, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e == err));

    // make sure detection works multiple times
    let result = copy_dir::copy_dir(wallet_dir_prefill_2, wallet_dir_prefill_3);
    assert!(result.unwrap().is_empty());
    let mut wallet_prefill_3 = Wallet::new(wallet_data_prefill_3).unwrap();
    let result = test_go_online_result(&mut wallet_prefill_3, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e == err));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn consistency_check_fail_media() {
    initialize();

    let file_str = "README.md";

    // prepare test wallet with UTXOs + an asset
    let (mut wallet_orig, online_orig) = get_funded_wallet!();
    let wallet_data_orig = test_get_wallet_data(&wallet_orig);
    test_issue_asset_cfa(
        &mut wallet_orig,
        &online_orig,
        None,
        Some(file_str.to_string()),
    );

    // get wallet data
    let wallet_dir_orig = test_get_wallet_dir(&wallet_orig);
    let fingerprint = wallet_data_orig.master_fingerprint.clone();
    // prepare directories
    let data_dir_prefill_1 = get_test_data_dir_path().join("test_consistency.media.prefill_1");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.media.prefill_2");
    let data_dir_prefill_3 = get_test_data_dir_path().join("test_consistency.media.prefill_3");
    let wallet_dir_prefill_1 = PathBuf::from(&data_dir_prefill_1).join(&fingerprint);
    let wallet_dir_prefill_2 = PathBuf::from(&data_dir_prefill_2).join(&fingerprint);
    let wallet_dir_prefill_3 = PathBuf::from(&data_dir_prefill_3).join(&fingerprint);
    for dir in [
        &data_dir_prefill_1,
        &data_dir_prefill_2,
        &data_dir_prefill_3,
    ] {
        if PathBuf::from(dir).is_dir() {
            fs::remove_dir_all(dir.clone()).unwrap();
        }
        fs::create_dir_all(dir).unwrap();
    }
    // prepare wallet data objects
    let wallet_data_prefill_1 = get_test_wallet_data(
        data_dir_prefill_1.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    let wallet_data_prefill_2 = get_test_wallet_data(
        data_dir_prefill_2.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    let wallet_data_prefill_3 = get_test_wallet_data(
        data_dir_prefill_3.to_str().unwrap(),
        &wallet_data_orig.account_xpub_colored,
        &wallet_data_orig.account_xpub_vanilla,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
        &wallet_data_orig.master_fingerprint,
    );
    // copy original wallet's data to prefilled wallets 1 + 2 data dir
    for destination in [&wallet_dir_prefill_1, &wallet_dir_prefill_2] {
        let result = copy_dir::copy_dir(&wallet_dir_orig, destination);
        assert!(result.unwrap().is_empty());
    }

    // check the first wallet copy works ok
    let mut wallet_prefill_1 = Wallet::new(wallet_data_prefill_1).unwrap();
    let result = test_go_online_result(&mut wallet_prefill_1, false, None);
    assert!(result.is_ok());

    // introduce media inconsistency by removing media dir
    fs::remove_dir_all(wallet_dir_prefill_2.join(MEDIA_DIR)).unwrap();

    // detect inconsistency
    let err = "DB media do not match with the ones stored in media directory";
    let mut wallet_prefill_2 = Wallet::new(wallet_data_prefill_2.clone()).unwrap();
    let result = test_go_online_result(&mut wallet_prefill_2, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e == err));

    // make sure detection works multiple times
    let result = copy_dir::copy_dir(wallet_dir_prefill_2, wallet_dir_prefill_3);
    assert!(result.unwrap().is_empty());
    let mut wallet_prefill_3 = Wallet::new(wallet_data_prefill_3).unwrap();
    let result = test_go_online_result(&mut wallet_prefill_3, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e == err));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn on_off_online() {
    initialize();

    // create wallet and go online
    let mut wallet = get_test_wallet(true, None);
    let wallet_data = wallet.wallet_data.clone();
    let online = test_go_online(&mut wallet, false, None);

    // go offline and close wallet
    drop(online);
    drop(wallet);

    // re-instantiate wallet and go back online
    let mut wallet = Wallet::new(wallet_data).unwrap();
    test_go_online(&mut wallet, false, None);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn offline() {
    initialize();

    // don't go online and manually craft the Online object
    let mut wallet = get_test_wallet(true, Some(MAX_ALLOCATIONS_PER_UTXO));
    let online = Online {
        id: 0,
        indexer_url: s!(""),
    };

    // the online check should report that the wallet is offline
    let result = test_create_utxos_begin_result(&mut wallet, &online, true, None, None, FEE_RATE);
    assert!(matches!(result, Err(Error::Offline)));
}
