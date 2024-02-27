use super::*;
use bdk::descriptor::Descriptor;
use serial_test::parallel;

fn check_wallet(wallet: &Wallet, network: BitcoinNetwork, keychain_vanilla: Option<u8>) {
    let external_descriptor = &wallet
        .bdk_wallet
        .get_descriptor_for_keychain(KeychainKind::External);
    match external_descriptor {
        Descriptor::Wpkh(ref wpkh) => {
            let full_derivation_path = wpkh.as_inner().full_derivation_path().unwrap().to_string();
            let split: Vec<&str> = full_derivation_path.split('/').collect();
            assert_eq!(split[1], KEYCHAIN_RGB_OPRET.to_string());
        }
        _ => panic!("wrong descriptor type"),
    }
    let internal_descriptor = &wallet
        .bdk_wallet
        .get_descriptor_for_keychain(KeychainKind::Internal);
    match internal_descriptor {
        Descriptor::Wpkh(ref wpkh) => {
            let full_derivation_path = wpkh.as_inner().full_derivation_path().unwrap().to_string();
            let split: Vec<&str> = full_derivation_path.split('/').collect();
            assert_eq!(
                split[1],
                keychain_vanilla.unwrap_or(KEYCHAIN_BTC).to_string()
            );
        }
        _ => panic!("wrong descriptor type"),
    }
    assert_eq!(wallet.wallet_data.bitcoin_network, network);
}

#[test]
#[parallel]
fn success() {
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
        pubkey: keys.account_xpub,
        mnemonic: Some(keys.mnemonic),
        vanilla_keychain,
    })
    .unwrap();
    check_wallet(&wallet, bitcoin_network, vanilla_keychain);
}

#[test]
#[parallel]
fn signet_success() {
    fs::create_dir_all(get_test_data_dir_string()).unwrap();

    let bitcoin_network = BitcoinNetwork::Signet;
    let mut wallet = get_test_wallet_with_net(true, None, bitcoin_network);
    check_wallet(&wallet, bitcoin_network, None);
    let electrum_url = "ssl://electrum.iriswallet.com:50033";
    test_go_online(&mut wallet, false, Some(electrum_url));
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.bitcoin_network, bitcoin_network);
}

#[test]
#[parallel]
fn testnet_success() {
    fs::create_dir_all(get_test_data_dir_string()).unwrap();

    let bitcoin_network = BitcoinNetwork::Testnet;
    let mut wallet = get_test_wallet_with_net(true, None, bitcoin_network);
    check_wallet(&wallet, bitcoin_network, None);
    let electrum_url = "ssl://electrum.iriswallet.com:50013";
    test_go_online(&mut wallet, false, Some(electrum_url));
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.bitcoin_network, bitcoin_network);
}

#[test]
#[parallel]
fn mainnet_success() {
    fs::create_dir_all(get_test_data_dir_string()).unwrap();

    let bitcoin_network = BitcoinNetwork::Mainnet;
    let mut wallet = get_test_wallet_with_net(true, None, bitcoin_network);
    check_wallet(&wallet, bitcoin_network, None);
    let electrum_url = "ssl://electrum.iriswallet.com:50003";
    test_go_online(&mut wallet, false, Some(electrum_url));
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
    wallet_data_bad.pubkey = s!("");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidPubkey { details: _ })));

    // bad byte in pubkey
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.pubkey = s!("l1iI0");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidPubkey { details: _ })));

    drop(wallet);

    // bad mnemonic word count
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.mnemonic = Some(s!(""));
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidMnemonic { details: _ })));

    // invalid vanilla keychain
    let mut wallet_data_bad = wallet_data;
    wallet_data_bad.vanilla_keychain = Some(KEYCHAIN_RGB_OPRET);
    let result = Wallet::new(wallet_data_bad.clone());
    assert!(matches!(result, Err(Error::InvalidVanillaKeychain)));
    wallet_data_bad.vanilla_keychain = Some(KEYCHAIN_RGB_TAPRET);
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidVanillaKeychain)));
}

#[test]
#[parallel]
fn re_instantiate_wallet() {
    initialize();

    let amount: u64 = 66;

    // create wallets
    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet, rcv_online) = get_funded_wallet!();
    let mut wallet_data = wallet.wallet_data.clone();

    // issue
    let asset = test_issue_asset_nia(&wallet, &online, None);

    // send
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    stop_mining();
    test_refresh_all(&rcv_wallet, &rcv_online);
    test_refresh_asset(&wallet, &online, &asset.asset_id);
    mine(true);
    test_refresh_asset(&rcv_wallet, &rcv_online, &asset.asset_id);
    test_refresh_asset(&wallet, &online, &asset.asset_id);

    // drop wallet
    drop(online);
    drop(wallet);

    // re-instantiate wallet
    let mut wallet = Wallet::new(wallet_data.clone()).unwrap();
    let online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();

    // check wallet asset
    check_test_wallet_data(&wallet, &asset, None, 1, amount);

    // drop wallet
    drop(online);
    drop(wallet);

    // re-instantiate wallet in watch only mode
    wallet_data.mnemonic = None;
    let mut wallet = Wallet::new(wallet_data).unwrap();
    let _online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();
}

#[test]
#[parallel]
fn watch_only() {
    initialize();

    fs::create_dir_all(get_test_data_dir_path()).unwrap();
    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network);

    // watch-only wallet
    let mut wallet_watch = Wallet::new(WalletData {
        data_dir: get_test_data_dir_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        pubkey: keys.account_xpub.clone(),
        mnemonic: None,
        vanilla_keychain: None,
    })
    .unwrap();
    let online_watch = wallet_watch
        .go_online(true, ELECTRUM_URL.to_string())
        .unwrap();

    // signer wallet
    let wallet_sign = Wallet::new(WalletData {
        data_dir: get_test_data_dir_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        pubkey: keys.account_xpub,
        mnemonic: Some(keys.mnemonic),
        vanilla_keychain: None,
    })
    .unwrap();

    // check generated addresses are the same
    let address_watch = wallet_watch.get_address().unwrap();
    let address_signer = wallet_sign.get_address().unwrap();
    assert_eq!(address_watch, address_signer);

    // fund wallet
    fund_wallet(address_watch);
    mine(false);
    let unspents = test_list_unspents(&wallet_watch, Some(&online_watch), false);
    assert_eq!(unspents.len(), 1);

    // create UTXOs
    let unsigned_psbt =
        test_create_utxos_begin_result(&wallet_watch, &online_watch, false, None, None, FEE_RATE)
            .unwrap();
    let signed_psbt = wallet_sign.sign_psbt(unsigned_psbt, None).unwrap();
    wallet_watch
        .create_utxos_end(online_watch.clone(), signed_psbt)
        .unwrap();
    let unspents = test_list_unspents(&wallet_watch, Some(&online_watch), false);
    assert_eq!(unspents.len(), UTXO_NUM as usize + 1);
}
