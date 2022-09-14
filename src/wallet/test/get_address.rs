use super::*;

#[test]
fn success() {
    initialize();

    let (wallet, _online) = get_empty_wallet!();
    let address = wallet.get_address();
    assert!(!address.is_empty());
}
