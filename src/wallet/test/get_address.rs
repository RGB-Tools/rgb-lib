use super::*;

#[test]
fn success() {
    initialize();

    let wallet = get_test_wallet(false);
    let address = wallet.get_address();
    assert!(!address.is_empty());
}
