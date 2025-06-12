use super::*;

use std::os::unix::fs::PermissionsExt;

fn check_wallet(wallet: &Wallet, network: BitcoinNetwork, keychain_vanilla: Option<u8>) {
    let keychains: Vec<_> = wallet.bdk_wallet.keychains().collect();
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
                    let account_derivation_children = get_account_derivation_children(coin_type);
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
                    let account_derivation_children = get_account_derivation_children(coin_type);
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
    assert_eq!(wallet.wallet_data.bitcoin_network, network);
}

#[test]
#[parallel]
fn success() {
    create_test_data_dir();

    // with private keys
    let wallet = get_test_wallet(true, None);
    let bak_info_after = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_after.is_none());

    // without private keys
    let wallet = get_test_wallet(false, None);
    check_wallet(&wallet, BitcoinNetwork::Regtest, None);

    // with custom vanilla keychain
    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network);
    let vanilla_keychain = Some(u8::MAX);
    let wallet = Wallet::new(WalletData {
        data_dir: get_test_data_dir_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        account_xpub_colored: keys.account_xpub_colored,
        account_xpub_vanilla: keys.account_xpub_vanilla,
        mnemonic: Some(keys.mnemonic),
        master_fingerprint: keys.master_fingerprint,
        vanilla_keychain,
    })
    .unwrap();
    check_wallet(&wallet, bitcoin_network, vanilla_keychain);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn signet_success() {
    create_test_data_dir();

    let bitcoin_network = BitcoinNetwork::Signet;
    let mut wallet = get_test_wallet_with_net(true, None, bitcoin_network);
    check_wallet(&wallet, bitcoin_network, None);
    let indexer_url = "ssl://electrum.iriswallet.com:50033";
    test_go_online(&mut wallet, false, Some(indexer_url));
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.bitcoin_network, bitcoin_network);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn testnet_success() {
    create_test_data_dir();

    let bitcoin_network = BitcoinNetwork::Testnet;
    let mut wallet = get_test_wallet_with_net(true, None, bitcoin_network);
    check_wallet(&wallet, bitcoin_network, None);
    let indexer_url = "ssl://electrum.iriswallet.com:50013";
    test_go_online(&mut wallet, false, Some(indexer_url));
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.bitcoin_network, bitcoin_network);
}

#[cfg(all(feature = "electrum", feature = "esplora"))]
#[test]
#[parallel]
fn mainnet_success() {
    create_test_data_dir();

    let bitcoin_network = BitcoinNetwork::Mainnet;
    let mut wallet = get_test_wallet_with_net(true, None, bitcoin_network);
    check_wallet(&wallet, bitcoin_network, None);
    let indexer_url = "ssl://electrum.iriswallet.com:50003";
    test_go_online(&mut wallet, false, Some(indexer_url));
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.bitcoin_network, bitcoin_network);

    let indexer_url = "https://blockstream.info/api";
    test_go_online(&mut wallet, false, Some(indexer_url));
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.bitcoin_network, bitcoin_network);
}

