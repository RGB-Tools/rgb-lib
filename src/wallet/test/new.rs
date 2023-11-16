use super::*;
use crate::utils::{ACCOUNT, PURPOSE};
use bdk::descriptor::Descriptor;
use serial_test::parallel;

fn check_wallet(wallet: &Wallet, network: BitcoinNetwork, keychain_vanilla: Option<u8>) {
    let hardened = if wallet.wallet_data.mnemonic.is_some() {
        true
    } else {
        false
    };
    let mut coin_type = i32::from(network != BitcoinNetwork::Mainnet).to_string();
    let mut purpose = PURPOSE.to_string();
    let mut account = ACCOUNT.to_string();
    if hardened {
        coin_type = format!("{coin_type}'");
        purpose = format!("{purpose}'");
        account = format!("{account}'");
    }
    let external_descriptor = &wallet
        .bdk_wallet
        .get_descriptor_for_keychain(KeychainKind::External);
    match external_descriptor {
        Descriptor::Wpkh(ref wpkh) => {
            let full_derivation_path = wpkh.as_inner().full_derivation_path().unwrap().to_string();
            let split: Vec<&str> = full_derivation_path.split("/").collect();
            assert_eq!(split[1], purpose);
            assert_eq!(split[2], coin_type);
            assert_eq!(split[3], account);
            assert_eq!(split[4], KEYCHAIN_RGB_OPRET.to_string());
        }
        _ => panic!("wrong descriptor type"),
    }
    let internal_descriptor = &wallet
        .bdk_wallet
        .get_descriptor_for_keychain(KeychainKind::Internal);
    match internal_descriptor {
        Descriptor::Wpkh(ref wpkh) => {
            let full_derivation_path = wpkh.as_inner().full_derivation_path().unwrap().to_string();
            let split: Vec<&str> = full_derivation_path.split("/").collect();
            assert_eq!(split[1], purpose);
            assert_eq!(split[2], coin_type);
            assert_eq!(split[3], account);
            assert_eq!(
                split[4],
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
        data_dir: TEST_DATA_DIR.to_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        pubkey: keys.xpub.clone(),
        mnemonic: Some(keys.mnemonic.clone()),
        vanilla_keychain,
    })
    .unwrap();
    check_wallet(&wallet, bitcoin_network, vanilla_keychain);
}

#[test]
#[parallel]
fn testnet_success() {
    fs::create_dir_all(TEST_DATA_DIR).unwrap();

    let bitcoin_network = BitcoinNetwork::Testnet;
    let mut wallet = get_test_wallet_with_net(true, None, bitcoin_network);
    check_wallet(&wallet, bitcoin_network, None);
    wallet
        .go_online(false, s!("ssl://electrum.iriswallet.com:50013"))
        .unwrap();
    assert!(!wallet.watch_only);
    assert_eq!(wallet.wallet_data.bitcoin_network, bitcoin_network);
}

#[test]
#[parallel]
fn mainnet_success() {
    fs::create_dir_all(TEST_DATA_DIR).unwrap();

    let bitcoin_network = BitcoinNetwork::Mainnet;
    let mut wallet = get_test_wallet_with_net(true, None, bitcoin_network);
    check_wallet(&wallet, bitcoin_network, None);
    wallet
        .go_online(false, s!("ssl://electrum.iriswallet.com:50003"))
        .unwrap();
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let wallet_data = wallet.wallet_data.clone();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send
    let receive_data = test_blind_receive(&mut rcv_wallet);
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
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    stop_mining();
    test_refresh_all(&mut rcv_wallet, &rcv_online);
    test_refresh_asset(&mut wallet, &online, &asset.asset_id);
    mine(true);
    test_refresh_asset(&mut rcv_wallet, &rcv_online, &asset.asset_id);
    test_refresh_asset(&mut wallet, &online, &asset.asset_id);

    // drop wallet
    drop(online);
    drop(wallet);

    // re-instantiate wallet
    let mut wallet = Wallet::new(wallet_data).unwrap();
    let _online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();

    // check wallet asset
    check_test_wallet_data(&mut wallet, &asset, None, 1, amount);
}
