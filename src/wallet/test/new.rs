use std::io::ErrorKind;

use bdk::miniscript::descriptor::DescriptorType;

use super::*;

fn check_wallet(wallet: &Wallet, desc_type: DescriptorType, network: BitcoinNetwork) {
    let coin_type = if network == BitcoinNetwork::Mainnet {
        0
    } else {
        1
    };
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
    initialize();

    // with private keys
    get_test_wallet(true);

    // without private keys
    get_test_wallet(false);
}

#[test]
fn testnet_success() {
    fs::create_dir_all(TEST_DATA_DIR).unwrap();

    let bitcoin_network = BitcoinNetwork::Testnet;
    let keys = generate_keys(bitcoin_network);
    let wallet = Wallet::new(WalletData {
        data_dir: TEST_DATA_DIR.to_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
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
fn mainnet_success() {
    fs::create_dir_all(TEST_DATA_DIR).unwrap();

    let bitcoin_network = BitcoinNetwork::Mainnet;
    let keys = generate_keys(bitcoin_network);
    let wallet = Wallet::new(WalletData {
        data_dir: TEST_DATA_DIR.to_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
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
fn mainnet_with_online_success() {
    fs::create_dir_all(TEST_DATA_DIR).unwrap();

    let bitcoin_network = BitcoinNetwork::Mainnet;
    let keys = generate_keys(bitcoin_network);
    let wallet = Wallet::new_with_online(
        WalletData {
            data_dir: TEST_DATA_DIR.to_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            pubkey: keys.xpub.clone(),
            mnemonic: Some(keys.mnemonic.clone()),
        },
        Some(Online {
            id: 1,
            electrum_url: "".to_string(),
            proxy_url: "".to_string(),
        }),
    )
    .unwrap();
    check_wallet(&wallet, DescriptorType::Wpkh, bitcoin_network);
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.pubkey, keys.xpub);
    assert_eq!(wallet.wallet_data.mnemonic, Some(keys.mnemonic));
    assert!(wallet.online.is_some());
}

#[test]
fn fail() {
    initialize();

    let wallet = get_test_wallet(true);
    let wallet_data = wallet.get_wallet_data();

    // inexistent data dir
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.data_dir = s!("");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::IO(_))));
    if let Err(Error::IO(err)) = result {
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    // pubkey too short
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.pubkey = s!("");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidPubkey(_))));

    // bad byte in pubkey
    let mut wallet_data_bad = wallet_data.clone();
    wallet_data_bad.pubkey = s!("l1iI0");
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidPubkey(_))));

    // bad mnemonic word count
    let mut wallet_data_bad = wallet_data;
    wallet_data_bad.mnemonic = Some(s!(""));
    let result = Wallet::new(wallet_data_bad);
    assert!(matches!(result, Err(Error::InvalidMnemonic(_))));
}
