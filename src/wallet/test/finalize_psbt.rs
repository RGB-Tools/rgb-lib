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
    let signed_psbt = party
        .wallet
        .sign_psbt(unsigned_psbt_str.clone(), None)
        .unwrap();
    let finalized_psbt = party.wallet.finalize_psbt(signed_psbt, None);
    assert!(finalized_psbt.is_ok());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();
    let mut party = get_funded_party!();
    let result = party.wallet.finalize_psbt("rgb1invalid".to_string(), None);
    assert_matches!(result, Err(Error::InvalidPsbt { details: _ }));

    let address = party.get_address();
    let unsigned_psbt_str = party
        .wallet
        .send_btc_begin(party.online, address, AMOUNT, FEE_RATE, false, true)
        .unwrap();
    let wallet_2 = get_test_wallet(true, None);
    let result = wallet_2.finalize_psbt(unsigned_psbt_str, None);
    assert_matches!(result, Err(Error::CannotFinalizePsbt));
}
