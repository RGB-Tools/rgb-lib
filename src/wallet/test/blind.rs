use super::*;

#[test]
fn success() {
    initialize();

    let expiration = 60;
    let (mut wallet, online) = get_funded_wallet!();

    // default expiration
    let now_timestamp = now().unix_timestamp();
    let blind_data = wallet.blind(None, None).unwrap();
    assert!(blind_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + DURATION_RCV_TRANSFER as i64;
    assert!(blind_data.expiration_timestamp.unwrap() - timestamp <= 1);

    // positive expiration
    let now_timestamp = now().unix_timestamp();
    let blind_data = wallet.blind(None, Some(expiration)).unwrap();
    assert!(blind_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + expiration as i64;
    assert!(blind_data.expiration_timestamp.unwrap() - timestamp <= 1);

    // 0 expiration
    let blind_data = wallet.blind(None, Some(0)).unwrap();
    assert!(blind_data.expiration_timestamp.is_none());

    // asset id is set
    let asset = wallet
        .issue_asset(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let result = wallet.blind(Some(asset.asset_id), None);
    assert!(result.is_ok());
}

#[test]
fn respect_max_allocations() {
    initialize();

    let (mut wallet, _online) = get_funded_wallet!();

    // generate MAX_ALLOCATIONS_PER_UTXO + 1 blinded UTXOs and save selected TXOs
    let mut txo_list: HashSet<DbTxo> = HashSet::new();
    for _ in 0..=MAX_ALLOCATIONS_PER_UTXO {
        let blind_data = wallet.blind(None, None).unwrap();
        let transfer = get_test_transfer_recipient(&wallet, &blind_data.blinded_utxo);
        let coloring = get_test_coloring(&wallet, transfer.asset_transfer_idx);
        let txo = get_test_txo(&wallet, coloring.txo_idx);
        txo_list.insert(txo);
    }

    // check a second TXO has been selected
    assert_eq!(txo_list.len(), 2);
}

#[test]
fn expire() {
    initialize();

    let expiration = 1;
    let (mut wallet, _online) = get_funded_wallet!();

    // check expiration
    let now_timestamp = now().unix_timestamp();
    let blind_data_1 = wallet.blind(None, Some(expiration)).unwrap();
    let timestamp = now_timestamp + expiration as i64;
    assert!(blind_data_1.expiration_timestamp.unwrap() - timestamp <= 1);

    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration as u64 * 1000 + 2000,
    ));

    // trigger the expiration of pending transfers
    wallet._handle_expired_transfers().unwrap();

    // check transfer is now in status Failed
    let transfer = get_test_transfer_recipient(&wallet, &blind_data_1.blinded_utxo);
    let transfer_data = wallet.database.get_transfer_data(&transfer).unwrap();
    assert_eq!(transfer_data.status, TransferStatus::Failed);
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, _online) = get_funded_wallet!();

    // bad asset id
    let result = wallet.blind(Some(s!("rgb1inexistent")), None);
    assert!(matches!(result, Err(Error::AssetNotFound(_))));

    // insufficient funds
    let (mut wallet, _online) = get_empty_wallet!();
    let result = wallet.blind(None, None);
    assert!(matches!(result, Err(Error::InsufficientFunds)));
}
