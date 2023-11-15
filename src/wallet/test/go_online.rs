use super::*;
use serial_test::parallel;
use std::ffi::OsString;

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
}

#[test]
#[parallel]
fn fail() {
    initialize();

    let mut wallet = get_test_wallet(true, None);

    // cannot go online with a broken electrum URL
    let result = test_go_online_result(&mut wallet, false, Some("other:50001"));
    assert!(matches!(result, Err(Error::InvalidElectrum { details: _ })));

    // cannot go online again with broken electrum URL
    test_go_online(&mut wallet, false, None);
    let result = test_go_online_result(&mut wallet, false, Some("other:50001"));
    assert!(matches!(result, Err(Error::InvalidElectrum { details: _ })));

    // bad online object
    let wrong_online = Online {
        id: 1,
        electrum_url: wallet.online_data.as_ref().unwrap().electrum_url.clone(),
    };
    let result = wallet._check_online(wrong_online);
    assert!(matches!(result, Err(Error::CannotChangeOnline)));
}

#[test]
#[parallel]
fn consistency_check_fail_utxos() {
    initialize();

    // prepare test wallet with UTXOs + an asset
    let (mut wallet_orig, online_orig) = get_funded_wallet!();
    let wallet_data_orig = test_get_wallet_data(&wallet_orig);
    test_issue_asset_nia(&mut wallet_orig, &online_orig, None);

    // get wallet fingerprint
    let wallet_dir_orig = test_get_wallet_dir(&wallet_orig);
    let pubkey = ExtendedPubKey::from_str(&wallet_data_orig.pubkey).unwrap();
    let extended_key: ExtendedKey = ExtendedKey::from(pubkey);
    let bdk_network = BdkNetwork::from(BitcoinNetwork::Regtest);
    let xpub = extended_key.into_xpub(bdk_network, &Secp256k1::new());
    let fingerprint = xpub.fingerprint().to_string();
    // prepare directories
    let data_dir_empty = get_test_data_dir_path().join("test_consistency.empty");
    let data_dir_prefill = get_test_data_dir_path().join("test_consistency.prefill");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.prefill_2");
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
    let wallet_data_empty = get_test_wallet_data(
        data_dir_empty.to_str().unwrap(),
        &wallet_data_orig.pubkey,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
    );
    let wallet_data_prefill = get_test_wallet_data(
        data_dir_prefill.to_str().unwrap(),
        &wallet_data_orig.pubkey,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
    );
    let wallet_data_prefill_2 = get_test_wallet_data(
        data_dir_prefill_2.to_str().unwrap(),
        &wallet_data_orig.pubkey,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
    );
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
        let src = PathBuf::from(&wallet_dir_orig).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill).join(file);
        fs::copy(src, dst).unwrap();
    }

    // introduce asset inconsistency by spending UTXOs from other instance of the same wallet,
    // simulating a wallet used on multiple devices (which needs to be avoided to prevent asset
    // loss)
    let mut wallet_empty = Wallet::new(wallet_data_empty).unwrap();
    let online_empty = test_go_online(&mut wallet_empty, false, None);
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();
    test_drain_to_keep(&wallet_empty, &online_empty, &test_get_address(&rcv_wallet));

    // detect asset inconsistency
    let mut wallet_prefill = Wallet::new(wallet_data_prefill).unwrap();
    let result = test_go_online_result(&mut wallet_prefill, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: _ })));

    // make sure detection works multiple times (doesn't get reset on first failed check)
    let mut wallet_prefill_2 = Wallet::new(wallet_data_prefill_2).unwrap();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_prefill).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill_2).join(file);
        fs::copy(src, dst).unwrap();
    }
    let result = test_go_online_result(&mut wallet_prefill_2, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: _ })));
}

#[test]
#[parallel]
fn consistency_check_fail_asset_ids() {
    initialize();

    // prepare test wallet with UTXOs + an asset
    let (mut wallet_orig, online_orig) = get_funded_wallet!();
    let wallet_data_orig = test_get_wallet_data(&wallet_orig);
    let _asset = test_issue_asset_nia(&mut wallet_orig, &online_orig, None);

    // get wallet fingerprint
    let wallet_dir_orig = test_get_wallet_dir(&wallet_orig);
    let pubkey = ExtendedPubKey::from_str(&wallet_data_orig.pubkey).unwrap();
    let extended_key: ExtendedKey = ExtendedKey::from(pubkey);
    let bdk_network = BdkNetwork::from(BitcoinNetwork::Regtest);
    let xpub = extended_key.into_xpub(bdk_network, &Secp256k1::new());
    let fingerprint = xpub.fingerprint().to_string();
    // prepare directories
    let data_dir_prefill_1 = get_test_data_dir_path().join("test_consistency.prefill_1");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.prefill_2");
    let data_dir_prefill_3 = get_test_data_dir_path().join("test_consistency.prefill_3");
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
        &wallet_data_orig.pubkey,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
    );
    let wallet_data_prefill_2 = get_test_wallet_data(
        data_dir_prefill_2.to_str().unwrap(),
        &wallet_data_orig.pubkey,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
    );
    let wallet_data_prefill_3 = get_test_wallet_data(
        data_dir_prefill_3.to_str().unwrap(),
        &wallet_data_orig.pubkey,
        wallet_data_orig.mnemonic.as_ref().unwrap(),
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
    fs::remove_dir_all(wallet_dir_prefill_2.join("regtest")).unwrap();

    // detect inconsistency
    let mut wallet_prefill_2 = Wallet::new(wallet_data_prefill_2).unwrap();
    let result = test_go_online_result(&mut wallet_prefill_2, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: _ })));

    // make sure detection works multiple times
    let result = copy_dir::copy_dir(wallet_dir_prefill_2, wallet_dir_prefill_3);
    assert!(result.unwrap().is_empty());
    let mut wallet_prefill_3 = Wallet::new(wallet_data_prefill_3).unwrap();
    let result = test_go_online_result(&mut wallet_prefill_3, false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: _ })));
}

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
