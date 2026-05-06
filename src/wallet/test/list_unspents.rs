use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_empty_party!();
    let mut rcv_party = get_funded_party!();

    // no unspents
    let bak_info_before = party.db_backup_info_opt();
    assert!(bak_info_before.is_none());
    let unspent_list_settled = party.list_unspents(true);
    let bak_info_after = party.db_backup_info_opt();
    assert!(bak_info_after.is_none());
    assert_eq!(unspent_list_settled.len(), 0);
    let unspent_list_all = party.list_unspents(false);
    assert_eq!(unspent_list_all.len(), 0);

    fund_wallet(party.get_address());
    mine(false);

    // one unspent, no RGB allocations
    let unspent_list_settled = party.list_unspents_with_sync(true);
    assert_eq!(unspent_list_settled.len(), 1);
    let unspent_list_all = party.list_unspents(false);
    assert_eq!(unspent_list_all.len(), 1);
    assert!(unspent_list_all.iter().all(|u| !u.utxo.colorable));

    party.create_utxos_default();

    // multiple unspents, one settled RGB allocation
    let asset = party.issue_asset_nia(None);
    let unspent_list_settled = party.list_unspents(true);
    assert_eq!(unspent_list_settled.len(), UTXO_NUM as usize + 1);
    let unspent_list_all = party.list_unspents(false);
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
    let receive_data_fail = rcv_party.blind_receive();
    rcv_party.fail_transfers_single(receive_data_fail.batch_transfer_idx);
    rcv_party.show_unspent_colorings("after blind fail");
    let unspent_list_all = rcv_party.list_unspents(false);
    let mut allocations = vec![];
    unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations.len(), 0);
    // one failed send, not listed
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = party.send_result(&recipient_map).unwrap();
    let txid = send_result.txid;
    assert!(!txid.is_empty());
    party.fail_transfers_single(send_result.batch_transfer_idx);
    party.show_unspent_colorings("after send fail");
    let unspent_list_all = party.list_unspents(false);
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
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue + send some asset
    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    rcv_party.show_unspent_colorings("receiver after send - WaitingCounterparty");
    party.show_unspent_colorings("sender after send - WaitingCounterparty");
    // check receiver lists no settled allocations
    let rcv_unspent_list = rcv_party.list_unspents(true);
    assert!(
        !rcv_unspent_list
            .iter()
            .any(|u| !u.rgb_allocations.is_empty())
    );
    // check receiver lists one pending blind
    let rcv_unspent_list_all = rcv_party.list_unspents(false);
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
    let unspent_list_settled = party.list_unspents(true);
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
    let unspent_list_all = party.list_unspents(false);
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
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    rcv_party.show_unspent_colorings("receiver after send - WaitingConfirmations");
    party.show_unspent_colorings("sender after send - WaitingConfirmations");
    // check receiver lists no settled allocations
    let rcv_unspent_list = rcv_party.list_unspents(true);
    assert!(
        !rcv_unspent_list
            .iter()
            .any(|u| !u.rgb_allocations.is_empty())
    );
    // check receiver lists one pending blind
    let rcv_unspent_list_all = rcv_party.list_unspents(false);
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
    let unspent_list_settled = party.list_unspents(true);
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
    let unspent_list_all = party.list_unspents(false);
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
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    rcv_party.show_unspent_colorings("receiver after send - Settled");
    party.show_unspent_colorings("sender after send - Settled");
    // check receiver lists one settled allocation
    let rcv_unspent_list = rcv_party.list_unspents(true);
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
    let rcv_unspent_list_all = rcv_party.list_unspents(false);
    let mut allocations = vec![];
    rcv_unspent_list_all
        .iter()
        .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
    assert_eq!(allocations, settled_allocations);
    // check sender lists one settled change
    let unspent_list_settled = party.list_unspents(true);
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
    let unspent_list_all = party.list_unspents(false);
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

    let mut party = get_empty_party!();

    fund_wallet(party.get_address());

    // no unspents if skipping sync
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 0);

    // 1 unspent after manually syncing
    party
        .wallet
        .sync(
            party.online,
            SyncOptions {
                keychain: SyncKeychain::Vanilla {
                    lookback: INDEXER_SYNC_LOOKBACK as u32,
                },
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 1);
}
