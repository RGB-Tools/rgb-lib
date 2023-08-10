use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    let asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let blind_data = rcv_wallet
        .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo,
            amount: AMOUNT,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send_default(&mut wallet, &online, recipient_map);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet.refresh(online.clone(), None, vec![]).unwrap();
    drain_wallet(&wallet, online.clone());
    fund_wallet(wallet.get_address());
    test_create_utxos_default(&mut wallet, online.clone());

    let transactions = wallet.list_transactions(None).unwrap();
    println!("Transactions: {transactions:?}");

    let transactions = wallet.list_transactions(Some(online)).unwrap();
    println!("Transactions: {transactions:?}");
}
