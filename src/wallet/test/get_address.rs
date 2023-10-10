use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    let wallet = get_test_wallet(false, None);
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_before.is_none());
    let address = wallet.get_address().unwrap();
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