#[test]
#[parallel]
fn fail() {
    let wallet = get_test_wallet(true, None);
    let wallet_data = test_get_wallet_data(&wallet);

    // inexistent data dir
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.data_dir = s!("");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InexistentDataDir)));

    // pubkey too short
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.account_xpub_colored = s!("");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidPubkey { details: _ })));

    // bad byte in pubkey
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.account_xpub_colored = s!("l1iI0");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidPubkey { details: _ })));

    drop(wallet);

    // bad mnemonic word count
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.mnemonic = Some(s!(""));
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidMnemonic { details: _ })));

    // invalid bitcoin keys
    let mut wallet_data_bad = wallet_data.clone();
    let alt_keys = generate_keys(BitcoinNetwork::Regtest);
    wallet_data_bad.account_xpub_colored = alt_keys.xpub;
    let result = Wallet::new(wallet_data_bad.clone());
    assert!(matches!(result, Err(Error::InvalidBitcoinKeys)));

    // bitcoin network mismatch
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.bitcoin_network = BitcoinNetwork::Testnet;
    let result = Wallet::new(wallet_data_bad.clone());
    assert!(matches!(result, Err(Error::BitcoinNetworkMismatch)));

    // invalid fingerprint
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.master_fingerprint = s!("invalid");
    let result = Wallet::new(wallet_data_bad.clone());
    assert!(matches!(result, Err(Error::InvalidFingerprint)));

    // fingerprint mismatch
    let mut wallet_data_bad = wallet_data;
    wallet_data_bad.master_fingerprint = s!("badbadff");
    let result = Wallet::new(wallet_data_bad.clone());
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let mut wallet_data = wallet.wallet_data.clone();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    // drop wallet
    drop(online);
    drop(wallet);

    // re-instantiate wallet
    let mut wallet = Wallet::new(wallet_data.clone()).unwrap();
    let online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();

    // check wallet asset
    check_test_wallet_data(&mut wallet, &asset, None, 1, amount);

    // drop wallet
    drop(online);
    drop(wallet);

    // re-instantiate wallet in watch only mode
    wallet_data.mnemonic = None;
    let mut wallet = Wallet::new(wallet_data).unwrap();
    let _online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn watch_only_success() {
    initialize();

    create_test_data_dir();
    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network);

    // watch-only wallet
    let mut wallet_watch = Wallet::new(WalletData {
        data_dir: get_test_data_dir_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        account_xpub_colored: keys.account_xpub_colored.clone(),
        account_xpub_vanilla: keys.account_xpub_vanilla.clone(),
        mnemonic: None,
        master_fingerprint: keys.master_fingerprint.clone(),
        vanilla_keychain: None,
    })
    .unwrap();
    let online_watch = wallet_watch
        .go_online(true, ELECTRUM_URL.to_string())
        .unwrap();

    // signer wallet
    let mut wallet_sign = Wallet::new(WalletData {
        data_dir: get_test_data_dir_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        account_xpub_colored: keys.account_xpub_colored,
        account_xpub_vanilla: keys.account_xpub_vanilla,
        mnemonic: Some(keys.mnemonic),
        master_fingerprint: keys.master_fingerprint.clone(),
        vanilla_keychain: None,
    })
    .unwrap();

    // check generated addresses are the same
    let address_watch = wallet_watch.get_address().unwrap();
    let address_signer = wallet_sign.get_address().unwrap();
    assert_eq!(address_watch, address_signer);

    // fund wallet
    fund_wallet(address_watch);
    mine(false, false);
    let unspents = test_list_unspents(&mut wallet_watch, Some(&online_watch), false);
    assert_eq!(unspents.len(), 1);

    // create UTXOs
    let unsigned_psbt = test_create_utxos_begin_result(
        &mut wallet_watch,
        &online_watch,
        false,
        None,
        None,
        FEE_RATE,
    )
    .unwrap();
    let signed_psbt = wallet_sign.sign_psbt(unsigned_psbt, None).unwrap();
    wallet_watch
        .create_utxos_end(online_watch.clone(), signed_psbt, false)
        .unwrap();
    let unspents = test_list_unspents(&mut wallet_watch, Some(&online_watch), false);
    assert_eq!(unspents.len(), UTXO_NUM as usize + 1);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn watch_only_fail() {
    initialize();

    create_test_data_dir();
    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network);

    // watch-only wallet invalid fingerprint
    let result = Wallet::new(WalletData {
        data_dir: get_test_data_dir_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        account_xpub_colored: keys.account_xpub_colored.clone(),
        account_xpub_vanilla: keys.account_xpub_vanilla.clone(),
        mnemonic: None,
        master_fingerprint: s!("invalid"),
        vanilla_keychain: None,
    });
    assert!(matches!(result, Err(Error::InvalidFingerprint)));
}

#[test]
#[parallel]
fn get_account_xpub_success() {
    // wallet
    let wallet = get_test_wallet(true, None);
    let mnemonic = wallet.wallet_data.mnemonic.unwrap();

    // get colored account xpub
    let (_, account_xpub, _) = get_account_data(BitcoinNetwork::Regtest, &mnemonic, true).unwrap();
    assert_eq!(account_xpub.network, NetworkKind::Test,);
    assert_eq!(account_xpub.depth, 3);

    // get vanilla account xpub
    let (_, account_xpub, _) = get_account_data(BitcoinNetwork::Regtest, &mnemonic, false).unwrap();
    assert_eq!(account_xpub.network, NetworkKind::Test,);
    assert_eq!(account_xpub.depth, 3);
}
