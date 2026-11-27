use super::*;

/// Build and import a public contract without allocating anything to the test wallet. No
/// indexer or proxy is needed to test metadata recovered from the validated RGB stock.
fn import_bfa(wallet: &Wallet, ticker: &str, reject_list: bool) -> String {
    let location = EvmContract {
        chain_id: 42161,
        address: wallet
            .check_contract_address("0x1111111111111111111111111111111111111111".to_string())
            .unwrap(),
    };
    let mut builder = ContractBuilder::with(
        Identity::default(),
        BridgedFungibleAsset::schema_rules(),
        wallet.chain_net(),
    )
    .add_global_state(
        "ticker",
        wallet.check_unicode_ticker(ticker.to_string()).unwrap(),
    )
    .unwrap()
    .add_global_state(
        "name",
        wallet.check_name("Upgrade fixture".to_string()).unwrap(),
    )
    .unwrap()
    .add_global_state("precision", wallet.check_precision(6).unwrap())
    .unwrap()
    .add_global_state(
        "terms",
        wallet.new_asset_terms(RicardianContract::default(), None),
    )
    .unwrap()
    .add_global_state(
        RGB_GLOBAL_BRIDGE_LOCATION,
        BridgeLocation::Evm(location),
    )
    .unwrap()
    .add_rights(
        RGB_STATE_MINT_RIGHT,
        wallet.get_builder_seal(Outpoint {
            txid: "11".repeat(32),
            vout: 0,
        }),
    )
    .unwrap();
    if reject_list {
        builder = builder
            .add_global_state(RGB_GLOBAL_REJECT_LIST, RejectListLocation::Evm(location))
            .unwrap();
    }
    let (_, path, contract) = wallet.issue_contract(builder).unwrap();
    contract
        .clone()
        .into_consignment()
        .save_file(&path)
        .unwrap();
    wallet
        .import_contract(path.to_string_lossy().into_owned(), vec![])
        .unwrap();
    contract.contract_id().to_string()
}

fn import_nia(wallet: &Wallet) -> String {
    let builder = ContractBuilder::with(
        Identity::default(),
        NonInflatableAsset::schema_rules(),
        wallet.chain_net(),
    )
    .add_global_state(
        "spec",
        AssetSpec {
            ticker: wallet.check_ticker("NIA".to_string()).unwrap(),
            name: wallet.check_name("Unchanged NIA".to_string()).unwrap(),
            details: None,
            precision: wallet.check_precision(0).unwrap(),
        },
    )
    .unwrap()
    .add_global_state(
        "terms",
        wallet.new_asset_terms(RicardianContract::default(), None),
    )
    .unwrap()
    .add_global_state(RGB_GLOBAL_ISSUED_SUPPLY, Amount::from(10u64))
    .unwrap()
    .add_fungible_state(
        RGB_STATE_ASSET_OWNER,
        wallet.get_builder_seal(Outpoint {
            txid: "22".repeat(32),
            vout: 0,
        }),
        10u64,
    )
    .unwrap();
    let (_, path, contract) = wallet.issue_contract(builder).unwrap();
    contract
        .clone()
        .into_consignment()
        .save_file(&path)
        .unwrap();
    wallet
        .import_contract(path.to_string_lossy().into_owned(), vec![])
        .unwrap();
    contract.contract_id().to_string()
}

fn clear_bfa_metadata(wallet: &Wallet) {
    let txn = wallet.database().begin_transaction().unwrap();
    for asset in txn
        .iter_assets()
        .unwrap()
        .into_iter()
        .filter(|a| a.schema == AssetSchema::Bfa)
    {
        let mut asset: DbAssetActMod = asset.into();
        asset.bridge_location = ActiveValue::Set(None);
        asset.reject_list = ActiveValue::Set(None);
        txn.update_asset(&mut asset).unwrap();
    }
    txn.commit().unwrap();
}

