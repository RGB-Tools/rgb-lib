use super::*;

fn reuse_recipient_map(
    asset_id: &str,
    receive_data: &ReceiveData,
    amount: u64,
    witness: bool,
) -> HashMap<String, Vec<Recipient>> {
    HashMap::from([(
        asset_id.to_string(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
            witness_data: witness.then_some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )])
}

fn reusable_receive(
    party: &mut SinglesigParty,
    witness: bool,
    quota: Option<u32>,
    encryption: Option<Encryption>,
) -> Result<ReceiveData, Error> {
    if witness {
        party.wallet.witness_receive(
            Some(party.online),
            None,
            Assignment::Any,
            default_rcv_expiration(),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
            encryption,
            quota,
            None,
        )
    } else {
        party.wallet.blind_receive(
            Some(party.online),
            None,
            Assignment::Any,
            default_rcv_expiration(),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
            encryption,
            quota,
        )
    }
}

/// The incoming transfers of the recipient for the asset (or without one, if `None`), with the
/// given status.
fn incoming_transfers(
    party: &SinglesigParty,
    asset_id: Option<&str>,
    status: TransferStatus,
) -> Vec<Transfer> {
    party
        .list_transfers(asset_id)
        .into_iter()
        .filter(|t| {
            matches!(
                t.kind,
                TransferKind::ReceiveBlind | TransferKind::ReceiveWitness
            ) && t.status == status
        })
        .collect()
}

/// Send twice to a reusable invoice (with quota 2), checking a third transfer is refused, and
/// settle both transfers.
fn send_twice_to_reusable_invoice(witness: bool) {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // an allocation per transfer, as the change of a pending transfer is not spendable yet
    let asset = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT * 2, AMOUNT * 3]));

    let receive_data = reusable_receive(&mut rcv_party, witness, Some(2), None).unwrap();
    if !witness {
        // the blinded UTXO keeps room for both transfers the invoice can receive
        let transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
        let Some(RecipientTypeFull::Blind { unblinded_utxo }) = transfer.recipient_type else {
            panic!("expected a blind receive");
        };
        let unspent = rcv_party
            .list_unspents(false)
            .into_iter()
            .find(|u| u.utxo.outpoint == unblinded_utxo)
            .unwrap();
        assert_eq!(unspent.pending_blinded, 2);
    }

    // each operation uses a fresh sender key, so the same invoice can be paid more than once; the
    // 1st TX is broadcast before sending again, so the 2nd one doesn't spend the same inputs
    let recipient_map = reuse_recipient_map(&asset.asset_id, &receive_data, amount, witness);
    // no blocks are mined (e.g. by other tests) until both transfers are waiting confirmations
    let guard = stop_mining();
    let txid_1 = party.send_retry(&recipient_map);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    assert!(party.check_test_transfer_status_sender(&txid_1, TransferStatus::WaitingConfirmations));
    let txid_2 = party.send_retry(&recipient_map);
    assert_ne!(
        party.transfer_id_sender(&txid_1),
        party.transfer_id_sender(&txid_2)
    );

    // the invoice is now full
    let result = party.send_result(&recipient_map);
    assert_matches!(result, Err(Error::InvoiceQuotaExceeded));

    // the recipient discovers the 2nd transfer, which gets a transfer of its own
    rcv_party.wait_for_refresh(None);
    assert_eq!(
        incoming_transfers(
            &rcv_party,
            Some(&asset.asset_id),
            TransferStatus::WaitingConfirmations
        )
        .len(),
        2
    );
    party.wait_for_refresh(Some(&asset.asset_id));
    assert!(party.check_test_transfer_status_sender(&txid_2, TransferStatus::WaitingConfirmations));

    // both transfers settle
    drop(guard);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    assert_eq!(
        incoming_transfers(&rcv_party, Some(&asset.asset_id), TransferStatus::Settled).len(),
        2
    );
    assert!(party.check_test_transfer_status_sender(&txid_1, TransferStatus::Settled));
    assert!(party.check_test_transfer_status_sender(&txid_2, TransferStatus::Settled));
    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance.settled, amount * 2);
    assert_eq!(balance.spendable, amount * 2);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_blind_invoice() {
    send_twice_to_reusable_invoice(false);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_witness_invoice() {
    send_twice_to_reusable_invoice(true);
}

/// A copy of a transfer (the same payment posted with a different sender key) to a reusable
/// invoice is refused, so the payment is only credited once.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_invoice_payment_copy_refused() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);

    // the recipient accepts cleartext consignments, so the copy can be posted as-is
    let receive_data =
        reusable_receive(&mut rcv_party, false, Some(2), Some(Encryption::Optional)).unwrap();
    let recipient_map = reuse_recipient_map(&asset.asset_id, &receive_data, amount, false);
    // no blocks are mined (e.g. by other tests) until the copy has been refused
    let guard = stop_mining();
    let txid = party.send_retry(&recipient_map);
    rcv_party.wait_for_refresh(None);
    assert_eq!(
        incoming_transfers(
            &rcv_party,
            Some(&asset.asset_id),
            TransferStatus::WaitingConfirmations
        )
        .len(),
        1
    );

    // post a copy of the transfer, with another sender key
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid);
    post_test_consignment(
        &get_proxy_client(None),
        &receive_data.recipient_id,
        &receive_data.public_key,
        &consignment_path,
        &party.consignment_hash_sender(&txid),
    );

    // the copy is discovered and refused, getting no asset, as the invoice has none
    rcv_party.wait_for_refresh(None);
    assert_eq!(
        incoming_transfers(&rcv_party, None, TransferStatus::Failed).len(),
        1
    );

    // the original transfer settles, credited once
    party.wait_for_refresh(Some(&asset.asset_id));
    drop(guard);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Settled));
    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance.settled, amount);
}

