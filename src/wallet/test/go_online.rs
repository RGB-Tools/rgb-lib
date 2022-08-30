use std::ffi::OsString;

use super::*;

#[test]
fn success() {
    initialize();

    let mut wallet = get_test_wallet(true);

    // go online
    let result_1 = wallet.go_online(ELECTRUM_URL.to_string(), false);
    assert!(result_1.is_ok());

    // can go online twice with the same electrum URL
    let result_2 = wallet.go_online(ELECTRUM_URL.to_string(), false);
    assert!(result_2.is_ok());
    assert_eq!(result_1.unwrap(), result_2.unwrap());
}

#[test]
fn fail() {
    initialize();

    let mut wallet = get_test_wallet(true);
    wallet.go_online(ELECTRUM_URL.to_string(), false).unwrap();

    // cannot go online twice with different electrum URLs
    let result = wallet.go_online("other:50001".to_string(), false);
    assert!(matches!(result, Err(Error::CannotChangeOnline())));

    // bad online object
    let (_wrong_wallet, wrong_online) = get_empty_wallet!();
    let result = wallet._check_online(wrong_online);
    assert!(matches!(result, Err(Error::InvalidOnline())));
}

#[test]
fn consistency_check_fail_utxos() {
    initialize();

    // prepare test wallet with utxos + an asset
    let (mut wallet_orig, online_orig) = get_funded_wallet!(true, true);
    let wallet_data_orig = wallet_orig.get_wallet_data();
    wallet_orig
        .issue_asset(
            online_orig.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            AMOUNT,
        )
        .unwrap();

    let bitcoin_network = BitcoinNetwork::Regtest;
    // get wallet fingerprint
    let wallet_dir_orig = wallet_orig.get_wallet_dir();
    let pubkey = ExtendedPubKey::from_str(&wallet_data_orig.pubkey).unwrap();
    let extended_key: ExtendedKey = ExtendedKey::from(pubkey);
    let bdk_network = BdkNetwork::from(bitcoin_network.clone());
    let xpub = extended_key.into_xpub(bdk_network, &Secp256k1::new());
    let fingerprint = xpub.fingerprint().to_string();
    // prepare directories
    let data_dir_empty = PathBuf::from(TEST_DATA_DIR).join("test_consistency.empty");
    let data_dir_prefill = PathBuf::from(TEST_DATA_DIR).join("test_consistency.prefill");
    let data_dir_prefill_2 = PathBuf::from(TEST_DATA_DIR).join("test_consistency.prefill_2");
    let wallet_dir_prefill = PathBuf::from(&data_dir_prefill).join(&fingerprint);
    let wallet_dir_prefill_2 = PathBuf::from(&data_dir_prefill_2).join(&fingerprint);
    for dir in [
        &data_dir_empty,
        &data_dir_prefill,
        &wallet_dir_prefill,
        &wallet_dir_prefill_2,
    ] {
        if PathBuf::from(dir).is_dir() {
            fs::remove_dir_all(dir.clone()).unwrap();
        }
        fs::create_dir_all(dir).unwrap();
    }
    // prepare wallet data objects
    let wallet_data_empty = WalletData {
        data_dir: data_dir_empty
            .clone()
            .into_os_string()
            .into_string()
            .unwrap(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        pubkey: wallet_data_orig.pubkey.clone(),
        mnemonic: wallet_data_orig.mnemonic.clone(),
    };
    let wallet_data_prefill = WalletData {
        data_dir: data_dir_prefill
            .clone()
            .into_os_string()
            .into_string()
            .unwrap(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        pubkey: wallet_data_orig.pubkey.clone(),
        mnemonic: wallet_data_orig.mnemonic.clone(),
    };
    let wallet_data_prefill_2 = WalletData {
        data_dir: data_dir_prefill_2
            .clone()
            .into_os_string()
            .into_string()
            .unwrap(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        pubkey: wallet_data_orig.pubkey.clone(),
        mnemonic: wallet_data_orig.mnemonic.clone(),
    };
    // copy original wallet's db data to prefilled wallet data dir
    let wallet_dir_entries = fs::read_dir(&wallet_dir_orig).unwrap();
    let db_files: Vec<OsString> = wallet_dir_entries
        .into_iter()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .into_string()
                .unwrap()
                .starts_with("rgb_db")
        })
        .map(|e| e.as_ref().unwrap().file_name())
        .collect();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_orig).join(&file);
        let dst = PathBuf::from(&wallet_dir_prefill).join(&file);
        fs::copy(&src, &dst).unwrap();
    }

    // introduce asset inconsistency by spending utxos from other instance of the same wallet,
    // simulating a wallet used on multiple devices (which needs to be avoided to prevent asset
    // loss)
    let mut wallet_empty = Wallet::new(wallet_data_empty).unwrap();
    let online_empty = wallet_empty
        .go_online(ELECTRUM_URL.to_string(), false)
        .unwrap();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();
    wallet_empty
        .drain_to(online_empty.clone(), rcv_wallet.get_address(), false)
        .unwrap();

    // detect asset inconcistency
    let mut wallet_prefill = Wallet::new(wallet_data_prefill).unwrap();
    let result = wallet_prefill.go_online(ELECTRUM_URL.to_string(), false);
    assert!(matches!(result, Err(Error::Inconsistency(_))));

    // make sure detection works multiple times (doesn't get reset on first failed check)
    let mut wallet_prefill_2 = Wallet::new(wallet_data_prefill_2).unwrap();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_prefill).join(&file);
        let dst = PathBuf::from(&wallet_dir_prefill_2).join(&file);
        fs::copy(&src, &dst).unwrap();
    }
    let result = wallet_prefill_2.go_online(ELECTRUM_URL.to_string(), false);
    assert!(matches!(result, Err(Error::Inconsistency(_))));
}

