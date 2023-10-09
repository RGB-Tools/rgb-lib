use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    let wallet = get_test_wallet(false, None);
    let address = wallet.get_address().unwrap();
    assert!(!address.is_empty());
}