/// Post to a reusable invoice a transfer whose consignment isn't one, in cleartext.
fn post_garbage_transfer(receive_data: &ReceiveData) {
    let data_dir = PrivateDataDir::new();
    let path = data_dir.path().join("garbage");
    fs::write(&path, b"not a consignment").unwrap();
    post_test_consignment(
        &get_proxy_client(None),
        &receive_data.recipient_id,
        &receive_data.public_key,
        &path,
        &hash_file(&path).unwrap(),
    );
}

/// All the transfers of the party, whatever their asset.
fn all_transfers(party: &SinglesigParty) -> Vec<Transfer> {
    party.list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::All)
}

/// Refresh the recipient once (a single pass, so the mocked proxy failures are used as set).
fn refresh_once(party: &mut SinglesigParty) {
    party.refresh_result(None, &[]).unwrap();
}

/// A transfer to a reusable invoice that can't be paying it (here, as its consignment can't be
/// decrypted) is refused and deleted from the proxy, freeing its quota slot, without getting a
/// local transfer, and a failed deletion is retried.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_invoice_spam_refused() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);

    // the recipient requires encryption, so a cleartext consignment can't be decrypted
    let receive_data =
        reusable_receive(&mut rcv_party, false, Some(2), Some(Encryption::Required)).unwrap();
    let recipient_map = reuse_recipient_map(&asset.asset_id, &receive_data, 66, false);
    post_garbage_transfer(&receive_data);
    post_garbage_transfer(&receive_data);
    assert_matches!(
        party.send_result(&recipient_map),
        Err(Error::InvoiceQuotaExceeded)
    );

    // both are refused, getting no local transfer, but their deletion fails
    MOCK_TRANSFER_DELETE_FAILURES.replace(2);
    refresh_once(&mut rcv_party);
    let transfers = all_transfers(&rcv_party);
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].status, TransferStatus::WaitingCounterparty);
    assert_matches!(
        party.send_result(&recipient_map),
        Err(Error::InvoiceQuotaExceeded)
    );

    // they're discovered again, refused and deleted
    refresh_once(&mut rcv_party);
    assert_eq!(all_transfers(&rcv_party).len(), 1);

    // the invoice can now be paid, the payment getting the transfer created with the invoice
    party.send_retry(&recipient_map);
    refresh_once(&mut rcv_party);
    let transfers = all_transfers(&rcv_party);
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].status, TransferStatus::WaitingConfirmations);
}