#[test]
fn consistency_check_fail_asset_ids() {
    initialize();

    // prepare test wallet with utxos + an asset
    let (mut wallet_orig, online_orig) = get_funded_wallet!();
    let wallet_data_orig = wallet_orig.get_wallet_data();
    let _asset = wallet_orig
        .issue_asset(
            online_orig.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            AMOUNT,
        )
        .unwrap();

    let bitcoin_network = BitcoinNetwork::Regtest;
    // get wallet fingerprint
    let wallet_dir_orig = wallet_orig.get_wallet_dir();
    let pubkey = ExtendedPubKey::from_str(&wallet_data_orig.pubkey).unwrap();
    let extended_key: ExtendedKey = ExtendedKey::from(pubkey);
    let bdk_network = BdkNetwork::from(bitcoin_network.clone());
    let xpub = extended_key.into_xpub(bdk_network, &Secp256k1::new());
    let fingerprint = xpub.fingerprint().to_string();
    // prepare directories
    let data_dir_prefill_1 = PathBuf::from(TEST_DATA_DIR).join("test_consistency.prefill_1");
    let data_dir_prefill_2 = PathBuf::from(TEST_DATA_DIR).join("test_consistency.prefill_2");
    let data_dir_prefill_3 = PathBuf::from(TEST_DATA_DIR).join("test_consistency.prefill_3");
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
    let wallet_data_prefill_1 = WalletData {
        data_dir: data_dir_prefill_1.to_str().unwrap().to_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        pubkey: wallet_data_orig.pubkey.clone(),
        mnemonic: wallet_data_orig.mnemonic.clone(),
    };
    let wallet_data_prefill_2 = WalletData {
        data_dir: data_dir_prefill_2.to_str().unwrap().to_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        pubkey: wallet_data_orig.pubkey.clone(),
        mnemonic: wallet_data_orig.mnemonic.clone(),
    };
    let wallet_data_prefill_3 = WalletData {
        data_dir: data_dir_prefill_3.to_str().unwrap().to_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        pubkey: wallet_data_orig.pubkey.clone(),
        mnemonic: wallet_data_orig.mnemonic.clone(),
    };
    // copy original wallet's data to prefilled wallets 1 + 2 data dir
    for destination in [&wallet_dir_prefill_1, &wallet_dir_prefill_2] {
        let result = Command::new("cp")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .arg("-r")
            .arg(&wallet_dir_orig)
            .arg(&destination)
            .status();
        assert!(result.is_ok());
    }

    // check the first wallet copy works ok
    let mut wallet_prefill_1 = Wallet::new(wallet_data_prefill_1).unwrap();
    let result = wallet_prefill_1.go_online(ELECTRUM_URL.to_string(), false);
    assert!(result.is_ok());

    // introduce asset id inconsistency by removing rgb data from wallet dir
    fs::remove_dir_all(wallet_dir_prefill_2.join("sled.db")).unwrap();

    // detect inconsistency
    let mut wallet_prefill_2 = Wallet::new(wallet_data_prefill_2).unwrap();
    let result = wallet_prefill_2.go_online(ELECTRUM_URL.to_string(), false);
    assert!(matches!(result, Err(Error::Inconsistency(_))));

    // make sure detection works multiple times
    let result = Command::new("cp")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("-r")
        .arg(&wallet_dir_prefill_2)
        .arg(&wallet_dir_prefill_3)
        .status();
    assert!(result.is_ok());
    let mut wallet_prefill_3 = Wallet::new(wallet_data_prefill_3).unwrap();
    let result = wallet_prefill_3.go_online(ELECTRUM_URL.to_string(), false);
    assert!(matches!(result, Err(Error::Inconsistency(_))));
}
