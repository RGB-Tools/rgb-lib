use super::*;

#[test]
#[parallel]
fn success() {
    let mut party = offline_party!(get_test_wallet(false, None));
    let bak_info_before = party.db_backup_info_opt();
    assert!(bak_info_before.is_none());
    let address = party.get_address();
    let bak_info_after = party.db_backup_info();
    assert!(
        bak_info_after
            .last_operation_timestamp
            .parse::<i128>()
            .unwrap()
            > 0
    );
    assert!(!address.is_empty());
}
