use super::*;

#[test]
fn success() {
    initialize();

    let amount = 69;
    let expiration = 60;
    let (mut wallet, online) = get_funded_wallet!();

    // default expiration
    let now_timestamp = now().unix_timestamp();
    let blind_data = wallet.blind(None, None, None).unwrap();
    assert!(blind_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + DURATION_RCV_TRANSFER as i64;
    assert!(blind_data.expiration_timestamp.unwrap() - timestamp <= 1);

    // positive expiration
    let now_timestamp = now().unix_timestamp();
    let blind_data = wallet.blind(None, None, Some(expiration)).unwrap();
    assert!(blind_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + expiration as i64;
    assert!(blind_data.expiration_timestamp.unwrap() - timestamp <= 1);

    // 0 expiration
    let blind_data = wallet.blind(None, None, Some(0)).unwrap();
    assert!(blind_data.expiration_timestamp.is_none());

    // asset id is set
    let asset = wallet
        .issue_asset_rgb20(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_id = asset.asset_id;
    let result = wallet.blind(Some(asset_id.clone()), None, None);
    assert!(result.is_ok());

    // all set
    let now_timestamp = now().unix_timestamp();
    let result = wallet.blind(Some(asset_id.clone()), Some(amount), Some(expiration));
    assert!(result.is_ok());
    let blind_data = result.unwrap();

    // Invoice checks
    let invoice = Invoice::new(blind_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    let invoice_from_data = Invoice::from_invoice_data(invoice_data.clone()).unwrap();
    let approx_expiry = now_timestamp + expiration as i64;
    assert_eq!(invoice.bech32_invoice(), invoice_from_data.bech32_invoice());
    assert_eq!(invoice_data.blinded_utxo, blind_data.blinded_utxo);
    assert_eq!(invoice_data.asset_id, Some(asset_id));
    assert_eq!(invoice_data.amount, Some(amount));
    assert!(invoice_data.expiration_timestamp.unwrap() - approx_expiry <= 1);

    // check BlindedUTXO
    let result = BlindedUTXO::new(blind_data.blinded_utxo);
    assert!(result.is_ok());
}

#[test]
fn respect_max_allocations() {
    initialize();

    let (mut wallet, _online) = get_funded_wallet!();

    let available_allocations = UTXO_NUM as u32 * MAX_ALLOCATIONS_PER_UTXO;
    let mut created_allocations = 0;
    for _ in 0..UTXO_NUM {
        let mut txo_list: HashSet<DbTxo> = HashSet::new();
        for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
            let blind_data = wallet.blind(None, None, None).unwrap();
            created_allocations += 1;
            let transfer = get_test_transfer_recipient(&wallet, &blind_data.blinded_utxo);
            let coloring = get_test_coloring(&wallet, transfer.asset_transfer_idx);
            let txo = get_test_txo(&wallet, coloring.txo_idx);
            txo_list.insert(txo);
        }

        // check allocations have been equally distributed between UTXOs
        assert_eq!(txo_list.len(), UTXO_NUM as usize);
    }
    assert_eq!(available_allocations, created_allocations);

    let result = wallet.blind(None, None, None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[test]
fn expire() {
    initialize();

    let expiration = 1;
    let (mut wallet, online) = get_funded_wallet!();

    // check expiration
    let now_timestamp = now().unix_timestamp();
    let blind_data_1 = wallet.blind(None, None, Some(expiration)).unwrap();
    let timestamp = now_timestamp + expiration as i64;
    assert!(blind_data_1.expiration_timestamp.unwrap() - timestamp <= 1);

    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration as u64 * 1000 + 2000,
    ));

    // trigger the expiration of pending transfers
    let _asset = wallet
        .issue_asset_rgb20(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // check transfer is now in status Failed
    let transfer = get_test_transfer_recipient(&wallet, &blind_data_1.blinded_utxo);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(transfer_data.status, TransferStatus::Failed);
}

#[test]
fn pending_outgoing_transfer_fail() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

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
    let asset_id = asset.asset_id;
    // get issuance UTXO
    let unspents = wallet.list_unspents(false).unwrap();
    let unspent_issue = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_id.clone()))
        })
        .unwrap();
    // send
    let blind_data = rcv_wallet.blind(None, None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo,
            amount,
        }],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());

    // check blind doesn't get allocated to UTXO being spent
    let blind_data = wallet.blind(None, None, None).unwrap();
    show_unspent_colorings(&wallet, "after 1st blind");
    let unspents = wallet.list_unspents(false).unwrap();
    let unspent_blind_1 = unspents
        .iter()
        .find(|u| u.rgb_allocations.iter().any(|a| a.asset_id.is_none()))
        .unwrap();
    assert_ne!(unspent_issue.utxo.outpoint, unspent_blind_1.utxo.outpoint);
    // remove blind
    wallet
        .fail_transfers(
            online.clone(),
            Some(blind_data.blinded_utxo.clone()),
            None,
            false,
        )
        .unwrap();
    wallet
        .delete_transfers(Some(blind_data.blinded_utxo), None, false)
        .unwrap();

    // take transfer from WaitingCounterparty to WaitingConfirmations
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online, Some(asset_id.clone()), vec![])
        .unwrap();
    // check blind doesn't get allocated to UTXO being spent
    let _result = wallet.blind(None, None, None).unwrap();
    show_unspent_colorings(&wallet, "after 2nd blind");
    let unspents = wallet.list_unspents(false).unwrap();
    let unspent_blind_2 = unspents
        .iter()
        .find(|u| u.rgb_allocations.iter().any(|a| a.asset_id.is_none()))
        .unwrap();
    assert_ne!(unspent_issue.utxo.outpoint, unspent_blind_2.utxo.outpoint);
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, online) = get_empty_wallet!();

    // bad asset id
    let result = wallet.blind(Some(s!("rgb1inexistent")), None, None);
    assert!(matches!(result, Err(Error::AssetNotFound(_))));

    // insufficient funds
    let result = wallet.blind(None, None, None);
    assert!(matches!(result, Err(Error::InsufficientBitcoins)));

    // invalid BlindedUTXO
    let result = BlindedUTXO::new(s!("invalid"));
    assert!(matches!(result, Err(Error::InvalidBlindedUTXO(_))));

    // invalid invoice
    let result = Invoice::new(s!("invalid"));
    assert!(matches!(result, Err(Error::InvalidInvoice(_))));

    // unsupported invoice
    fund_wallet(wallet.get_address());
    wallet.create_utxos(online, false, None, None).unwrap();
    let blind_data = wallet.blind(None, None, None).unwrap();
    let concealed_seal = ConcealedSeal::from_str(&blind_data.blinded_utxo).unwrap();
    let beneficiary = Beneficiary::BlindUtxo(concealed_seal);
    let amount = AmountExt::Milli(1, 1);
    let mut invoice = UniversalInvoice::new(beneficiary, None, None);
    invoice.set_amount(amount);
    let result = Invoice::new(invoice.to_string());
    assert!(matches!(result, Err(Error::UnsupportedInvoice)));
}

