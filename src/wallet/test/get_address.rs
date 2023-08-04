use super::*;

#[test]
fn success() {
    let wallet = get_test_wallet(false, None);
    let address = wallet.get_address();
    assert!(!address.is_empty());
}
