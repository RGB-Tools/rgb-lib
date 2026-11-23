use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_funded_party!();
    let asset = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT]));

    let bak_info_before = party.db_backup_info();
    let (destination_dir, contract_path) = party.export_contract_to_tempdir(&asset.asset_id);
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    let expected_path = destination_dir.path().join(format!(
        "{}.{RGB_BIN_EXTENSION}",
        asset.asset_id.replace(ASSET_ID_PREFIX, "")
    ));
    assert_eq!(PathBuf::from(&contract_path), expected_path);
    let contract = Contract::load_file(&contract_path).unwrap();
    assert_eq!(contract.contract_id().to_string(), asset.asset_id);
    assert_eq!(
        contract.schema_id(),
        NonInflatableAsset::schema().schema_id()
    );
    assert!(!contract.transfer);
    assert!(contract.bundles.is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let mut party = get_funded_party!();
    let asset = party.issue_asset_nia(None);
    let destination_dir = TempDir::new().unwrap();
    let destination_dir_str = destination_dir.path().to_string_lossy().to_string();

    // unknown asset
    let result = party
        .wallet
        .export_contract(s!("rgb:unknown"), destination_dir_str.clone());
    assert_matches!(result, Err(Error::AssetNotFound { asset_id }) if asset_id == "rgb:unknown");

    // missing destination directory
    let missing_dir = destination_dir
        .path()
        .join("missing")
        .to_string_lossy()
        .to_string();
    let result = party
        .wallet
        .export_contract(asset.asset_id.clone(), missing_dir.clone());
    assert_matches!(result, Err(Error::InvalidFilePath { file_path }) if file_path == missing_dir);

    // destination is a file
    let result = party
        .wallet
        .export_contract(asset.asset_id.clone(), FILE_STR.to_string());
    assert_matches!(result, Err(Error::InvalidFilePath { file_path }) if file_path == FILE_STR);

    // contract file already exists
    let contract_path = party
        .wallet
        .export_contract(asset.asset_id.clone(), destination_dir_str.clone())
        .unwrap();
    let result = party
        .wallet
        .export_contract(asset.asset_id.clone(), destination_dir_str);
    assert_matches!(result, Err(Error::FileAlreadyExists { path }) if path == contract_path);
    // leaving the existing file alone and no temporary file behind
    let files: Vec<PathBuf> = fs::read_dir(destination_dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files, vec![PathBuf::from(&contract_path)]);
    Contract::load_file(&contract_path).unwrap();
}
