use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    let blind_data = wallet.blind(None, Some(60)).unwrap();
    wallet
        .fail_transfers(online, Some(blind_data.blinded_utxo))
        .unwrap();
}

#[test]
fn fail() {
    initialize();

    // wallets
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let (mut wallet, online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            AMOUNT,
        )
        .unwrap();
    let asset_id = asset.asset_id;
    // blind
    let blind_data = rcv_wallet.blind(None, Some(60)).unwrap();
    let blinded_utxo = blind_data.blinded_utxo;
    // send
    wallet
        .send(online.clone(), asset_id.clone(), blinded_utxo.clone(), 66)
        .unwrap();

    // check starting transfer status
    assert!(check_test_transfer_status(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));

    // fail to fail incoming transfer: waiting counterparty -> confirmations
    let result = rcv_wallet.fail_transfers(rcv_online.clone(), Some(blinded_utxo.clone()));
    assert!(matches!(result, Err(Error::CannotFailTransfer(_))));
    assert!(check_test_transfer_status(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    // fail to fail outgoing transfer: waiting counterparty -> confirmations
    let result = wallet.fail_transfers(online.clone(), Some(blinded_utxo.clone()));
    assert!(matches!(result, Err(Error::CannotFailTransfer(_))));
    assert!(check_test_transfer_status(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));

    // fail to fail incoming transfer: waiting confirmations
    let result = rcv_wallet.fail_transfers(rcv_online.clone(), Some(blinded_utxo.clone()));
    assert!(matches!(result, Err(Error::CannotFailTransfer(_))));
    assert!(check_test_transfer_status(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    // fail to fail outgoing transfer: waiting confirmations
    let result = wallet.fail_transfers(online.clone(), Some(blinded_utxo.clone()));
    assert!(matches!(result, Err(Error::CannotFailTransfer(_))));
    assert!(check_test_transfer_status(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));

    // mine and refresh so transfers can settle
    mine();
    wallet
        .refresh(online.clone(), Some(asset_id.clone()))
        .unwrap();
    rcv_wallet
        .refresh(rcv_online.clone(), Some(asset_id))
        .unwrap();

    // fail to fail incoming transfer: settled
    let result = rcv_wallet.fail_transfers(rcv_online, Some(blinded_utxo.clone()));
    assert!(matches!(result, Err(Error::CannotFailTransfer(_))));
    assert!(check_test_transfer_status(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::Settled
    ));
    // fail to fail outgoing transfer: settled
    let result = wallet.fail_transfers(online, Some(blinded_utxo.clone()));
    assert!(matches!(result, Err(Error::CannotFailTransfer(_))));
    assert!(check_test_transfer_status(
        &wallet,
        &blinded_utxo,
        TransferStatus::Settled
    ));
}
