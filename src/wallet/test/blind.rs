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
fn pending_transfer_skip() {
    initialize();

    let (mut wallet, _online) = get_funded_wallet!();

    let blind_data_1 = wallet.blind(None, None).unwrap();
    let blind_data_2 = wallet.blind(None, None).unwrap();

    let db_transfer_1 = wallet
        .database
        .iter_transfers()
        .unwrap()
        .into_iter()
        .find(|t| t.blinded_utxo == Some(blind_data_1.blinded_utxo.clone()))
        .unwrap();
    let db_transfer_2 = wallet
        .database
        .iter_transfers()
        .unwrap()
        .into_iter()
        .find(|t| t.blinded_utxo == Some(blind_data_2.blinded_utxo.clone()))
        .unwrap();

    let db_coloring_1 = wallet
        .database
        .iter_colorings()
        .unwrap()
        .into_iter()
        .find(|c| c.transfer_idx == db_transfer_1.idx)
        .unwrap();
    let db_coloring_2 = wallet
        .database
        .iter_colorings()
        .unwrap()
        .into_iter()
        .find(|c| c.transfer_idx == db_transfer_2.idx)
        .unwrap();

    let db_txo_1 = wallet
        .database
        .iter_txos()
        .unwrap()
        .into_iter()
        .find(|t| t.idx == db_coloring_1.txo_idx)
        .unwrap();
    let db_txo_2 = wallet
        .database
        .iter_txos()
        .unwrap()
        .into_iter()
        .find(|t| t.idx == db_coloring_2.txo_idx)
        .unwrap();

    assert_ne!(db_txo_1.idx, db_txo_2.idx);
}

#[test]
fn expire() {
    initialize();

    let expiration = 1;
    let (mut wallet, online) = get_funded_wallet!();

    // check expiration
    let now_timestamp = now().unix_timestamp();
    let blind_data_1 = wallet.blind(None, Some(expiration)).unwrap();
    let timestamp = now_timestamp + expiration as i64;
    assert!(blind_data_1.expiration_timestamp.unwrap() - timestamp <= 1);

    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration as u64 * 1000 + 2000,
    ));

    // call create_utxos() to trigger the expiration check (via _get_utxo(true, ...))
    let _result = wallet.create_utxos(online);

    // check transfer is now in status Failed
    let db_transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(db_transfers.len(), 1);
    let db_transfer = db_transfers.first().unwrap();
    assert_eq!(db_transfer.status, TransferStatus::Failed);
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
