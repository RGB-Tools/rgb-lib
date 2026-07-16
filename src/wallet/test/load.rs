use super::*;

use crate::keys::restore_keys;

#[test]
#[parallel]
fn success() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Signet, WitnessVersion::Taproot);
    let wallet_data = WalletData {
        data_dir: test_data_dir_str.clone(),
        bitcoin_network: BitcoinNetwork::Signet,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: 1,
        supported_schemas: vec![AssetSchema::Nia, AssetSchema::Cfa],
    };
    let wallet_keys = SinglesigKeys::from_keys(&keys, Some(2));
    let wallet = Wallet::new(wallet_data.clone(), wallet_keys.clone()).unwrap();
    let wallet_dir = wallet.get_wallet_dir();
    let descriptors = wallet.get_descriptors();
    drop(wallet);

    // load the wallet back with no settings other than where it lives and who it is
    let loaded = Wallet::load(
        &test_data_dir_str,
        &keys.master_fingerprint,
        Some(keys.mnemonic.clone()),
    )
    .unwrap();

    assert_eq!(loaded.get_wallet_dir(), wallet_dir);
    assert_eq!(loaded.get_descriptors(), descriptors);

    let loaded_data = loaded.get_wallet_data();
    assert_eq!(loaded_data.data_dir, wallet_data.data_dir);
    assert_eq!(loaded_data.bitcoin_network, wallet_data.bitcoin_network);
    assert!(matches!(loaded_data.database_type, DatabaseType::Sqlite));
    assert_eq!(
        loaded_data.max_allocations_per_utxo,
        wallet_data.max_allocations_per_utxo
    );
    assert_eq!(loaded_data.supported_schemas, wallet_data.supported_schemas);

    let loaded_keys = loaded.get_keys();
    assert_eq!(loaded_keys.account_xpub_vanilla, keys.account_xpub_vanilla);
    assert_eq!(loaded_keys.account_xpub_colored, keys.account_xpub_colored);
    assert_eq!(loaded_keys.master_fingerprint, keys.master_fingerprint);
    assert_eq!(loaded_keys.vanilla_keychain, wallet_keys.vanilla_keychain);
    assert_eq!(loaded_keys.mnemonic, wallet_keys.mnemonic);
    assert_eq!(loaded_keys.witness_version, keys.witness_version);
}

#[test]
#[parallel]
fn watch_only_success() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::SegWitV0);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    drop(wallet);

    // omitting the mnemonic loads the wallet in watch-only mode
    let loaded = Wallet::load(&test_data_dir_str, &keys.master_fingerprint, None).unwrap();

    let loaded_keys = loaded.get_keys();
    assert!(loaded_keys.mnemonic.is_none());
    assert_eq!(loaded_keys.master_fingerprint, keys.master_fingerprint);
    // the witness version the wallet was created with survives the round-trip, so the loaded
    // wallet doesn't silently fall back to the default and derive a different set of descriptors
    assert_eq!(loaded_keys.witness_version, keys.witness_version);
}

#[test]
#[parallel]
fn new_updates_manifest_success() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet_data = WalletData {
        data_dir: test_data_dir_str.clone(),
        bitcoin_network: BitcoinNetwork::Regtest,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: 1,
        supported_schemas: vec![AssetSchema::Nia],
    };
    let wallet = Wallet::new(wallet_data.clone(), SinglesigKeys::from_keys(&keys, None)).unwrap();
    drop(wallet);

    let loaded = Wallet::load(&test_data_dir_str, &keys.master_fingerprint, None).unwrap();
    let loaded_data = loaded.get_wallet_data();
    assert_eq!(
        loaded_data.max_allocations_per_utxo,
        wallet_data.max_allocations_per_utxo
    );
    assert_eq!(loaded_data.supported_schemas, wallet_data.supported_schemas);
    drop(loaded);

    // new is the way to change the settings that aren't fixed at creation, so it rewrites the
    // manifest and later loads pick the new values up
    let updated_wallet_data = WalletData {
        max_allocations_per_utxo: 3,
        supported_schemas: vec![AssetSchema::Nia, AssetSchema::Cfa],
        ..wallet_data
    };
    let wallet = Wallet::new(
        updated_wallet_data.clone(),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    drop(wallet);

    let loaded = Wallet::load(&test_data_dir_str, &keys.master_fingerprint, None).unwrap();
    let loaded_data = loaded.get_wallet_data();
    assert_eq!(
        loaded_data.max_allocations_per_utxo,
        updated_wallet_data.max_allocations_per_utxo
    );
    assert_eq!(
        loaded_data.supported_schemas,
        updated_wallet_data.supported_schemas
    );
}

