use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();
    let (mut wallet, online) = get_funded_wallet!();
    let address = test_get_address(&mut wallet);
    let unsigned_psbt_str = wallet
        .send_btc_begin(online, address, AMOUNT, FEE_RATE, false, true)
        .unwrap();
    let signed_psbt = wallet.sign_psbt(unsigned_psbt_str.clone(), None).unwrap();
    let finalized_psbt = wallet.finalize_psbt(signed_psbt, None);
    assert!(finalized_psbt.is_ok());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();
    let (mut wallet_1, online) = get_funded_wallet!();
    let result = wallet_1.finalize_psbt("rgb1invalid".to_string(), None);
    assert_matches!(result, Err(Error::InvalidPsbt { details: _ }));

    let address = test_get_address(&mut wallet_1);
    let unsigned_psbt_str = wallet_1
        .send_btc_begin(online, address, AMOUNT, FEE_RATE, false, true)
        .unwrap();
    let wallet_2 = get_test_wallet(true, None);
    let result = wallet_2.finalize_psbt(unsigned_psbt_str, None);
    assert_matches!(result, Err(Error::CannotFinalizePsbt));
}
