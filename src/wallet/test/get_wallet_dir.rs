use super::*;

#[test]
#[parallel]
fn success() {
    let test_data_dir = create_test_data_dir();

    let keys = generate_keys(BitcoinNetwork::Regtest);
    let wallet = Wallet::new(get_test_wallet_data(
        test_data_dir.to_string_lossy().as_ref(),
        &keys.account_xpub_colored,
        &keys.account_xpub_vanilla,
        &keys.mnemonic,
        &keys.master_fingerprint,
    ))
    .unwrap();

    let expected_dir = fs::canonicalize(test_data_dir.join(keys.master_fingerprint)).unwrap();

    let wallet_dir = test_get_wallet_dir(&wallet);
    assert_eq!(wallet_dir, expected_dir);
}
