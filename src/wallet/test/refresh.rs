use super::*;

#[test]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // asset not found
    let result = wallet.refresh(online, Some(s!("rgb1inexistent")));
    assert!(matches!(result, Err(Error::AssetNotFound(_))));
}
