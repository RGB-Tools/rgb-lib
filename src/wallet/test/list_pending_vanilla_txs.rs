use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_empty_wallet!();
    for _ in 0..3 {
        fund_wallet(test_get_address(&mut wallet));
    }
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    // empty to start
    assert!(wallet.list_pending_vanilla_txs().unwrap().is_empty());

    // one send_btc_begin(dry_run=false) creates a SendBtc pending entry
    let send_psbt_str = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
            1000,
            FEE_RATE,
            false,
            false,
        )
        .unwrap();
    let send_psbt = Psbt::from_str(&send_psbt_str).unwrap();
    let send_txid = send_psbt.unsigned_tx.compute_txid().to_string();
    let pending = wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].txid, send_txid);
    assert_eq!(pending[0].r#type, WalletTransactionType::SendBtc);

    // a concurrent create_utxos_begin(dry_run=false) adds a second pending entry
    // with CreateUtxos type
    let create_psbt_str = wallet
        .create_utxos_begin(online, false, Some(1), None, FEE_RATE, true, false)
        .unwrap();
    let create_psbt = Psbt::from_str(&create_psbt_str).unwrap();
    let create_txid = create_psbt.unsigned_tx.compute_txid().to_string();
    let pending = wallet.list_pending_vanilla_txs().unwrap();
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
    let signed = wallet.sign_psbt(send_psbt_str, None).unwrap();
    let _ = wallet.send_btc_end(online, signed, false).unwrap();
    let pending = wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].r#type, WalletTransactionType::CreateUtxos);
    assert_eq!(pending[0].txid, create_txid);
}
