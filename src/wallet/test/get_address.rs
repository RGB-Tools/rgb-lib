use super::*;

#[test]
#[parallel]
fn success() {
    let mut wallet = get_test_wallet(false, None);
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_before = txn.get_backup_info().unwrap();
    txn.commit().unwrap();
    assert!(bak_info_before.is_none());
    let address = test_get_address(&mut wallet);
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_after = txn.get_backup_info().unwrap();
    txn.commit().unwrap();
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
