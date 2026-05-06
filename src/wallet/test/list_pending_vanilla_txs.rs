use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_empty_party!();
    for _ in 0..3 {
        fund_wallet(party.get_address());
    }
    let mut rcv_party = get_empty_party!();

    // empty to start
    assert!(party.wallet.list_pending_vanilla_txs().unwrap().is_empty());

    // one send_btc_begin(dry_run=false) creates a SendBtc pending entry
    let send_psbt_str = party
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
    let send_psbt = Psbt::from_str(&send_psbt_str).unwrap();
    let send_txid = send_psbt.unsigned_tx.compute_txid().to_string();
    let pending = party.wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].txid, send_txid);
    assert_eq!(pending[0].r#type, WalletTransactionType::SendBtc);

    // a concurrent create_utxos_begin(dry_run=false) adds a second pending entry
    // with CreateUtxos type
    let create_psbt_str = party
        .wallet
        .create_utxos_begin(party.online, false, Some(1), None, FEE_RATE, true, false)
        .unwrap();
    let create_psbt = Psbt::from_str(&create_psbt_str).unwrap();
    let create_txid = create_psbt.unsigned_tx.compute_txid().to_string();
    let pending = party.wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 2);
    assert!(
        pending
            .iter()
            .any(|p| p.r#type == WalletTransactionType::SendBtc && p.txid == send_txid)
    );
    assert!(
        pending
            .iter()
            .any(|p| p.r#type == WalletTransactionType::CreateUtxos && p.txid == create_txid)
    );

    // completing the send_btc drops it from the list
    let signed = party.wallet.sign_psbt(send_psbt_str, None).unwrap();
    let _ = party.wallet.send_btc_end(party.online, signed).unwrap();
    let pending = party.wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].r#type, WalletTransactionType::CreateUtxos);
    assert_eq!(pending[0].txid, create_txid);
}
