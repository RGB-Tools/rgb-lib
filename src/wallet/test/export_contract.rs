use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, Some(&[AMOUNT, AMOUNT]));

    // export
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    let runtime = wallet.rgb_runtime().unwrap();
    let contract_id = ContractId::from_str(&asset.asset_id).unwrap();
    let contract = runtime.export_contract(contract_id).unwrap();
    let bak_info_after = wallet.database.get_backup_info().unwrap();
    assert_eq!(bak_info_after, bak_info_before);

    // checks
    assert_eq!(contract.contract_id().to_string(), asset.asset_id);
    assert_eq!(
        contract.schema_id(),
        NonInflatableAsset::schema().schema_id()
    );
    assert!(!contract.transfer);
    assert!(contract.bundles.is_empty());
    assert!(contract.terminals.is_empty());
}
