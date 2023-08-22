use super::*;

#[test]
fn success() {
    initialize();

    // up_to version with 0 allocatable UTXOs
    println!("\n=== up_to true, 0 allocatable");
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), true, None, None, FEE_RATE);
    assert_eq!(num_utxos_created, UTXO_NUM);
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);

    // up_to version with allocatable UTXOs partially available (1 missing)
    println!("\n=== up_to true, need to create 1 more");
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        online.clone(),
        true,
        Some(UTXO_NUM + 1),
        None,
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), (UTXO_NUM + 2) as usize);

    // forced version always creates UTXOs
    println!("\n=== up_to false");
    let num_utxos_created = test_create_utxos_default(&mut wallet, online);
    assert_eq!(num_utxos_created, UTXO_NUM);
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), (UTXO_NUM * 2 + 2) as usize);
}

#[test]
fn up_to_allocation_checks() {
    initialize();

    let amount = 66;

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // MAX_ALLOCATIONS_PER_UTXO failed allocations
    //  - check unspent counted as allocatable
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), false, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);
    let mut blinded_utxos: Vec<String> = vec![];
    let mut txo_list: HashSet<DbTxo> = HashSet::new();
    for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
        let receive_data = wallet
            .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
            .unwrap();
        let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
        let coloring = get_test_coloring(&wallet, transfer.asset_transfer_idx);
        let txo = get_test_txo(&wallet, coloring.txo_idx);
        blinded_utxos.push(receive_data.recipient_id);
        txo_list.insert(txo);
    }
    // check all blinds are on the same UTXO + fail all of them
    assert_eq!(txo_list.len(), 1);
    for blinded_utxo in blinded_utxos {
        assert!(wallet
            .fail_transfers(online.clone(), Some(blinded_utxo), None, false)
            .unwrap());
    }
    // request 1 new UTXO, expecting the existing one is still allocatable
    let result = wallet.create_utxos(online.clone(), true, Some(1), None, FEE_RATE);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), 2);

    drain_wallet(&wallet, online.clone());
    fund_wallet(wallet.get_address());
    wallet._sync_db_txos().unwrap();

    // MAX_ALLOCATIONS_PER_UTXO allocations
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), true, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);
    // create MAX_ALLOCATIONS_PER_UTXO blinds on the same UTXO
    let mut txo_list: HashSet<DbTxo> = HashSet::new();
    for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
        let receive_data = wallet
            .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
            .unwrap();
        let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
        let coloring = get_test_coloring(&wallet, transfer.asset_transfer_idx);
        let txo = get_test_txo(&wallet, coloring.txo_idx);
        txo_list.insert(txo);
    }
    assert_eq!(txo_list.len(), 1);
    // request 1 new UTXO, expecting one is created
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), true, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), 3);

    if MAX_ALLOCATIONS_PER_UTXO > 2 {
        drain_wallet(&wallet, online.clone());
        fund_wallet(wallet.get_address());
        fund_wallet(rcv_wallet.get_address());

        let num_utxos_created =
            test_create_utxos(&mut wallet, online.clone(), true, Some(2), None, FEE_RATE);
        assert_eq!(num_utxos_created, 2);
        let num_utxos_created = test_create_utxos(
            &mut rcv_wallet,
            rcv_online.clone(),
            true,
            Some(1),
            None,
            FEE_RATE,
        );
        assert_eq!(num_utxos_created, 1);
        // issue
        let asset = wallet
            .issue_asset_rgb20(
                online.clone(),
                TICKER.to_string(),
                NAME.to_string(),
                PRECISION,
                vec![AMOUNT],
            )
            .unwrap();
        // send
        let receive_data = rcv_wallet
            .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
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

        // - wait counterparty
        // UTXO 1 (input) locked, UTXO 2 (change) has at least 1 free allocation
        show_unspent_colorings(&wallet, "sender after send - WaitingCounterparty");
        let num_utxos_created =
            test_create_utxos(&mut wallet, online.clone(), true, Some(2), None, FEE_RATE);
        assert_eq!(num_utxos_created, 1);
        // UTXO 1 (blind) has at least 1 free allocation
        show_unspent_colorings(&rcv_wallet, "receiver after send - WaitingCounterparty");
        let result = rcv_wallet.create_utxos(rcv_online.clone(), true, Some(1), None, FEE_RATE);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

        // - wait confirmations
        stop_mining();
        rcv_wallet
            .refresh(rcv_online.clone(), None, vec![])
            .unwrap();
        wallet
            .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
            .unwrap();
        // UTXO 1 now spent, UTXO 2 (RGB change) has at least 1 free allocation, UTXO 3 is empty, UTXO 4 (BTC change) is empty
        show_unspent_colorings(&wallet, "sender after send - WaitingConfirmations");
        let result = wallet.create_utxos(online.clone(), true, Some(3), None, FEE_RATE);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
        // UTXO 1 (blind) has at least 1 free allocation
        show_unspent_colorings(&rcv_wallet, "receiver after send - WaitingConfirmations");
        let result = rcv_wallet.create_utxos(rcv_online.clone(), true, Some(1), None, FEE_RATE);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

        // - settled
        mine(true);
        rcv_wallet
            .refresh(rcv_online.clone(), None, vec![])
            .unwrap();
        wallet
            .refresh(online.clone(), Some(asset.asset_id), vec![])
            .unwrap();
        // UTXO 1 now spent, UTXO 2 (RGB change) has at least 1 free allocation, UTXOs 3-4 are empty
        show_unspent_colorings(&wallet, "sender after send - Settled");
        let num_utxos_created =
            test_create_utxos(&mut wallet, online, true, Some(4), None, FEE_RATE);
        assert_eq!(num_utxos_created, 1);
        // UTXO 1 (blind) has at least 1 free allocation
        show_unspent_colorings(&rcv_wallet, "receiver after send - Settled");
        let result = rcv_wallet.create_utxos(rcv_online, true, Some(1), None, FEE_RATE);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
    }
}

#[test]
fn fail() {
    initialize();

    // cannot create UTXOs for an empty wallet
    let (mut wallet, online) = get_empty_wallet!();
    let result = wallet.create_utxos(online.clone(), true, None, None, FEE_RATE);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    fund_wallet(wallet.get_address());
    test_create_utxos_default(&mut wallet, online.clone());

    // don't create UTXOs if enough allocations are already available
    let result = wallet.create_utxos(online.clone(), true, None, None, FEE_RATE);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

    // fee min/max
    let result = wallet.create_utxos_begin(online.clone(), false, Some(1), None, 0.9);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));
    let result = wallet.create_utxos_begin(online, false, Some(1), None, 1000.1);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_HIGH));
}