#[test]
#[parallel]
fn watch_only_toggle_success() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    drop(wallet);

    // the mnemonic is the one setting that is meant to come and go across opens, so it's neither
    // recorded in the manifest nor checked against it: signing and watch-only opens keep separate
    // BDK stores and address the same scripts
    let watch_only = Wallet::load(&test_data_dir_str, &keys.master_fingerprint, None).unwrap();
    assert!(watch_only.get_keys().mnemonic.is_none());
    drop(watch_only);

    let signing = Wallet::load(
        &test_data_dir_str,
        &keys.master_fingerprint,
        Some(keys.mnemonic.clone()),
    )
    .unwrap();
    assert_eq!(signing.get_keys().mnemonic, Some(keys.mnemonic.clone()));
    drop(signing);

    // and the same toggle works on a wallet that was only ever opened watch-only
    let keys_2 = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys_no_mnemonic(&keys_2, None),
    )
    .unwrap();
    drop(wallet);

    let signing = Wallet::load(
        &test_data_dir_str,
        &keys_2.master_fingerprint,
        Some(keys_2.mnemonic.clone()),
    )
    .unwrap();
    assert_eq!(signing.get_keys().mnemonic, Some(keys_2.mnemonic));
}

#[test]
#[parallel]
fn new_immutable_settings_fail() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet_data = get_test_wallet_data(&test_data_dir_str);
    let wallet_keys = SinglesigKeys::from_keys(&keys, None);
    let wallet = Wallet::new(wallet_data.clone(), wallet_keys.clone()).unwrap();
    drop(wallet);

    let new_with = |wallet_data: WalletData, keys: SinglesigKeys| -> Error {
        Wallet::new(wallet_data, keys).err().unwrap()
    };

    // same mnemonic, different witness version: derives a different set of descriptors, so it
    // would address a different wallet from the same directory
    let segwit_keys = restore_keys(
        BitcoinNetwork::Regtest,
        keys.mnemonic.clone(),
        WitnessVersion::SegWitV0,
    )
    .unwrap();
    let err = new_with(
        wallet_data.clone(),
        SinglesigKeys::from_keys(&segwit_keys, None),
    );
    assert_matches!(err, Error::WalletSettingMismatch { setting, .. } if setting == "witness_version");

    // the master fingerprint is the directory name and is the same on every network, so nothing
    // else would stop the same directory being reused for a different chain. the network keeps
    // the error BDK's genesis check has always raised for this
    let other_network_wallet_data = WalletData {
        bitcoin_network: BitcoinNetwork::Testnet,
        ..wallet_data.clone()
    };
    let err = new_with(other_network_wallet_data, wallet_keys.clone());
    assert_matches!(err, Error::BitcoinNetworkMismatch);

    // the vanilla keychain index feeds the vanilla descriptor
    let err = new_with(
        wallet_data.clone(),
        SinglesigKeys::from_keys(&keys, Some(2)),
    );
    assert_matches!(err, Error::WalletSettingMismatch { setting, .. } if setting == "vanilla_keychain");

    // a rejected new leaves the recorded settings untouched
    let loaded = Wallet::load(
        &test_data_dir_str,
        &keys.master_fingerprint,
        Some(keys.mnemonic.clone()),
    )
    .unwrap();
    assert_eq!(
        loaded.get_keys().witness_version,
        wallet_keys.witness_version
    );
    // the manifest records the resolved keychain, so a wallet created with None loads back as
    // Some(KEYCHAIN_BTC), the one from the original new() call (same keychain, but explicit)
    assert_eq!(loaded.get_keys().vanilla_keychain, Some(KEYCHAIN_BTC));
    assert_eq!(
        loaded.get_wallet_data().bitcoin_network,
        wallet_data.bitcoin_network
    );

    // but None and Some(KEYCHAIN_BTC) mean the same keychain, so spelling it either way is not a
    // change and must not be rejected
    Wallet::new(
        wallet_data.clone(),
        SinglesigKeys::from_keys(&keys, Some(KEYCHAIN_BTC)),
    )
    .unwrap();
    Wallet::new(wallet_data.clone(), wallet_keys.clone()).unwrap();
}

#[test]
#[parallel]
fn new_immutable_settings_watch_only_fail() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    drop(wallet);

    // watch-only opens use a separate BDK store, so the first watch-only open of a signing wallet
    // finds an empty one and has no persisted descriptor to be checked against
    let other_keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let mut wrong_xpub_keys = SinglesigKeys::from_keys_no_mnemonic(&keys, None);
    wrong_xpub_keys.account_xpub_colored = other_keys.account_xpub_colored.clone();
    let err = Wallet::new(get_test_wallet_data(&test_data_dir_str), wrong_xpub_keys)
        .err()
        .unwrap();
    assert_matches!(err, Error::WalletSettingMismatch { setting, .. } if setting == "account_xpub_colored");
}