/// A transfer to a reusable invoice that failed after getting a local transfer is deleted from
/// the proxy, freeing its quota slot, and a failed deletion is retried.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_invoice_failed_transfer_delete_retried() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // an allocation per transfer, as the change of a pending transfer is not spendable yet
    let asset = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT * 2]));

    // the recipient accepts cleartext consignments, so a copy can be posted as-is
    let receive_data =
        reusable_receive(&mut rcv_party, false, Some(2), Some(Encryption::Optional)).unwrap();
    let recipient_map = reuse_recipient_map(&asset.asset_id, &receive_data, amount, false);
    let _guard = stop_mining();
    let txid = party.send_retry(&recipient_map);
    refresh_once(&mut rcv_party);
    party.wait_for_refresh(Some(&asset.asset_id));

    // a copy of the transfer (with another sender key) passes the checks made when discovering it
    // but fails once processed, as its payment has already been received
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid);
    post_test_consignment(
        &get_proxy_client(None),
        &receive_data.recipient_id,
        &receive_data.public_key,
        &consignment_path,
        &party.consignment_hash_sender(&txid),
    );
    MOCK_TRANSFER_DELETE_FAILURES.replace(1);
    refresh_once(&mut rcv_party);
    assert_eq!(
        incoming_transfers(&rcv_party, None, TransferStatus::Failed).len(),
        1
    );
    assert_matches!(
        party.send_result(&recipient_map),
        Err(Error::InvoiceQuotaExceeded)
    );

    // the deletion is retried, freeing the quota slot for another payment
    refresh_once(&mut rcv_party);
    party.send_retry(&recipient_map);
    refresh_once(&mut rcv_party);
    assert_eq!(
        incoming_transfers(
            &rcv_party,
            Some(&asset.asset_id),
            TransferStatus::WaitingConfirmations
        )
        .len(),
        2
    );
}

/// A transfer to a reusable invoice whose consignment can't be downloaded gets no local transfer
/// and is discovered again on a later refresh.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_invoice_download_fail() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);

    let receive_data = reusable_receive(&mut rcv_party, false, Some(2), None).unwrap();
    let recipient_map = reuse_recipient_map(&asset.asset_id, &receive_data, 66, false);
    party.send_retry(&recipient_map);

    MOCK_GET_CONSIGNMENT_FAILURES.replace(1);
    refresh_once(&mut rcv_party);
    let transfers = all_transfers(&rcv_party);
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].status, TransferStatus::WaitingCounterparty);

    refresh_once(&mut rcv_party);
    let transfers = all_transfers(&rcv_party);
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].status, TransferStatus::WaitingConfirmations);
}

/// The script of a reusable witness invoice stays watched after its first transfer settles, so a
/// later payment to it is flagged as pending witness before its transfer has been discovered.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_witness_invoice_watched_after_settle() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT * 2]));

    let receive_data = reusable_receive(&mut rcv_party, true, Some(2), None).unwrap();
    let recipient_map = reuse_recipient_map(&asset.asset_id, &receive_data, amount, true);
    let txid = party.send_retry(&recipient_map);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Settled));

    // a 2nd payment, broadcast right away (as a donation), is seen by a sync before the transfer
    // is discovered
    let _guard = stop_mining();
    let OperationResult { txid, .. } = party
        .wallet
        .send(
            party.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            default_send_expiration(),
            Some(PROXY_ENDPOINT.clone()),
        )
        .unwrap();
    rcv_party.sync(SyncOptions {
        keychain: SyncKeychain::Colored,
        strategy: SyncStrategy::FastSync,
    });
    let txos: Vec<DbTxo> = rcv_party
        .db_txos()
        .into_iter()
        .filter(|t| t.txid == txid)
        .collect();
    assert_eq!(txos.len(), 1);
    assert!(txos[0].pending_witness);

    // the transfer is then discovered
    refresh_once(&mut rcv_party);
    assert_eq!(
        incoming_transfers(
            &rcv_party,
            Some(&asset.asset_id),
            TransferStatus::WaitingConfirmations
        )
        .len(),
        1
    );
}