#[test]
fn wrong_asset_fail() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();

    // issue one asset per wallet
    let asset_a = wallet_1
        .issue_asset_rgb20(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_b = wallet_2
        .issue_asset_rgb20(
            online_2.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    let blind_data_a = wallet_1.blind(Some(asset_a.asset_id), None, None).unwrap();

    let recipient_map = HashMap::from([(
        asset_b.asset_id.clone(),
        vec![Recipient {
            amount,
            blinded_utxo: blind_data_a.blinded_utxo.clone(),
        }],
    )]);
    let txid = wallet_2
        .send(online_2.clone(), recipient_map, false)
        .unwrap();
    assert!(!txid.is_empty());

    // transfer is pending
    let rcv_transfer_a = get_test_transfer_recipient(&wallet_1, &blind_data_a.blinded_utxo);
    let (rcv_transfer_data_a, _) = get_test_transfer_data(&wallet_1, &rcv_transfer_a);
    assert_eq!(
        rcv_transfer_data_a.status,
        TransferStatus::WaitingCounterparty
    );

    // transfer doesn't progress to status WaitingConfirmations on the receiving side
    wallet_1.refresh(online_1, None, vec![]).unwrap();
    wallet_2.refresh(online_2, None, vec![]).unwrap();

    // transfer has been NACKed
    let (rcv_transfer_data_a, _) = get_test_transfer_data(&wallet_1, &rcv_transfer_a);
    assert_eq!(rcv_transfer_data_a.status, TransferStatus::Failed);
    let rcv_transfers_b = wallet_1.list_transfers(asset_b.asset_id);
    assert!(matches!(rcv_transfers_b, Err(Error::AssetNotFound(_))));
}