#[test]
fn bfa_metadata_upgrade_backfills_from_stock() {
    let data_dir = PrivateDataDir::new();
    let wallet = data_dir.wallet(false, None);
    let ids = [
        import_bfa(&wallet, "BFA1", false),
        import_bfa(&wallet, "BFA2", true),
        import_nia(&wallet),
    ];
    let metadata: Vec<_> = ids
        .iter()
        .map(|id| wallet.get_asset_metadata(id.clone()).unwrap())
        .collect();
    let assets = serde_json::to_value(wallet.list_assets(vec![]).unwrap()).unwrap();
    let transfers = serde_json::to_value(
        wallet
            .list_transfers(AssetFilter::AnyOrNone, TransferScope::All)
            .unwrap(),
    )
    .unwrap();
    let wallet_data = wallet.get_wallet_data();
    let keys = wallet.get_keys();
    let db_path = wallet.get_wallet_dir().join(RGB_LIB_DB_NAME);
    drop(wallet);

    // Restore the previous metadata columns and migration markers. The stored contracts,
    // allocations and transfers remain untouched, as in a real wallet upgrade.
    let db = block_on(sea_orm::Database::connect(format!(
        "sqlite:{}?mode=rw",
        db_path.display()
    )))
    .unwrap();
    for statement in [
        "ALTER TABLE asset ADD COLUMN reject_list_url VARCHAR NULL",
        "UPDATE asset SET reject_list_url = json_extract(reject_list, '$.Url')",
        "ALTER TABLE asset DROP COLUMN reject_list",
        "ALTER TABLE asset DROP COLUMN bridge_location",
        "DELETE FROM seaql_migrations WHERE version IN ('m20261117_104500_asset_reject_list', 'm20261117_160000_asset_bridge_location')",
    ] {
        block_on(sea_orm::ConnectionTrait::execute_unprepared(&db, statement)).unwrap();
    }
    block_on(db.close()).unwrap();

    // Opening backfills both the bridge and the EVM reject list from the retained genesis.
    for _ in 0..2 {
        let wallet = Wallet::new(wallet_data.clone(), keys.clone()).unwrap();
        for (id, expected) in ids.iter().zip(&metadata) {
            assert_eq!(&wallet.get_asset_metadata(id.clone()).unwrap(), expected);
        }
        assert_eq!(
            serde_json::to_value(wallet.list_assets(vec![]).unwrap()).unwrap(),
            assets
        );
        assert_eq!(
            serde_json::to_value(
                wallet
                    .list_transfers(AssetFilter::AnyOrNone, TransferScope::All)
                    .unwrap()
            )
            .unwrap(),
            transfers
        );
        assert!(
            wallet
                .list_transfers(AssetFilter::AnyOrNone, TransferScope::All)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn bfa_metadata_upgrade_missing_stock_is_atomic() {
    let data_dir = PrivateDataDir::new();
    let wallet = data_dir.wallet(false, None);
    let first = import_bfa(&wallet, "BFA1", true);
    let second = import_bfa(&wallet, "BFA2", false);
    clear_bfa_metadata(&wallet);
    let txn = wallet.database().begin_transaction().unwrap();
    // A database row without its authoritative contract cannot be repaired by inventing data.
    // Move the second row to a valid but absent contract ID, preserving its shape.
    let mut asset: DbAssetActMod = txn.get_asset(second).unwrap().unwrap().into();
    asset.id = ActiveValue::Set("rgb:Ar4ouaLv-b7f7Dc_-z5EMvtu-FA5KNh1-nlae~jk-8xMBo7E".to_string());
    txn.update_asset(&mut asset).unwrap();
    txn.commit().unwrap();

    assert!(
        setup_rgb(
            wallet.database(),
            AssetSchema::VALUES.to_vec(),
            BitcoinNetwork::Regtest,
            false,
            wallet.logger()
        )
        .is_err()
    );
    let txn = wallet.database().begin_transaction().unwrap();
    let first = txn.get_asset(first).unwrap().unwrap();
    assert!(first.bridge_location.is_none());
    assert!(first.reject_list.is_none());
    txn.commit().unwrap();
    assert_matches!(wallet.list_assets(vec![]), Err(Error::Inconsistency { .. }));
    let data = wallet.get_wallet_data();
    let keys = wallet.get_keys();
    drop(wallet);
    assert!(Wallet::new(data, keys).is_err());
}

#[test]
fn bfa_metadata_upgrade_empty_wallet_reopens() {
    let data_dir = PrivateDataDir::new();
    let wallet = data_dir.wallet(false, None);
    let data = wallet.get_wallet_data();
    let keys = wallet.get_keys();
    let expected = serde_json::to_value(wallet.list_assets(vec![]).unwrap()).unwrap();
    drop(wallet);
    let wallet = Wallet::new(data, keys).unwrap();
    assert_eq!(
        serde_json::to_value(wallet.list_assets(vec![]).unwrap()).unwrap(),
        expected
    );
}
