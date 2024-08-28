use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();
    let (mut wallet, online) = get_funded_wallet!();
    let address = test_get_address(&mut wallet);

    let unsigned_psbt_str = wallet
        .send_btc_begin(online, address, AMOUNT, FEE_RATE, false)
        .unwrap();

    // no SignOptions
    let signed_psbt = wallet.sign_psbt(unsigned_psbt_str.clone(), None).unwrap();
    assert!(Psbt::from_str(&signed_psbt).is_ok());

    // with SignOptions
    let opts = SignOptions::default();
    let signed_psbt = wallet
        .sign_psbt(unsigned_psbt_str.clone(), Some(opts))
        .unwrap();
    assert!(Psbt::from_str(&signed_psbt).is_ok());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();
    let wallet = get_test_wallet(true, None);

    let result = wallet.sign_psbt("rgb1invalid".to_string(), None);
    assert!(matches!(result, Err(Error::InvalidPsbt { details: _ })));
}