/// The batch transfer of the receive created along with a reusable invoice.
fn invoice_batch_transfer_idx(party: &SinglesigParty, receive_data: &ReceiveData) -> i32 {
    all_transfers(party)
        .into_iter()
        .find(|t| t.recipient_id.as_ref() == Some(&receive_data.recipient_id))
        .unwrap()
        .batch_transfer_idx
}

/// The failed transfers of a reusable invoice that can still receive transfers are not deleted,
/// as they're what its data is reached through, so the invoice keeps receiving: its blinded UTXO
/// keeps room for its transfers and a later payment is discovered.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_invoice_transfers_not_deleted_while_live() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);

    // the receive created with the invoice is failed (cancelling it), as is an unrelated receive
    let receive_data = reusable_receive(&mut rcv_party, false, Some(2), None).unwrap();
    let invoice_bt_idx = invoice_batch_transfer_idx(&rcv_party, &receive_data);
    assert!(rcv_party.fail_transfers_single(invoice_bt_idx));
    let other_receive_data = rcv_party.blind_receive();
    let other_bt_idx = invoice_batch_transfer_idx(&rcv_party, &other_receive_data);
    assert!(rcv_party.fail_transfers_single(other_bt_idx));

    // deleting the invoice transfer is refused, deleting all failed transfers skips it
    assert_matches!(
        rcv_party.delete_transfers_result(Some(invoice_bt_idx), false),
        Err(Error::CannotDeleteBatchTransfer)
    );
    assert!(rcv_party.delete_transfers(None, false));
    let transfers = all_transfers(&rcv_party);
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].batch_transfer_idx, invoice_bt_idx);
    assert_eq!(transfers[0].status, TransferStatus::Failed);
    assert!(!rcv_party.delete_transfers(None, false));

    // the blinded UTXO keeps room for both transfers the invoice can receive
    let transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let Some(RecipientTypeFull::Blind { unblinded_utxo }) = transfer.recipient_type else {
        panic!("expected a blind receive");
    };
    let unspent = rcv_party
        .list_unspents(false)
        .into_iter()
        .find(|u| u.utxo.outpoint == unblinded_utxo)
        .unwrap();
    assert_eq!(unspent.pending_blinded, 2);

    // a payment to the invoice is still discovered
    let recipient_map = reuse_recipient_map(&asset.asset_id, &receive_data, amount, false);
    party.send_retry(&recipient_map);
    refresh_once(&mut rcv_party);
    assert_eq!(
        incoming_transfers(
            &rcv_party,
            Some(&asset.asset_id),
            TransferStatus::WaitingConfirmations
        )
        .len(),
        1
    );
}

