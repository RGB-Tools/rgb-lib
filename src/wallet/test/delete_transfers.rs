use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    let blind_data = wallet.blind(None, Some(60)).unwrap();
    wallet
        .fail_transfers(online, Some(blind_data.blinded_utxo.clone()))
        .unwrap();
    wallet
        .delete_transfers(Some(blind_data.blinded_utxo))
        .unwrap();
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, _online) = get_funded_wallet!();

    let blind_data = wallet.blind(None, Some(60)).unwrap();
    let result = wallet.delete_transfers(Some(blind_data.blinded_utxo));
    assert!(matches!(result, Err(Error::CannotDeleteTransfer(_))));
}
