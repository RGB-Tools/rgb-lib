use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    let unsigned_psbt_str = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
            1000,
            FEE_RATE,
            false,
            false,
        )
        .unwrap();
    let unsigned_psbt = Psbt::from_str(&unsigned_psbt_str).unwrap();
    let psbt_txid = unsigned_psbt.unsigned_tx.compute_txid().to_string();

    // pre-abort: reservation + wallet_transaction exist
    assert_eq!(wallet.list_pending_vanilla_txs().unwrap().len(), 1);
    assert!(!wallet.database().iter_reserved_txos().unwrap().is_empty());

    // abort
    wallet.abort_pending_vanilla_tx(psbt_txid.clone()).unwrap();

    // post-abort: reservation + wallet_transaction row both gone
    assert!(wallet.list_pending_vanilla_txs().unwrap().is_empty());
    assert!(wallet.database().iter_reserved_txos().unwrap().is_empty());
    assert!(
        wallet
            .database()
            .get_wallet_transaction_with_reserved_txos_by_txid(&psbt_txid)
            .unwrap()
            .is_none()
    );

    // the previously-reserved UTXOs are now available again: a fresh send_btc_begin
    // re-selects them
    let unsigned_psbt_2_str = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
            1000,
            FEE_RATE,
            true,
            false,
        )
        .unwrap();
    let unsigned_psbt_2 = Psbt::from_str(&unsigned_psbt_2_str).unwrap();
    assert_eq!(
        unsigned_psbt.unsigned_tx.input,
        unsigned_psbt_2.unsigned_tx.input
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    // unknown txid
    let result = wallet.abort_pending_vanilla_tx(FAKE_TXID.to_string());
    assert!(matches!(result, Err(Error::CannotAbortPendingVanillaTx)));

    // a wallet_transaction row exists but has no attached reservations
    let txid = test_send_btc(
        &mut wallet,
        online,
        &test_get_address(&mut rcv_wallet),
        1000,
    );
    let (_wt, reservations) = wallet
        .database()
        .get_wallet_transaction_with_reserved_txos_by_txid(&txid)
        .unwrap()
        .expect("SendBtc wallet_transaction should exist after send_btc");
    assert!(reservations.is_empty());
    let result = wallet.abort_pending_vanilla_tx(txid);
    assert!(matches!(result, Err(Error::CannotAbortPendingVanillaTx)));
}
