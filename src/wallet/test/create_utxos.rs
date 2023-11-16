use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    // up_to version with 0 allocatable UTXOs
    println!("\n=== up_to true, 0 allocatable");
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let num_utxos_created = test_create_utxos(&mut wallet, &online, true, None, None, FEE_RATE);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert_eq!(num_utxos_created, UTXO_NUM);
    let unspents = test_list_unspents(&wallet, None, false);
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);

    // up_to version with allocatable UTXOs partially available (1 missing)
    println!("\n=== up_to true, need to create 1 more");
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        &online,
        true,
        Some(UTXO_NUM + 1),
        None,
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);
    let unspents = test_list_unspents(&wallet, None, false);
    assert_eq!(unspents.len(), (UTXO_NUM + 2) as usize);

    // forced version always creates UTXOs
    println!("\n=== up_to false");
    let num_utxos_created = test_create_utxos_default(&mut wallet, &online);
    assert_eq!(num_utxos_created, UTXO_NUM);
    let unspents = test_list_unspents(&wallet, None, false);
    assert_eq!(unspents.len(), (UTXO_NUM * 2 + 2) as usize);
}

#[test]
#[parallel]
fn up_to_allocation_checks() {
    initialize();

    let amount = 66;

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // MAX_ALLOCATIONS_PER_UTXO failed allocations
    //  - check unspent counted as allocatable
    let num_utxos_created = test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);
    let mut blinded_utxos: Vec<String> = vec![];
    let mut txo_list: HashSet<DbTxo> = HashSet::new();
    for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
        let receive_data = test_blind_receive(&mut wallet);
        let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
        let coloring = get_test_coloring(&wallet, transfer.asset_transfer_idx);
        let txo = get_test_txo(&wallet, coloring.txo_idx);
        blinded_utxos.push(receive_data.recipient_id);
        txo_list.insert(txo);
    }
    // check all blinds are on the same UTXO + fail all of them
    assert_eq!(txo_list.len(), 1);
    for blinded_utxo in blinded_utxos {
        assert!(test_fail_transfers_blind(
            &mut wallet,
            &online,
            &blinded_utxo
        ));
    }
    // request 1 new UTXO, expecting the existing one is still allocatable
    let result = wallet.create_utxos(online.clone(), true, Some(1), None, FEE_RATE);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
    let unspents = test_list_unspents(&wallet, None, false);
    assert_eq!(unspents.len(), 2);

    drain_wallet(&wallet, &online);
    fund_wallet(test_get_address(&wallet));

    // MAX_ALLOCATIONS_PER_UTXO allocations
    let num_utxos_created = test_create_utxos(&mut wallet, &online, true, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);
    // create MAX_ALLOCATIONS_PER_UTXO blinds on the same UTXO
    let mut txo_list: HashSet<DbTxo> = HashSet::new();
    for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
        let receive_data = test_blind_receive(&mut wallet);
        let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
        let coloring = get_test_coloring(&wallet, transfer.asset_transfer_idx);
        let txo = get_test_txo(&wallet, coloring.txo_idx);
        txo_list.insert(txo);
    }
    assert_eq!(txo_list.len(), 1);
    // request 1 new UTXO, expecting one is created
    let num_utxos_created = test_create_utxos(&mut wallet, &online, true, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);
    let unspents = test_list_unspents(&wallet, None, false);
    assert_eq!(unspents.len(), 3);

    if MAX_ALLOCATIONS_PER_UTXO > 2 {
        drain_wallet(&wallet, &online);
        fund_wallet(test_get_address(&wallet));
        fund_wallet(test_get_address(&rcv_wallet));

        let num_utxos_created =
            test_create_utxos(&mut wallet, &online, true, Some(2), None, FEE_RATE);
        assert_eq!(num_utxos_created, 2);
        let num_utxos_created =
            test_create_utxos(&mut rcv_wallet, &rcv_online, true, Some(1), None, FEE_RATE);
        assert_eq!(num_utxos_created, 1);
        // issue
        let asset = test_issue_asset_nia(&mut wallet, &online, None);
        // send
        let receive_data = test_blind_receive(&mut rcv_wallet);
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
        let txid = test_send(&mut wallet, &online, &recipient_map);
        assert!(!txid.is_empty());

        // - wait counterparty
        // UTXO 1 (input) locked, UTXO 2 (change) has at least 1 free allocation
        show_unspent_colorings(&wallet, "sender after send - WaitingCounterparty");
        let num_utxos_created =
            test_create_utxos(&mut wallet, &online, true, Some(2), None, FEE_RATE);
        assert_eq!(num_utxos_created, 1);
        // UTXO 1 (blind) has at least 1 free allocation
        show_unspent_colorings(&rcv_wallet, "receiver after send - WaitingCounterparty");
        let result = rcv_wallet.create_utxos(rcv_online.clone(), true, Some(1), None, FEE_RATE);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

        // - wait confirmations
        stop_mining();
        test_refresh_all(&mut rcv_wallet, &rcv_online);
        test_refresh_asset(&mut wallet, &online, &asset.asset_id);
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
        test_refresh_all(&mut rcv_wallet, &rcv_online);
        test_refresh_asset(&mut wallet, &online, &asset.asset_id);
        // UTXO 1 now spent, UTXO 2 (RGB change) has at least 1 free allocation, UTXOs 3-4 are empty
        show_unspent_colorings(&wallet, "sender after send - Settled");
        let num_utxos_created =
            test_create_utxos(&mut wallet, &online, true, Some(4), None, FEE_RATE);
        assert_eq!(num_utxos_created, 1);
        // UTXO 1 (blind) has at least 1 free allocation
        show_unspent_colorings(&rcv_wallet, "receiver after send - Settled");
        let result = rcv_wallet.create_utxos(rcv_online, true, Some(1), None, FEE_RATE);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
    }
}

#[test]
#[parallel]
fn fail() {
    initialize();

    // cannot create UTXOs for an empty wallet
    let (mut wallet, online) = get_empty_wallet!();
    let result = wallet.create_utxos(online.clone(), true, None, None, FEE_RATE);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: n,
            available: a
        }) if n == (UTXO_SIZE as u64 * UTXO_NUM as u64) + 1000 && a == 0
    ));

    fund_wallet(test_get_address(&wallet));
    test_create_utxos_default(&mut wallet, &online);

    // don't create UTXOs if enough allocations are already available
    let result = wallet.create_utxos(online.clone(), true, None, None, FEE_RATE);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

    // fee min/max
    let result = test_create_utxos_begin_result(&mut wallet, &online, false, Some(1), None, 0.9);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));
    let result = test_create_utxos_begin_result(&mut wallet, &online, false, Some(1), None, 1000.1);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_HIGH));
}
