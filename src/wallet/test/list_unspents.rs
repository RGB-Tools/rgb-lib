use super::*;

#[test]
fn success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_empty_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // no unspents
    let unspent_list_settled = wallet.list_unspents(None, true).unwrap();
    assert_eq!(unspent_list_settled.len(), 0);
    let unspent_list_all = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspent_list_all.len(), 0);

    fund_wallet(wallet.get_address());
    mine(false);

    // one (settled) unspent, no RGB allocations
    wallet._sync_db_txos().unwrap();
    let unspent_list_settled = wallet.list_unspents(None, true).unwrap();
    assert_eq!(unspent_list_settled.len(), 1);
    let unspent_list_all = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspent_list_all.len(), 1);
    assert!(unspent_list_all.iter().all(|u| !u.utxo.colorable));

    test_create_utxos_default(&mut wallet, online.clone());

    // multiple unspents, one settled RGB allocation
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let unspent_list_settled = wallet.list_unspents(None, true).unwrap();
    assert_eq!(unspent_list_settled.len(), UTXO_NUM as usize + 1);
    let unspent_list_all = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspent_list_all.len(), UTXO_NUM as usize + 1);
    assert_eq!(
        unspent_list_all.iter().filter(|u| u.utxo.colorable).count(),
        UTXO_NUM as usize
    );
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| !u.utxo.colorable)
            .count(),
        1
    );
    let mut settled_allocations = vec![];
    unspent_list_settled
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(settled_allocations.len(), 1);
    assert!(settled_allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone()) && a.amount == AMOUNT && a.settled));

    // multiple unspents, one failed blind, not listed
    let receive_data_fail = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    rcv_wallet
        .fail_transfers(
            rcv_online.clone(),
            Some(receive_data_fail.recipient_id),
            None,
            false,
        )
        .unwrap();
    show_unspent_colorings(&rcv_wallet, "after blind fail");
    let unspent_list_all = rcv_wallet.list_unspents(None, false).unwrap();
    let mut allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations.len(), 0);
    // one failed send, not listed
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());
    wallet
        .fail_transfers(online.clone(), None, Some(txid), false)
        .unwrap();
    show_unspent_colorings(&wallet, "after send fail");
    let unspent_list_all = wallet.list_unspents(None, false).unwrap();
    assert_eq!(
        unspent_list_all.iter().filter(|u| u.utxo.colorable).count(),
        UTXO_NUM as usize
    );
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| !u.utxo.colorable)
            .count(),
        1
    );
    let mut allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations.len(), 1);
    assert!(allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone()) && a.amount == AMOUNT && a.settled));

    drain_wallet(&wallet, online.clone());
    fund_wallet(wallet.get_address());
    mine(false);
    test_create_utxos_default(&mut wallet, online.clone());
    drain_wallet(&rcv_wallet, rcv_online.clone());
    fund_wallet(rcv_wallet.get_address());
    mine(false);
    test_create_utxos_default(&mut rcv_wallet, rcv_online.clone());

    // issue + send some asset
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
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());
    show_unspent_colorings(&rcv_wallet, "receiver after send - WaitingCounterparty");
    show_unspent_colorings(&wallet, "sender after send - WaitingCounterparty");
    // check receiver lists no settled allocations
    let rcv_unspent_list = rcv_wallet.list_unspents(None, true).unwrap();
    assert!(!rcv_unspent_list
        .iter()
        .any(|u| !u.rgb_allocations.is_empty()));
    // check receiver lists one pending blind
    let rcv_unspent_list_all = rcv_wallet.list_unspents(None, false).unwrap();
    let mut allocations = vec![];
    rcv_unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert!(!allocations.iter().any(|a| a.settled));
    assert_eq!(allocations.iter().filter(|a| !a.settled).count(), 1);
    // check sender lists one settled issue
    let unspent_list_settled = wallet.list_unspents(None, true).unwrap();
    let mut settled_allocations = vec![];
    unspent_list_settled
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(settled_allocations.len(), 1);
    assert!(settled_allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone()) && a.amount == AMOUNT && a.settled));
    // check sender lists one pending change
    let unspent_list_all = wallet.list_unspents(None, false).unwrap();
    assert_eq!(
        unspent_list_all.iter().filter(|u| u.utxo.colorable).count(),
        UTXO_NUM as usize
    );
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| !u.utxo.colorable)
            .count(),
        1
    );
    let mut pending_allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| pending_allocations.extend(u.rgb_allocations.iter().filter(|a| !a.settled)));
    assert_eq!(pending_allocations.len(), 1);
    assert!(pending_allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone()) && a.amount == AMOUNT - amount));

    stop_mining();

    // transfer progresses to status WaitingConfirmations
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    show_unspent_colorings(&rcv_wallet, "receiver after send - WaitingConfirmations");
    show_unspent_colorings(&wallet, "sender after send - WaitingConfirmations");
    // check receiver lists no settled allocations
    let rcv_unspent_list = rcv_wallet.list_unspents(None, true).unwrap();
    assert!(!rcv_unspent_list
        .iter()
        .any(|u| !u.rgb_allocations.is_empty()));
    // check receiver lists one pending blind
    let rcv_unspent_list_all = rcv_wallet.list_unspents(None, false).unwrap();
    let mut allocations = vec![];
    rcv_unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert!(!allocations.iter().any(|a| a.settled));
    assert_eq!(allocations.iter().filter(|a| !a.settled).count(), 1);
    assert!(allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone()) && a.amount == amount));
    // check sender lists one settled issue
    let unspent_list_settled = wallet.list_unspents(None, true).unwrap();
    let mut settled_allocations = vec![];
    unspent_list_settled
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(settled_allocations.len(), 1);
    assert!(settled_allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone()) && a.amount == AMOUNT && a.settled));
    // check sender lists one pending change
    let unspent_list_all = wallet.list_unspents(None, false).unwrap();
    let mut pending_allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| pending_allocations.extend(u.rgb_allocations.iter().filter(|a| !a.settled)));
    assert_eq!(pending_allocations.len(), 1);
    assert!(pending_allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone()) && a.amount == AMOUNT - amount));

    // transfer progresses to status Settled
    mine(true);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online, Some(asset.asset_id.clone()), vec![])
        .unwrap();
    show_unspent_colorings(&rcv_wallet, "receiver after send - Settled");
    show_unspent_colorings(&wallet, "sender after send - Settled");
    // check receiver lists one settled allocation
    let rcv_unspent_list = rcv_wallet.list_unspents(None, true).unwrap();
    let mut settled_allocations = vec![];
    rcv_unspent_list
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert!(settled_allocations.iter().all(|a| a.settled));
    assert_eq!(settled_allocations.len(), 1);
    assert!(settled_allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone()) && a.amount == amount));
    // check receiver lists no pending allocations
    let rcv_unspent_list_all = rcv_wallet.list_unspents(None, false).unwrap();
    let mut allocations = vec![];
    rcv_unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations, settled_allocations);
    // check sender lists one settled change
    let unspent_list_settled = wallet.list_unspents(None, true).unwrap();
    let mut settled_allocations = vec![];
    unspent_list_settled
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(settled_allocations.len(), 1);
    assert!(settled_allocations
        .iter()
        .all(|a| a.asset_id == Some(asset.asset_id.clone())
            && a.amount == AMOUNT - amount
            && a.settled));
    // check sender lists no pending allocations
    let unspent_list_all = wallet.list_unspents(None, false).unwrap();
    let mut allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations, settled_allocations);
}
