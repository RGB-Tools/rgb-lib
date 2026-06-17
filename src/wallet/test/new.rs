use super::*;

use std::os::unix::fs::PermissionsExt;

fn check_wallet(
    party: &impl OfflineSigParty<W = Wallet>,
    network: BitcoinNetwork,
    keychain_vanilla: Option<u8>,
) {
    let keychains: Vec<_> = party.wlt().bdk_wallet().keychains().collect();
    assert_eq!(keychains.len(), 2);
    for (keychain_kind, extended_descriptor) in keychains {
        match keychain_kind {
            KeychainKind::External => match extended_descriptor {
                ExtendedDescriptor::Tr(tr) => {
                    let full_derivation_path = tr
                        .internal_key()
                        .full_derivation_path()
                        .unwrap()
                        .to_string();
                    let coin_type = get_coin_type(&network, true);
                    let account_derivation_children =
                        get_account_derivation_children(WitnessVersion::Taproot, coin_type);
                    let expected_full_derivation_path =
                        get_extended_derivation_path(account_derivation_children, KEYCHAIN_RGB);
                    assert_eq!(
                        full_derivation_path,
                        expected_full_derivation_path.to_string()
                    );
                }
                _ => panic!("wrong descriptor type"),
            },
            KeychainKind::Internal => match extended_descriptor {
                ExtendedDescriptor::Tr(tr) => {
                    let full_derivation_path = tr
                        .internal_key()
                        .full_derivation_path()
                        .unwrap()
                        .to_string();
                    let coin_type = get_coin_type(&network, false);
                    let account_derivation_children =
                        get_account_derivation_children(WitnessVersion::Taproot, coin_type);
                    let keychain_vanilla = keychain_vanilla.unwrap_or(KEYCHAIN_BTC);
                    let expected_full_derivation_path =
                        get_extended_derivation_path(account_derivation_children, keychain_vanilla);
                    assert_eq!(
                        full_derivation_path,
                        expected_full_derivation_path.to_string()
                    );
                }
                _ => panic!("wrong descriptor type"),
            },
        }
    }
    assert_eq!(party.get_wallet_data().bitcoin_network, network);
}

