use super::*;

#[test]
#[parallel]
fn success() {
    let mut wallet = get_test_wallet(false, None);
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_before.is_none());
    let address = test_get_address(&mut wallet);
    let bak_info_after = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_after.is_some());
    assert!(
        bak_info_after
            .unwrap()
            .last_operation_timestamp
            .parse::<i128>()
            .unwrap()
            > 0
    );
    assert!(!address.is_empty());
}
