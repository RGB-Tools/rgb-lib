use bdk::miniscript::descriptor::DescriptorType;

use super::*;

fn check_wallet(wallet: &Wallet, desc_type: DescriptorType, network: BitcoinNetwork) {
    let coin_type = i32::from(network != BitcoinNetwork::Mainnet);
    let descriptor = &wallet
        .bdk_wallet
        .get_descriptor_for_keychain(KeychainKind::External);
    let descriptor_type = &descriptor.desc_type();
    assert_eq!(descriptor_type, &desc_type);
    let mut descriptor_string = descriptor.to_string();
    let _split = descriptor_string.split_off(20); // "wpkh([<chksum>/84'/0", "..."
    let descriptor_coin_type = descriptor_string.split_off(19);
    assert_eq!(descriptor_coin_type, coin_type.to_string());
    assert_eq!(wallet.bitcoin_network, network);
    assert_eq!(wallet.wallet_data.bitcoin_network, network);
}

#[test]
fn success() {
    // with private keys
    get_test_wallet(true, None);

    // without private keys
    get_test_wallet(false, None);
}

#[test]
fn testnet_success() {
    fs::create_dir_all(TEST_DATA_DIR).unwrap();

    let bitcoin_network = BitcoinNetwork::Testnet;
    let keys = generate_keys(bitcoin_network);
    let mut wallet = Wallet::new(WalletData {
        data_dir: TEST_DATA_DIR.to_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        pubkey: keys.xpub.clone(),
        mnemonic: Some(keys.mnemonic.clone()),
    })
    .unwrap();
    check_wallet(&wallet, DescriptorType::Wpkh, bitcoin_network);
    wallet
        .go_online(false, s!("ssl://electrum.iriswallet.com:50013"))
        .unwrap();
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.pubkey, keys.xpub);
    assert_eq!(wallet.wallet_data.mnemonic, Some(keys.mnemonic));
}

#[test]
fn mainnet_success() {
    fs::create_dir_all(TEST_DATA_DIR).unwrap();

    let bitcoin_network = BitcoinNetwork::Mainnet;
    let keys = generate_keys(bitcoin_network);
    let wallet = Wallet::new(WalletData {
        data_dir: TEST_DATA_DIR.to_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        pubkey: keys.xpub.clone(),
        mnemonic: Some(keys.mnemonic.clone()),
    })
    .unwrap();
    check_wallet(&wallet, DescriptorType::Wpkh, bitcoin_network);
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.pubkey, keys.xpub);
    assert_eq!(wallet.wallet_data.mnemonic, Some(keys.mnemonic));
}

#[test]
fn fail() {
    let wallet = get_test_wallet(true, None);
    let wallet_data = wallet.get_wallet_data();

    // inexistent data dir
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.data_dir = s!("");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::IO { details: _ })));
    if let Err(Error::IO { details: err }) = result {
        assert_eq!(err, "No such file or directory (os error 2)");
    }

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
    let mut wallet_data_bad = wallet_data;
    wallet_data_bad.mnemonic = Some(s!(""));
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidMnemonic { details: _ })));
}

#[test]
fn re_instantiate_wallet() {
    initialize();

    let amount: u64 = 66;

    // create wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let wallet_data = wallet.wallet_data.clone();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    stop_mining();
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(true);
    rcv_wallet
        .refresh(rcv_online, Some(asset.asset_id.clone()), vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // drop wallet
    drop(online);
    drop(wallet);

    // re-instantiate wallet
    let mut wallet = Wallet::new(wallet_data).unwrap();
    let _online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();

    // check wallet asset
    check_test_wallet_data(&mut wallet, &asset, None, 1, amount);
}
