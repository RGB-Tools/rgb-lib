use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();
    let mut party = get_funded_party!();
    let address = party.get_address();

    let unsigned_psbt_str = party
        .wallet
        .send_btc_begin(party.online, address, AMOUNT, FEE_RATE, false, true)
        .unwrap();

    // no SignOptions
    let signed_psbt = party
        .wallet
        .sign_psbt(unsigned_psbt_str.clone(), None)
        .unwrap();
    assert!(Psbt::from_str(&signed_psbt).is_ok());

    // with SignOptions
    let opts = SignOptions::default();
    let signed_psbt = party
        .wallet
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
