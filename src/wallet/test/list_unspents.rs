use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_empty_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // no unspents
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_before.is_none());
    let unspent_list_settled = test_list_unspents(&mut wallet, None, true);
    let bak_info_after = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_after.is_none());
    assert_eq!(unspent_list_settled.len(), 0);
    let unspent_list_all = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspent_list_all.len(), 0);

    fund_wallet(test_get_address(&mut wallet));
    mine(false, false);

    // one unspent, no RGB allocations
    let unspent_list_settled = test_list_unspents(&mut wallet, Some(&online), true);
    assert_eq!(unspent_list_settled.len(), 1);
    let unspent_list_all = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspent_list_all.len(), 1);
    assert!(unspent_list_all.iter().all(|u| !u.utxo.colorable));

    test_create_utxos_default(&mut wallet, &online);

    // multiple unspents, one settled RGB allocation
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let unspent_list_settled = test_list_unspents(&mut wallet, None, true);
    assert_eq!(unspent_list_settled.len(), UTXO_NUM as usize + 1);
    let unspent_list_all = test_list_unspents(&mut wallet, None, false);
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
    assert!(
        settled_allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == AMOUNT
                } else {
                    false
                }
                && a.settled)
    );

    // multiple unspents, one failed blind, not listed
    let receive_data_fail = test_blind_receive(&rcv_wallet);
    test_fail_transfers_single(
        &mut rcv_wallet,
        &rcv_online,
        receive_data_fail.batch_transfer_idx,
    );
    show_unspent_colorings(&mut rcv_wallet, "after blind fail");
    let unspent_list_all = test_list_unspents(&mut rcv_wallet, None, false);
    let mut allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations.len(), 0);
    // one failed send, not listed
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = test_send_result(&mut wallet, &online, &recipient_map).unwrap();
    let txid = send_result.txid;
    assert!(!txid.is_empty());
    test_fail_transfers_single(&mut wallet, &online, send_result.batch_transfer_idx);
    show_unspent_colorings(&mut wallet, "after send fail");
    let unspent_list_all = test_list_unspents(&mut wallet, None, false);
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| u.utxo.colorable && u.utxo.exists)
            .count(),
        UTXO_NUM as usize
    );
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| u.utxo.colorable && !u.utxo.exists)
            .count(),
        1
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
    assert!(
        allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == AMOUNT
                } else {
                    false
                }
                && a.settled)
    );

    // new wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue + send some asset
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    show_unspent_colorings(&mut rcv_wallet, "receiver after send - WaitingCounterparty");
    show_unspent_colorings(&mut wallet, "sender after send - WaitingCounterparty");
    // check receiver lists no settled allocations
    let rcv_unspent_list = test_list_unspents(&mut rcv_wallet, None, true);
    assert!(
        !rcv_unspent_list
            .iter()
            .any(|u| !u.rgb_allocations.is_empty())
    );
    // check receiver lists one pending blind
    let rcv_unspent_list_all = test_list_unspents(&mut rcv_wallet, None, false);
    let mut allocations = vec![];
    rcv_unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert!(!allocations.iter().any(|a| a.settled));
    assert_eq!(
        rcv_unspent_list_all
            .iter()
            .filter(|u| u.pending_blinded > 0)
            .count(),
        1
    );
    assert_eq!(
        rcv_unspent_list_all
            .iter()
            .filter(|u| u.pending_blinded == 1)
            .count(),
        1
    );
    // check sender lists one settled issue
    let unspent_list_settled = test_list_unspents(&mut wallet, None, true);
    let mut settled_allocations = vec![];
    unspent_list_settled
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(settled_allocations.len(), 1);
    assert!(
        settled_allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == AMOUNT
                } else {
                    false
                }
                && a.settled)
    );
    // check sender lists one pending change (exists = false) + 1 settled issue
    let unspent_list_all = test_list_unspents(&mut wallet, None, false);
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| u.utxo.colorable && u.utxo.exists)
            .count(),
        UTXO_NUM as usize
    );
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| u.utxo.colorable && !u.utxo.exists)
            .count(),
        1
    );
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| !u.utxo.colorable)
            .count(),
        1
    );
    let mut pending_allocations = vec![];
    let mut settled_allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| pending_allocations.extend(u.rgb_allocations.iter().filter(|a| !a.settled)));
    assert_eq!(pending_allocations.len(), 1);
    assert!(
        pending_allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == AMOUNT - amount
                } else {
                    false
                })
    );
    unspent_list_all
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.iter().filter(|a| a.settled)));
    assert_eq!(settled_allocations.len(), 1);
    assert!(
        settled_allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == AMOUNT
                } else {
                    false
                })
    );

    // transfer progresses to status WaitingConfirmations
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    show_unspent_colorings(
        &mut rcv_wallet,
        "receiver after send - WaitingConfirmations",
    );
    show_unspent_colorings(&mut wallet, "sender after send - WaitingConfirmations");
    // check receiver lists no settled allocations
    let rcv_unspent_list = test_list_unspents(&mut rcv_wallet, None, true);
    assert!(
        !rcv_unspent_list
            .iter()
            .any(|u| !u.rgb_allocations.is_empty())
    );
    // check receiver lists one pending blind
    let rcv_unspent_list_all = test_list_unspents(&mut rcv_wallet, None, false);
    let mut allocations = vec![];
    rcv_unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert!(!allocations.iter().any(|a| a.settled));
    assert_eq!(allocations.iter().filter(|a| !a.settled).count(), 1);
    assert!(
        allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == amount
                } else {
                    false
                })
    );
    assert!(rcv_unspent_list_all.iter().all(|u| u.pending_blinded == 0));
    // check sender lists one settled issue
    let unspent_list_settled = test_list_unspents(&mut wallet, None, true);
    let mut settled_allocations = vec![];
    unspent_list_settled
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(settled_allocations.len(), 1);
    assert!(
        settled_allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == AMOUNT
                } else {
                    false
                }
                && a.settled)
    );
    // check sender lists one pending change (exists = true)
    let unspent_list_all = test_list_unspents(&mut wallet, None, false);
    let mut pending_allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| pending_allocations.extend(u.rgb_allocations.iter().filter(|a| !a.settled)));
    assert_eq!(pending_allocations.len(), 1);
    assert!(
        pending_allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == AMOUNT - amount
                } else {
                    false
                })
    );
    assert_eq!(
        unspent_list_all
            .iter()
            .filter(|u| u.utxo.colorable && !u.utxo.exists)
            .count(),
        0
    );

    // transfer progresses to status Settled
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    show_unspent_colorings(&mut rcv_wallet, "receiver after send - Settled");
    show_unspent_colorings(&mut wallet, "sender after send - Settled");
    // check receiver lists one settled allocation
    let rcv_unspent_list = test_list_unspents(&mut rcv_wallet, None, true);
    let mut settled_allocations = vec![];
    rcv_unspent_list
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert!(settled_allocations.iter().all(|a| a.settled));
    assert_eq!(settled_allocations.len(), 1);
    assert!(
        settled_allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == amount
                } else {
                    false
                })
    );
    // check receiver lists no pending allocations
    let rcv_unspent_list_all = test_list_unspents(&mut rcv_wallet, None, false);
    let mut allocations = vec![];
    rcv_unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations, settled_allocations);
    // check sender lists one settled change
    let unspent_list_settled = test_list_unspents(&mut wallet, None, true);
    let mut settled_allocations = vec![];
    unspent_list_settled
        .iter()
        .for_each(|u| settled_allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(settled_allocations.len(), 1);
    assert!(
        settled_allocations
            .iter()
            .all(|a| a.asset_id == Some(asset.asset_id.clone())
                && if let Assignment::Fungible(amt) = a.assignment {
                    amt == AMOUNT - amount
                } else {
                    false
                }
                && a.settled)
    );
    // check sender lists no pending allocations
    let unspent_list_all = test_list_unspents(&mut wallet, None, false);
    let mut allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations, settled_allocations);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let (mut wallet, online) = get_empty_wallet!();

    fund_wallet(test_get_address(&mut wallet));

    // no unspents if skipping sync
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 0);

    // 1 unspent after manually syncing
    wallet.sync(online.clone()).unwrap();
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 1);
}
