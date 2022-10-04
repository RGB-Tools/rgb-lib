use std::io::ErrorKind;

use super::*;

#[test]
fn success() {
    initialize();

    // with private keys
    get_test_wallet(true);

    // without private keys
    get_test_wallet(false);
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
