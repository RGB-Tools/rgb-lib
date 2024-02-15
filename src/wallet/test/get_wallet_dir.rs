use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    let test_data_dir_str = get_test_data_dir_string();
    let test_data_dir = PathBuf::from(test_data_dir_str.clone());
    fs::create_dir_all(&test_data_dir).unwrap();

    let keys = generate_keys(BitcoinNetwork::Regtest);
    let wallet = Wallet::new(get_test_wallet_data(
        &test_data_dir_str,
        &keys.account_xpub,
        &keys.mnemonic,
    ))
    .unwrap();

    let expected_dir = fs::canonicalize(test_data_dir.join(keys.account_xpub_fingerprint)).unwrap();

    let wallet_dir = test_get_wallet_dir(&wallet);
    assert_eq!(wallet_dir, expected_dir);
}
