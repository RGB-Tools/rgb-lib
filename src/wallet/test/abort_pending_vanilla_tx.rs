use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_empty_party!();

    let unsigned_psbt_str = party
        .wallet
        .send_btc_begin(
            party.online,
            rcv_party.get_address(),
            1000,
            FEE_RATE,
            false,
            false,
        )
        .unwrap();
    let unsigned_psbt = Psbt::from_str(&unsigned_psbt_str).unwrap();
    let psbt_txid = unsigned_psbt.unsigned_tx.compute_txid().to_string();

    // pre-abort: reservation + wallet_transaction exist
    assert_eq!(party.wallet.list_pending_vanilla_txs().unwrap().len(), 1);
    let reserved_txos = party.db_reserved_txos();
    assert!(!reserved_txos.is_empty());

    // abort
    let bak_info_before = party.db_backup_info();
    party.abort_pending_vanilla_tx(&psbt_txid);
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // post-abort: reservation + wallet_transaction row both gone
    assert!(party.wallet.list_pending_vanilla_txs().unwrap().is_empty());
    let reserved_txos = party.db_reserved_txos();
    assert!(reserved_txos.is_empty());
    let maybe_wallet_tx = party.db_wallet_transaction_with_reserved_txos_by_txid(&psbt_txid);
    assert!(maybe_wallet_tx.is_none());

    // the previously-reserved UTXOs are now available again: a fresh send_btc_begin
    // re-selects them
    let unsigned_psbt_2_str = party
        .wallet
        .send_btc_begin(
            party.online,
            rcv_party.get_address(),
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

    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    // unknown txid
    let result = party.abort_pending_vanilla_tx_result(FAKE_TXID);
    assert!(matches!(result, Err(Error::CannotAbortPendingVanillaTx)));

    // a wallet_transaction row exists but has no attached reservations
    let txid = party.send_btc(&rcv_party.get_address(), 1000);
    let (_wt, reservations) = party
        .db_wallet_transaction_with_reserved_txos_by_txid(&txid)
        .expect("SendBtc wallet_transaction should exist after send_btc");
    assert!(reservations.is_empty());
    let result = party.abort_pending_vanilla_tx_result(&txid);
    assert!(matches!(result, Err(Error::CannotAbortPendingVanillaTx)));
}