#[test]
#[parallel]
fn success() {
    create_test_data_dir();

    // with private keys
    let party = offline_party!(get_test_wallet(true, None));
    let bak_info_after = party.db_backup_info_opt();
    assert!(bak_info_after.is_none());

    // without private keys
    let party = offline_party!(get_test_wallet(false, None));
    check_wallet(&party, BitcoinNetwork::Regtest, None);

    // with custom vanilla keychain
    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);
    let vanilla_keychain = Some(u8::MAX);
    let party = offline_party!(
        Wallet::new(
            WalletData {
                data_dir: get_test_data_dir_string(),
                bitcoin_network,
                database_type: DatabaseType::Sqlite,
                max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
                supported_schemas: AssetSchema::VALUES.to_vec(),
            },
            SinglesigKeys::from_keys(&keys, vanilla_keychain),
        )
        .unwrap()
    );
    check_wallet(&party, bitcoin_network, vanilla_keychain);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn signet_success() {
    create_test_data_dir();

    let bitcoin_network = BitcoinNetwork::Signet;
    let mut party = offline_party!(get_test_wallet_with_net(true, None, bitcoin_network));
    check_wallet(&party, bitcoin_network, None);
    let indexer_url = "ssl://electrum.iriswallet.com:50033";
    party.go_online(false, Some(indexer_url));
    assert!(!party.wallet.watch_only());
    assert_eq!(party.get_wallet_data().bitcoin_network, bitcoin_network);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn testnet_success() {
    create_test_data_dir();

    let bitcoin_network = BitcoinNetwork::Testnet;
    let mut party = offline_party!(get_test_wallet_with_net(true, None, bitcoin_network));
    check_wallet(&party, bitcoin_network, None);
    let indexer_url = "ssl://electrum.iriswallet.com:50013";
    party.go_online(false, Some(indexer_url));
    assert!(!party.wallet.watch_only());
    assert_eq!(party.get_wallet_data().bitcoin_network, bitcoin_network);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn testnet4_success() {
    create_test_data_dir();

    let bitcoin_network = BitcoinNetwork::Testnet4;
    let mut party = offline_party!(get_test_wallet_with_net(true, None, bitcoin_network));
    check_wallet(&party, bitcoin_network, None);
    let indexer_url = "ssl://electrum.iriswallet.com:50053";
    party.go_online(false, Some(indexer_url));
    assert!(!party.wallet.watch_only());
    assert_eq!(party.get_wallet_data().bitcoin_network, bitcoin_network);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn mainnet_success_electrum() {
    create_test_data_dir();

    let bitcoin_network = BitcoinNetwork::Mainnet;
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);
    let mut party = offline_party!(
        Wallet::new(
            WalletData {
                data_dir: get_test_data_dir_string(),
                bitcoin_network,
                database_type: DatabaseType::Sqlite,
                max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
                // IFA not supported on mainnet
                supported_schemas: vec![AssetSchema::Cfa, AssetSchema::Nia, AssetSchema::Uda],
            },
            SinglesigKeys::from_keys(&keys, None),
        )
        .unwrap()
    );

    check_wallet(&party, bitcoin_network, None);
    let indexer_url = "ssl://electrum.iriswallet.com:50003";
    party.go_online(false, Some(indexer_url));
    assert!(!party.wallet.watch_only());
    assert_eq!(party.get_wallet_data().bitcoin_network, bitcoin_network);
}

#[cfg(feature = "esplora")]
#[test]
#[ignore = "frequently fails due to timeout"]
#[parallel]
fn mainnet_success_esplora() {
    create_test_data_dir();

    let bitcoin_network = BitcoinNetwork::Mainnet;
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);
    let mut party = offline_party!(
        Wallet::new(
            WalletData {
                data_dir: get_test_data_dir_string(),
                bitcoin_network,
                database_type: DatabaseType::Sqlite,
                max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
                // IFA not supported on mainnet
                supported_schemas: vec![AssetSchema::Cfa, AssetSchema::Nia, AssetSchema::Uda],
            },
            SinglesigKeys::from_keys(&keys, None),
        )
        .unwrap()
    );

    check_wallet(&party, bitcoin_network, None);
    let indexer_url = "https://blockstream.info/api";
    party.go_online(false, Some(indexer_url));
    assert!(!party.wallet.watch_only());
    assert_eq!(party.get_wallet_data().bitcoin_network, bitcoin_network);
}

#[test]
#[parallel]
fn fail() {
    let wallet = get_test_wallet(true, None);
    let wallet_data = wallet.get_wallet_data();
    let keys = wallet.get_keys();

    // inexistent data dir
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.data_dir = s!("");
    let result = Wallet::new(wallet_data_bad, keys.clone());
    assert!(matches!(result, Err(Error::InexistentDataDir)));

    // 0 max allocations per UTXO
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.max_allocations_per_utxo = 0;
    let result = Wallet::new(wallet_data_bad, keys.clone());
    assert!(matches!(result, Err(Error::NoMaxAllocationsPerUtxo)));

    // pubkey too short
    let mut keys_bad = keys.clone();
    keys_bad.account_xpub_colored = s!("");
    let result = Wallet::new(wallet_data.clone(), keys_bad);
    assert!(matches!(result, Err(Error::InvalidPubkey { details: _ })));

    // bad byte in pubkey
    let mut keys_bad = keys.clone();
    keys_bad.account_xpub_colored = s!("l1iI0");
    let result = Wallet::new(wallet_data.clone(), keys_bad);
    assert!(matches!(result, Err(Error::InvalidPubkey { details: _ })));

    drop(wallet);

    // bad mnemonic word count
    let mut keys_bad = keys.clone();
    keys_bad.mnemonic = Some(s!(""));
    let result = Wallet::new(wallet_data.clone(), keys_bad);
    assert!(matches!(result, Err(Error::InvalidMnemonic { details: _ })));

    // invalid bitcoin keys
    let mut keys_bad = keys.clone();
    let alt_keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    keys_bad.account_xpub_colored = alt_keys.xpub;
    let result = Wallet::new(wallet_data.clone(), keys_bad);
    assert!(matches!(result, Err(Error::InvalidBitcoinKeys)));

    // bitcoin network mismatch
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.bitcoin_network = BitcoinNetwork::Testnet;
    let result = Wallet::new(wallet_data_bad, keys.clone());
    assert!(matches!(result, Err(Error::BitcoinNetworkMismatch)));

    // invalid fingerprint
    let mut keys_bad = keys.clone();
    keys_bad.master_fingerprint = s!("invalid");
    let result = Wallet::new(wallet_data.clone(), keys_bad);
    assert!(matches!(result, Err(Error::InvalidFingerprint)));

    // fingerprint mismatch
    let mut keys_bad = keys.clone();
    keys_bad.master_fingerprint = s!("badbadff");
    let result = Wallet::new(wallet_data.clone(), keys_bad);
    assert!(matches!(result, Err(Error::FingerprintMismatch)));

    // non-writable wallet dir
    let non_writable_path = "non_writable";
    let _ = fs::remove_dir(non_writable_path);
    fs::create_dir(non_writable_path).unwrap();
    // set the permissions to read and execute only (no write permissions)
    let permissions = fs::Permissions::from_mode(0o555);
    fs::set_permissions(non_writable_path, permissions).unwrap();
    // try to load runtime, expecting it to receive a permission denied error
    let result = load_rgb_runtime(PathBuf::from_str(non_writable_path).unwrap());
    let err = result.err().unwrap();
    assert!(matches!(err, Error::IO { details: m } if m.starts_with("Permission denied")));
    // try to setup logger, expecting it to receive a permission denied error
    let result = std::panic::catch_unwind(|| setup_logger(non_writable_path, Some("log")));
    assert!(result.is_ok());
    let res = result.unwrap();
    assert!(res.is_err());
    let err = res.err().unwrap();
    assert!(matches!(err, Error::IO { details: m } if m.starts_with("Permission denied")));
    // remove non writable directory
    fs::remove_dir(non_writable_path).unwrap();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn re_instantiate_wallet() {
    initialize();

    let amount: u64 = 66;

    // create wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let wallet_data = party.get_wallet_data().clone();
    let keys = party.get_keys();

    // issue
    let asset = party.issue_asset_nia(None);

    // send
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    // drop wallet
    drop(party);

    // re-instantiate wallet
    let mut wallet = Wallet::new(wallet_data.clone(), keys.clone()).unwrap();
    let online = wallet.go_online(test_go_online_options(None)).unwrap();
    let mut party = party!(wallet, online);

    // check wallet asset
    party.check_test_wallet_data(&asset, None, 1, amount);

    // drop wallet
    drop(party);

    // re-instantiate wallet in watch only mode
    let mut keys_bad = keys.clone();
    keys_bad.mnemonic = None;
    let mut wallet = Wallet::new(wallet_data.clone(), keys_bad).unwrap();
    let _online = wallet.go_online(test_go_online_options(None)).unwrap();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn watch_only_success() {
    initialize();

    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);

    // watch-only wallet
    let mut wallet_watch = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: AssetSchema::VALUES.to_vec(),
        },
        SinglesigKeys::from_keys_no_mnemonic(&keys, None),
    )
    .unwrap();
    let online_watch = wallet_watch
        .go_online(test_go_online_options(None))
        .unwrap();
    let mut party_watch = party!(wallet_watch, online_watch);

    // signer wallet
    let mut wallet_sign = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: AssetSchema::VALUES.to_vec(),
        },
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();

    // check generated addresses are the same
    let address_watch = party_watch.get_address();
    let address_signer = wallet_sign.get_address().unwrap();
    assert_eq!(address_watch, address_signer);

    // fund wallet
    fund_wallet(address_watch);
    mine(false);
    let unspents = party_watch.list_unspents_with_sync(false);
    assert_eq!(unspents.len(), 1);

    // create UTXOs
    let unsigned_psbt = party_watch
        .create_utxos_begin_result(false, None, None, FEE_RATE)
        .unwrap();
    let signed_psbt = wallet_sign.sign_psbt(unsigned_psbt, None).unwrap();
    party_watch
        .wallet
        .create_utxos_end(online_watch, signed_psbt)
        .unwrap();
    let unspents = party_watch.list_unspents_with_sync(false);
    assert_eq!(unspents.len(), UTXO_NUM as usize + 1);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn watch_only_fail() {
    initialize();

    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);

    // watch-only wallet invalid fingerprint
    let mut keys_bad = keys.clone();
    keys_bad.master_fingerprint = s!("invalid");
    let result = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: AssetSchema::VALUES.to_vec(),
        },
        SinglesigKeys::from_keys_no_mnemonic(&keys_bad, None),
    );
    assert!(matches!(result, Err(Error::InvalidFingerprint)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn get_account_xpub_success() {
    // wallet
    let wallet = get_test_wallet(true, None);
    let mnemonic = wallet.get_keys().mnemonic.clone().unwrap();

    // get colored account xpub
    let (_, account_xpub, _) = get_account_data(
        &BitcoinNetwork::Regtest,
        &mnemonic,
        true,
        WitnessVersion::Taproot,
    )
    .unwrap();
    assert_eq!(account_xpub.network, NetworkKind::Test,);
    assert_eq!(account_xpub.depth, 3);

    // get vanilla account xpub
    let (_, account_xpub, _) = get_account_data(
        &BitcoinNetwork::Regtest,
        &mnemonic,
        false,
        WitnessVersion::Taproot,
    )
    .unwrap();
    assert_eq!(account_xpub.network, NetworkKind::Test,);
    assert_eq!(account_xpub.depth, 3);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn get_descriptors_success() {
    // wallet
    let wallet = get_test_wallet(true, None);

    // get descriptors from keys
    let keys = wallet.get_keys();
    let bitcoin_network = wallet.bitcoin_network();
    let descriptors = keys.build_descriptors(&bitcoin_network).unwrap().0;

    // get descriptors from wallet
    let wlt_descriptors = wallet.get_descriptors();

    // assert descriptors are the same
    assert_eq!(descriptors, wlt_descriptors);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn supported_schemas() {
    initialize();
    let bitcoin_network = BitcoinNetwork::Regtest;

    // wallet (NIA schema supported)
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);
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
    let online_nia = wallet_nia.go_online(test_go_online_options(None)).unwrap();
    let mut party_nia = party!(wallet_nia, online_nia);
    fund_wallet(party_nia.get_address());
    party_nia.create_utxos_default();

    // issue a NIA asset, should work
    let asset_nia = party_nia.issue_asset_nia(Some(&[AMOUNT]));

    // issue a different schema asset, should fail
    let result = party_nia.issue_asset_cfa_result(Some(&[AMOUNT]), None);
    assert_matches!(result, Err(Error::UnsupportedSchema { asset_schema: _ }));
    let result = party_nia.issue_asset_ifa_result(Some(&[AMOUNT]), None, None);
    assert_matches!(result, Err(Error::UnsupportedSchema { asset_schema: _ }));
    let result = party_nia.issue_asset_uda_result(None, None, vec![]);
    assert_matches!(result, Err(Error::UnsupportedSchema { asset_schema: _ }));

    // recipient wallet (UDA schema supported)
    let keys_rcv = generate_keys(bitcoin_network, WitnessVersion::Taproot);
    let mut rcv_wallet_uda = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![AssetSchema::Uda],
        },
        SinglesigKeys::from_keys(&keys_rcv, None),
    )
    .unwrap();
    let rcv_online_uda = rcv_wallet_uda
        .go_online(test_go_online_options(None))
        .unwrap();
    let mut rcv_party_uda = party!(rcv_wallet_uda, rcv_online_uda);
    fund_wallet(rcv_party_uda.get_address());
    rcv_party_uda.create_utxos_default();

    // send asset unsupported by the recipient
    let receive_data = rcv_party_uda.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(66),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_nia.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    let rcv_transfer = rcv_party_uda.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party_uda.get_test_transfer_data(&rcv_transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );

    // refresh the recipient, transfer should fail
    rcv_party_uda.refresh_all();
    let rcv_transfer = rcv_party_uda.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party_uda.get_test_transfer_data(&rcv_transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Failed);

    // wallet (CFA schema supported)
    let mut wallet_cfa = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![AssetSchema::Cfa],
        },
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let online_cfa = wallet_cfa.go_online(test_go_online_options(None)).unwrap();
    let mut party_cfa = party!(wallet_cfa, online_cfa);

    // send asset unsupported by the sender
    let receive_data = rcv_party_uda.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(66),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party_cfa.send_result(&recipient_map);
    assert_matches!(result, Err(Error::UnsupportedSchema { asset_schema: _ }));

    // wallet (no schema supported)
    let result = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![],
        },
        SinglesigKeys::from_keys(&keys, None),
    );
    assert!(result.is_err());
    if let Err(e) = result {
        assert_matches!(e, Error::NoSupportedSchemas);
    }

    // wallet (mainnet, IFA schema supported)
    let bitcoin_network = BitcoinNetwork::Mainnet;
    let keys_mainnet = generate_keys(bitcoin_network, WitnessVersion::Taproot);
    let result = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![AssetSchema::Nia, AssetSchema::Ifa],
        },
        SinglesigKeys::from_keys(&keys_mainnet, None),
    );
    // IFA on mainnet not allowed
    assert!(result.is_err());
    if let Err(e) = result {
        assert_matches!(e, Error::CannotUseIfaOnMainnet);
    }
}
