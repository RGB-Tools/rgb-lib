use super::*;

#[test]
#[parallel]
fn success() {
    let test_data_dir = PrivateDataDir::new();

    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet = Wallet::new(
        test_data_dir.wallet_data(),
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();

    let expected_dir = fs::canonicalize(test_data_dir.sub_path(keys.master_fingerprint)).unwrap();

    let wallet_dir = wallet.get_wallet_dir();
    assert_eq!(wallet_dir, expected_dir);
}
