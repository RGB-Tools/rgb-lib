use super::*;

#[test]
fn success() {
    initialize();

    let amount: u64 = 66;
    let pending_fake_allocation = RgbAllocation {
        asset_id: Some(s!("")),
        amount: 0,
        settled: false,
    };
    let settled_fake_allocation = RgbAllocation {
        asset_id: Some(s!("")),
        amount: 0,
        settled: false,
    };

    // no unspents
    let (wallet, _online) = get_empty_wallet!();
    let unspent_list = wallet.list_unspents(true).unwrap();
    assert_eq!(unspent_list.len(), 0);
    let unspent_list_all = wallet.list_unspents(false).unwrap();
    assert_eq!(unspent_list_all.len(), 0);

    // one (settled) unspent, no RGB allocations
    let (wallet, _online) = get_funded_noutxo_wallet!();
    wallet._sync_db_txos().unwrap();
    let unspent_list = wallet.list_unspents(true).unwrap();
    assert_eq!(unspent_list.len(), 1);
    let unspent_list_all = wallet.list_unspents(false).unwrap();
    assert_eq!(unspent_list_all.len(), 1);

    // more unspents, one with an RGB allocation
    let (mut wallet, online) = get_funded_wallet!();
    let asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let unspent_list = wallet.list_unspents(true).unwrap();
    assert_eq!(unspent_list.len(), UTXO_NUM as usize + 1);
    let unspent_list_all = wallet.list_unspents(false).unwrap();
    assert_eq!(unspent_list_all.len(), UTXO_NUM as usize + 1);
    let unspents_with_rgb_allocations: Vec<Unspent> = unspent_list
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .collect();
    assert_eq!(unspents_with_rgb_allocations.len(), 1);
    let unspent_with_settled_allocation = unspents_with_rgb_allocations.iter().find(|u| {
        u.rgb_allocations
            .iter()
            .find(|a| a.amount == AMOUNT)
            .unwrap_or(&pending_fake_allocation)
            .settled
    });
    assert!(unspent_with_settled_allocation.is_some());

    // an unspent with a pending allocation
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();
    let blind_data = rcv_wallet.blind(None, None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            blinded_utxo: blind_data.blinded_utxo,
        }],
    )]);
    let txid = wallet.send(online, recipient_map, false).unwrap();
    assert!(!txid.is_empty());
    let unspent_list = rcv_wallet.list_unspents(true).unwrap();
    assert_eq!(unspent_list.len(), UTXO_NUM as usize + 1);
    let unspent_list_all = wallet.list_unspents(false).unwrap();
    assert_eq!(unspent_list_all.len(), UTXO_NUM as usize + 1);
    let unspents_with_rgb_allocations: Vec<Unspent> = unspent_list_all
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .collect();
    assert_eq!(unspents_with_rgb_allocations.len(), 2);
    let unspent_with_settled_allocation = unspents_with_rgb_allocations.iter().find(|u| {
        u.rgb_allocations
            .iter()
            .find(|a| a.amount == AMOUNT)
            .unwrap_or(&pending_fake_allocation)
            .settled
    });
    assert!(unspent_with_settled_allocation.is_some());
    let unspent_with_pending_allocation = unspents_with_rgb_allocations.iter().find(|u| {
        !u.rgb_allocations
            .iter()
            .find(|a| a.amount == (AMOUNT - amount))
            .unwrap_or(&settled_fake_allocation)
            .settled
    });
    assert!(unspent_with_pending_allocation.is_some());
}
