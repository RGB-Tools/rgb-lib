use super::*;

#[test]
#[parallel]
fn success() {
    let test_data_dir = create_test_data_dir();
    let test_data_dir_str = test_data_dir.to_string_lossy().to_string();

    // test manual values
    let keys = generate_keys(BitcoinNetwork::Signet);
    let wallet_1 = Wallet::new(
        WalletData {
            data_dir: test_data_dir_str.clone(),
            bitcoin_network: BitcoinNetwork::Signet,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: 1,
            supported_schemas: AssetSchema::VALUES.to_vec(),
        },
        SinglesigKeys::from_keys(&keys, Some(2)),
    )
    .unwrap();

    let wallet_1_data = test_get_wallet_data(&wallet_1);
    assert_eq!(wallet_1_data.data_dir, test_data_dir_str);
    assert_eq!(
        wallet_1.get_wallet_dir().parent().unwrap(),
        fs::canonicalize(&wallet_1_data.data_dir).unwrap(),
    );
    assert_eq!(wallet_1_data.bitcoin_network, BitcoinNetwork::Signet);
    assert!(matches!(wallet_1_data.database_type, DatabaseType::Sqlite));
    assert_eq!(wallet_1_data.max_allocations_per_utxo, 1);

    // test default values
    let wallet_2 = Wallet::new(
        WalletData {
            data_dir: test_data_dir_str.clone(),
            bitcoin_network: BitcoinNetwork::Regtest,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: 5,
            supported_schemas: AssetSchema::VALUES.to_vec(),
        },
        SinglesigKeys::from_keys_no_mnemonic(&keys, None),
    )
    .unwrap();
    let wallet_2_data = test_get_wallet_data(&wallet_2);
    assert_eq!(wallet_2_data.data_dir, test_data_dir_str);
    assert_eq!(wallet_2_data.bitcoin_network, BitcoinNetwork::Regtest);
    assert!(matches!(wallet_2_data.database_type, DatabaseType::Sqlite));
    assert_eq!(
        wallet_2_data.max_allocations_per_utxo,
        MAX_ALLOCATIONS_PER_UTXO
    );
}