/// The failed transfers of a reusable invoice can be deleted once it can no longer receive
/// transfers.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_invoice_transfers_deleted_after_deadline() {
    initialize();

    // the deadline is the invoice expiration plus the proxy retention grace and the clock skew
    let expiration_secs: i64 = 5;
    let retention_grace: u32 = 2;
    let skew_margin: u32 = 1;
    MOCK_RETENTION_GRACE.replace(Some(retention_grace));
    MOCK_CLOCK_SKEW_MARGIN.replace(Some(skew_margin));

    let mut rcv_party = get_funded_party!();

    let expiration = now().unix_timestamp() + expiration_secs;
    let receive_data = rcv_party
        .wallet
        .blind_receive(
            Some(rcv_party.online),
            None,
            Assignment::Any,
            expiration as u64,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
            None,
            Some(2),
        )
        .unwrap();
    let invoice_bt_idx = invoice_batch_transfer_idx(&rcv_party, &receive_data);
    assert!(rcv_party.fail_transfers_single(invoice_bt_idx));
    assert_matches!(
        rcv_party.delete_transfers_result(Some(invoice_bt_idx), false),
        Err(Error::CannotDeleteBatchTransfer)
    );

    let deadline = expiration + i64::from(retention_grace) + i64::from(skew_margin);
    let wait_secs = (deadline - now().unix_timestamp()).clamp(0, i64::MAX) as u64 + 1;
    std::thread::sleep(std::time::Duration::from_secs(wait_secs));
    assert!(rcv_party.delete_transfers(Some(invoice_bt_idx), false));
    assert!(all_transfers(&rcv_party).is_empty());
}

/// A reusable invoice without an asset can receive a different asset with each transfer, the
/// 2nd one arriving once the 1st one has settled.
fn send_assets_to_reusable_invoice(witness: bool) {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset_1 = party.issue_asset_nia(None);
    let asset_2 = party.issue_asset_nia(None);

    let receive_data = reusable_receive(&mut rcv_party, witness, Some(2), None).unwrap();
    for asset in [&asset_1, &asset_2] {
        let recipient_map = reuse_recipient_map(&asset.asset_id, &receive_data, amount, witness);
        let txid = party.send_retry(&recipient_map);
        rcv_party.wait_for_refresh(None);
        party.wait_for_refresh(Some(&asset.asset_id));
        mine(false);
        rcv_party.wait_for_refresh(None);
        party.wait_for_refresh(Some(&asset.asset_id));
        assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Settled));
    }

    // each transfer received its own asset
    for asset in [&asset_1, &asset_2] {
        assert_eq!(
            incoming_transfers(&rcv_party, Some(&asset.asset_id), TransferStatus::Settled).len(),
            1
        );
        assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, amount);
    }
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_blind_invoice_any_asset() {
    send_assets_to_reusable_invoice(false);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reusable_witness_invoice_any_asset() {
    send_assets_to_reusable_invoice(true);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_fail() {
    initialize();

    let mut rcv_party = get_funded_party!();

    // invalid quota
    for witness in [false, true] {
        let result = reusable_receive(&mut rcv_party, witness, Some(0), None);
        assert_matches!(result, Err(Error::InvalidQuota { details: m }) if m == "must be at least 1");
    }

    // a blinded UTXO can only hold so many allocations
    let result = reusable_receive(
        &mut rcv_party,
        false,
        Some(MAX_ALLOCATIONS_PER_UTXO + 1),
        None,
    );
    assert_matches!(result, Err(Error::InvalidQuota { .. }));

    // nor more allocations than it can hold and still be spent
    MOCK_MAX_UTXO_ASSET_ALLOCATIONS.replace(Some(2));
    let result = reusable_receive(&mut rcv_party, false, Some(3), None);
    MOCK_MAX_UTXO_ASSET_ALLOCATIONS.replace(None);
    assert_matches!(result, Err(Error::InvalidQuota { details: m }) if m == "a blind invoice can receive at most 2 transfers");

    // reuse requires transport endpoints
    let result = rcv_party.wallet.blind_receive(
        None,
        None,
        Assignment::Any,
        default_rcv_expiration(),
        vec![],
        MIN_CONFIRMATIONS,
        None,
        Some(2),
    );
    assert_matches!(result, Err(Error::InvalidQuota { details: m }) if m == "a reusable invoice requires transport endpoints");
    let result = rcv_party.wallet.witness_receive(
        None,
        None,
        Assignment::Any,
        default_rcv_expiration(),
        vec![],
        MIN_CONFIRMATIONS,
        None,
        Some(2),
        None,
    );
    assert_matches!(result, Err(Error::InvalidQuota { details: m }) if m == "a reusable invoice requires transport endpoints");
}