#[test]
#[parallel]
fn new_mutable_settings_success() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    drop(wallet);

    // the guard doesn't get in the way of the settings new is meant to update, nor of the
    // watch-only toggle or moving the wallet directory
    let mut wallet_data = get_test_wallet_data(&test_data_dir_str);
    wallet_data.max_allocations_per_utxo = MAX_ALLOCATIONS_PER_UTXO + 1;
    wallet_data.supported_schemas = vec![AssetSchema::Nia];
    let wallet = Wallet::new(
        wallet_data.clone(),
        SinglesigKeys::from_keys_no_mnemonic(&keys, None),
    )
    .unwrap();
    assert!(wallet.get_keys().mnemonic.is_none());
    drop(wallet);

    let loaded = Wallet::load(&test_data_dir_str, &keys.master_fingerprint, None).unwrap();
    assert_eq!(
        loaded.get_wallet_data().max_allocations_per_utxo,
        wallet_data.max_allocations_per_utxo
    );
    assert_eq!(
        loaded.get_wallet_data().supported_schemas,
        wallet_data.supported_schemas
    );
}

#[test]
#[parallel]
fn restored_backup_success() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let descriptors = wallet.get_descriptors();

    let backup_file_path = get_test_data_dir_path().join("test_load_backup.rgb-lib_backup");
    let backup_file = backup_file_path.to_str().unwrap();
    let _ = fs::remove_file(backup_file);
    wallet.backup(backup_file, PASSWORD).unwrap();
    drop(wallet);

    let target_dir_path = get_restore_dir_path(Some("load"));
    let target_dir = target_dir_path.to_str().unwrap();
    let _ = fs::remove_dir_all(target_dir);
    restore_backup(backup_file, PASSWORD, target_dir).unwrap();

    // the manifest lives in the wallet directory, so it rides along in the backup and the wallet
    // can be loaded from where it was restored, not just from where it was created
    let loaded = Wallet::load(target_dir, &keys.master_fingerprint, Some(keys.mnemonic)).unwrap();
    assert_eq!(loaded.get_descriptors(), descriptors);
    assert_eq!(loaded.get_wallet_data().data_dir, target_dir);
}

#[test]
#[parallel]
fn inexistent_data_dir_fail() {
    let err = Wallet::load("/inexistent/data/dir", "deadbeef", None)
        .err()
        .unwrap();
    assert_matches!(err, Error::InexistentDataDir);
}

#[test]
#[parallel]
fn inexistent_manifest_fail() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let wallet_dir = wallet.get_wallet_dir();
    drop(wallet);

    // a wallet directory from before manifest support looks like one with the manifest removed
    fs::remove_file(wallet_dir.join(WALLET_MANIFEST_FILE)).unwrap();

    let err = Wallet::load(&test_data_dir_str, &keys.master_fingerprint, None)
        .err()
        .unwrap();
    assert_matches!(err, Error::InexistentWalletManifest { .. });

    // calling new once writes the manifest, after which load works
    Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    Wallet::load(&test_data_dir_str, &keys.master_fingerprint, None).unwrap();
}

#[test]
#[parallel]
fn unsupported_manifest_version_fail() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let manifest_path = wallet.get_wallet_dir().join(WALLET_MANIFEST_FILE);
    drop(wallet);

    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["version"] = Value::from(u8::MAX);
    fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

    let err = Wallet::load(&test_data_dir_str, &keys.master_fingerprint, None)
        .err()
        .unwrap();
    assert_matches!(
        err,
        Error::UnsupportedWalletManifestVersion { version } if version == u8::MAX.to_string()
    );
}

#[test]
#[parallel]
fn wrong_mnemonic_fail() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    drop(wallet);

    // a mnemonic that doesn't derive the manifest's xpubs is caught before it can be used
    let other_keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let err = Wallet::load(
        &test_data_dir_str,
        &keys.master_fingerprint,
        Some(other_keys.mnemonic),
    )
    .err()
    .unwrap();
    assert_matches!(err, Error::InvalidBitcoinKeys);
}

#[test]
#[parallel]
fn manifest_fingerprint_mismatch_fail() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    let keys_1 = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet_1 = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys_1, None),
    )
    .unwrap();
    let manifest_1 = wallet_1.get_wallet_dir().join(WALLET_MANIFEST_FILE);

    let keys_2 = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet_2 = Wallet::new(
        get_test_wallet_data(&test_data_dir_str),
        SinglesigKeys::from_keys(&keys_2, None),
    )
    .unwrap();
    let manifest_2 = wallet_2.get_wallet_dir().join(WALLET_MANIFEST_FILE);

    drop(wallet_1);
    drop(wallet_2);

    // a manifest that doesn't belong to the directory holding it would otherwise have `new` set up
    // a second, unrelated wallet at the fingerprint the manifest names
    fs::copy(manifest_1, &manifest_2).unwrap();

    let err = Wallet::load(&test_data_dir_str, &keys_2.master_fingerprint, None)
        .err()
        .unwrap();
    assert_matches!(err, Error::FingerprintMismatch);
}
