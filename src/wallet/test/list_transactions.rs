use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    mine(false);
    // don't sync wallet without online
    let transactions = wallet.list_transactions(None).unwrap();
    let rcv_transactions = wallet.list_transactions(None).unwrap();
    assert_eq!(transactions.len(), 2);
    assert_eq!(rcv_transactions.len(), 2);
    assert!(transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::User)));
    assert!(transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::CreateUtxos)));
    assert!(rcv_transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::User)));
    assert!(rcv_transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::CreateUtxos)));
    assert!(transactions.iter().all(|t| t.confirmation_time.is_none()));
    assert!(rcv_transactions
        .iter()
        .all(|t| t.confirmation_time.is_none()));
    // sync wallet when online is provided
    let transactions = wallet.list_transactions(Some(online.clone())).unwrap();
    let rcv_transactions = rcv_wallet
        .list_transactions(Some(rcv_online.clone()))
        .unwrap();
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
    assert!(rcv_transactions
        .iter()
        .all(|t| t.confirmation_time.is_some()));

    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let receive_data = rcv_wallet
        .witness_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            recipient_data: RecipientData::WitnessData {
                script_buf: ScriptBuf::from_hex(&receive_data.recipient_id).unwrap(),
                amount_sat: 1000,
                blinding: None,
            },
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send_default(&mut wallet, &online, recipient_map);
    // settle the transfer so the tx gets broadcasted and receiver sees the new UTXO
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet.refresh(online.clone(), None, vec![]).unwrap();
    mine(false);
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet.refresh(online.clone(), None, vec![]).unwrap();
    let transactions = wallet.list_transactions(Some(online.clone())).unwrap();
    let rcv_transactions = rcv_wallet
        .list_transactions(Some(rcv_online.clone()))
        .unwrap();
    assert_eq!(transactions.len(), 3);
    assert_eq!(rcv_transactions.len(), 3);
    assert!(transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::RgbSend)));
    assert!(rcv_transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::RgbSend)));
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
    assert!(rcv_transactions
        .iter()
        .all(|t| t.confirmation_time.is_some()));

    drain_wallet(&wallet, online.clone());
    mine(false);
    let transactions = wallet.list_transactions(Some(online)).unwrap();
    assert_eq!(transactions.len(), 4);
    assert!(transactions
        .iter()
        .any(|t| matches!(t.transaction_type, TransactionType::Drain)));
    assert!(transactions.iter().all(|t| t.confirmation_time.is_some()));
}
