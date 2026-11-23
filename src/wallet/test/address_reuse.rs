use super::*;

/// A wallet reusing its vanilla address returns it from get_address and sends the bitcoin change
/// to it, while its colored outputs go to new addresses.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn vanilla_address_reused() {
    initialize();

    let mut party = get_reusing_party(true, false);
    let mut other_party = get_funded_party!();
    let vanilla_script = party.first_script(KeychainKind::Internal);

    let address = party.wallet.get_address().unwrap();
    assert_eq!(party.wallet.get_address().unwrap(), address);
    assert_eq!(script_of(&address), vanilla_script);

    // the bitcoin change of create_utxos and send_btc goes to the vanilla address
    party.create_utxos(false, Some(2), None, FEE_RATE, None);
    party.send_btc(&other_party.wallet.get_address().unwrap(), 1000);
    let vanilla = party.unspent_scripts(KeychainKind::Internal);
    assert!(!vanilla.is_empty());
    assert!(vanilla.iter().all(|s| *s == vanilla_script));
    assert_eq!(
        party
            .wallet
            .bdk_wallet()
            .spk_index()
            .last_revealed_index(KeychainKind::Internal),
        Some(0)
    );

    // the colored UTXOs are on new addresses
    let colored = party.unspent_scripts(KeychainKind::External);
    assert_eq!(colored.len(), UTXO_NUM as usize + 2);
    assert_eq!(colored.iter().collect::<HashSet<_>>().len(), colored.len());
}

/// A wallet reusing its colored address creates its colored UTXOs and the change of its sends on
/// it, while get_address returns new addresses.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn colored_address_reused() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_reusing_party(false, true);
    let mut rcv_party = get_funded_party!();
    let colored_script = party.first_script(KeychainKind::External);

    assert_ne!(
        party.wallet.get_address().unwrap(),
        party.wallet.get_address().unwrap()
    );

    // the colored UTXOs are on the colored address, usable right away
    party.create_utxos(false, Some(2), None, FEE_RATE, None);
    let colored = party.unspent_scripts(KeychainKind::External);
    assert_eq!(colored.len(), UTXO_NUM as usize + 2);
    assert!(colored.iter().all(|s| *s == colored_script));
    assert!(party.db_txos().iter().all(|t| !t.pending_witness));

    // the change of a send goes to the colored address too
    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.witness_receive();
    party.send_retry(&witness_recipient_map(
        &asset.asset_id,
        &receive_data,
        Assignment::Fungible(amount),
        1000,
    ));
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    let colored = party.unspent_scripts(KeychainKind::External);
    assert!(colored.iter().all(|s| *s == colored_script));
    assert!(party.db_txos().iter().all(|t| !t.pending_witness));
    assert_eq!(
        party
            .wallet
            .bdk_wallet()
            .spk_index()
            .last_revealed_index(KeychainKind::External),
        Some(0)
    );
}

/// A witness receive can reuse the address of an earlier one: the transfers paying it are told
/// apart and each settles crediting its own amount, whatever the order they're paid in. With
/// colored reuse on, the first receive is given the address the wallet sends its own colored
/// outputs to, and the second one reuses it.
#[cfg(feature = "electrum")]
fn reuse_address_paid_in_order(colored_reuse: bool, reverse: bool) {
    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = if colored_reuse {
        get_reusing_party(false, true)
    } else {
        get_funded_party!()
    };

    // an allocation per transfer, as the change of a pending transfer is not spendable yet
    let asset = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT * 2]));

    // a blind receive pays no address
    assert!(rcv_party.blind_receive().address.is_none());

    // without an address each witness receive gets a new one
    let receive_data_other_1 = rcv_party.witness_receive();
    let receive_data_other_2 = rcv_party.witness_receive();
    assert_ne!(receive_data_other_1.address, receive_data_other_2.address);

    // with one it reuses it, with an invoice of its own
    let first = colored_reuse.then(|| rcv_party.first_address(KeychainKind::External));
    let receive_data_1 = rcv_party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), first)
        .unwrap();
    let address = receive_data_1.address.clone().unwrap();
    let receive_data_2 = rcv_party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(address.clone()))
        .unwrap();
    assert_eq!(receive_data_2.address.as_ref(), Some(&address));
    assert_eq!(receive_data_1.recipient_id, receive_data_2.recipient_id);
    assert_ne!(receive_data_1.public_key, receive_data_2.public_key);
    assert_ne!(receive_data_1.invoice, receive_data_2.invoice);

    // sharing the recipient ID, the receives can't be paid in the same batch
    let mut both = witness_recipient_map(
        &asset.asset_id,
        &receive_data_1,
        Assignment::Fungible(amount),
        1000,
    );
    both.get_mut(&asset.asset_id).unwrap().extend(
        witness_recipient_map(
            &asset.asset_id,
            &receive_data_2,
            Assignment::Fungible(amount),
            1000,
        )
        .remove(&asset.asset_id)
        .unwrap(),
    );
    assert_matches!(
        party.send_begin_result(&both),
        Err(Error::RecipientIDDuplicated)
    );

    // no blocks are mined (e.g. by other tests) until both transfers are waiting confirmations
    let mut payments = vec![(&receive_data_1, amount), (&receive_data_2, amount * 2)];
    if reverse {
        payments.reverse();
    }
    let guard = stop_mining();
    let mut txids = vec![];
    for (receive_data, amt) in &payments {
        txids.push(party.send_retry(&witness_recipient_map(
            &asset.asset_id,
            receive_data,
            Assignment::Fungible(*amt),
            1000,
        )));
        rcv_party.wait_for_refresh(None);
        party.wait_for_refresh(Some(&asset.asset_id));
    }
    for (txid, (receive_data, amt)) in txids.iter().zip(&payments) {
        assert!(
            party.check_test_transfer_status_sender(txid, TransferStatus::WaitingConfirmations)
        );
        rcv_party.check_receive(
            &asset.asset_id,
            receive_data,
            *amt,
            TransferStatus::WaitingConfirmations,
        );
    }

    // both transfers settle, each crediting its own amount
    drop(guard);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    for (txid, (receive_data, amt)) in txids.iter().zip(&payments) {
        assert!(party.check_test_transfer_status_sender(txid, TransferStatus::Settled));
        rcv_party.check_receive(&asset.asset_id, receive_data, *amt, TransferStatus::Settled);
    }
    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance.settled, amount * 3);
    assert_eq!(balance.spendable, amount * 3);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_address() {
    initialize();

    reuse_address_paid_in_order(false, false);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_address_colored_reuse_reverse_order() {
    initialize();

    reuse_address_paid_in_order(true, true);
}

/// The earlier witness receives on a reused address are marked as sharing it, so the address stays
/// watched while one of them is pending, even after the later receive on it settles: a payment to
/// it is flagged as pending witness before its transfer has been discovered.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reused_address_watched_after_later_settles() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT * 2]));

    let receive_data_1 = rcv_party.witness_receive();
    let address = receive_data_1.address.clone().unwrap();
    let receive_data_2 = rcv_party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(address.clone()))
        .unwrap();

    // the later receive settles
    let txid = party.send_retry(&witness_recipient_map(
        &asset.asset_id,
        &receive_data_2,
        Assignment::Fungible(amount),
        1000,
    ));
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Settled));

    // a payment that carries no transfer is flagged, as the earlier receive is still pending, and
    // the address stays watched
    let _guard = stop_mining();
    let txid = party.send_btc(&address, 5000);
    rcv_party.colored_fast_sync();
    assert!(rcv_party.only_txo_of(&txid).pending_witness);
    assert!(rcv_party.is_pending_witness_script(&script_of(&address)));

    // so the payment to the earlier receive, broadcast right away (as a donation), is flagged
    let OperationResult { txid, .. } = party
        .wallet
        .send(
            party.online,
            witness_recipient_map(
                &asset.asset_id,
                &receive_data_1,
                Assignment::Fungible(amount),
                1000,
            ),
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            default_send_expiration(),
            Some(PROXY_ENDPOINT.clone()),
        )
        .unwrap();
    rcv_party.colored_fast_sync();
    assert!(rcv_party.only_txo_of(&txid).pending_witness);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_address_fail() {
    initialize();

    let mut party = get_funded_party!();
    let mut other_party = get_funded_party!();
    let cannot_reuse = |party: &mut SinglesigParty, address: String| -> String {
        match party.witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(address)) {
            Err(Error::CannotReuseAddress { details }) => details,
            res => panic!("unexpected result: {res:?}"),
        }
    };

    // an invalid address
    let details = cannot_reuse(&mut party, s!("invalid"));
    assert!(details.starts_with("invalid address"), "{details}");

    // an address of another network
    let receive_data = party.witness_receive();
    let address = receive_data.address.clone().unwrap();
    let mainnet_address = BdkAddress::from_script(&script_of(&address), BdkNetwork::Bitcoin)
        .unwrap()
        .to_string();
    let details = cannot_reuse(&mut party, mainnet_address);
    assert_eq!(details, "invalid address: belongs to another network");

    // an address of the vanilla keychain or of another wallet
    let vanilla_address = party.wallet.get_address().unwrap();
    let details = cannot_reuse(&mut party, vanilla_address);
    assert_eq!(details, "not an address of the wallet's colored keychain");
    let other_address = other_party.witness_receive().address.unwrap();
    let details = cannot_reuse(&mut party, other_address);
    assert_eq!(details, "not an address of the wallet's colored keychain");

    // an address of the colored keychain not revealed yet
    let last_revealed = party
        .wallet
        .bdk_wallet()
        .spk_index()
        .last_revealed_index(KeychainKind::External)
        .unwrap();
    let unrevealed_address = party
        .wallet
        .bdk_wallet()
        .peek_address(KeychainKind::External, last_revealed + 1)
        .to_string();
    let details = cannot_reuse(&mut party, unrevealed_address);
    assert_eq!(details, "not revealed yet");

    // an address no witness receive used, like the one of a colored UTXO, can be reused
    let utxo_script = party
        .wallet
        .bdk_wallet()
        .list_unspent()
        .find(|o| o.keychain == KeychainKind::External)
        .unwrap()
        .txout
        .script_pubkey;
    let utxo_address = BdkAddress::from_script(&utxo_script, BdkNetwork::Regtest)
        .unwrap()
        .to_string();
    party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(utxo_address))
        .unwrap();

    // an address an earlier witness receive used with the out-of-band exchange
    let oob_address = party
        .witness_receive_result(vec![], None)
        .unwrap()
        .address
        .unwrap();
    let details = cannot_reuse(&mut party, oob_address);
    assert_eq!(
        details,
        "an earlier witness receive on it used the out-of-band exchange"
    );

    // reusing an address requires transport endpoints
    let result = party.witness_receive_result(vec![], Some(address.clone()));
    assert_matches!(result, Err(Error::UnsupportedAddressReuse { details: m }) if m == "a witness receive reusing an address requires transport endpoints");

    // the address can still be reused
    party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(address))
        .unwrap();
}

/// The outputs a wallet reusing its colored address creates for itself on it are usable right
/// away, even while a witness receive on the address is pending: the colored UTXOs and the bitcoin
/// change of a send that gets no RGB change.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn own_outputs_usable_with_pending_deposit() {
    initialize();

    let mut party = get_reusing_party_unfunded(false, true);
    let mut rcv_party = get_funded_party!();
    fund_wallet(party.wallet.get_address().unwrap());
    let colored_script = party.first_script(KeychainKind::External);

    // a witness receive with no address never gets the colored address, even before the wallet
    // has used it
    let receive_data = party.witness_receive();
    assert_ne!(
        script_of(receive_data.address.as_ref().unwrap()),
        colored_script
    );

    // the colored address can be passed to one, so a receive is pending on it
    let first = party.first_address(KeychainKind::External);
    party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(first))
        .unwrap();

    // the colored UTXOs created on it are usable
    party.create_utxos_default();
    party.colored_fast_sync();
    let utxos: Vec<DbTxo> = party
        .db_txos()
        .into_iter()
        .filter(|t| t.exists && !t.spent)
        .collect();
    assert_eq!(utxos.len(), UTXO_NUM as usize);
    assert!(utxos.iter().all(|t| !t.pending_witness));
    // they don't count as paying the receive, whose address stays watched
    assert!(party.is_pending_witness_script(&colored_script));

    // a send of a whole allocation gets no RGB change, its bitcoin change on the colored address
    // is usable
    let asset = party.issue_asset_nia(Some(&[AMOUNT]));
    let rcv_receive_data = rcv_party.blind_receive();
    let txid = party.send_retry(&blind_recipient_map(
        &asset.asset_id,
        &rcv_receive_data,
        Assignment::Fungible(AMOUNT),
    ));
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    party.colored_fast_sync();
    // the change follows the OP_RETURN output
    assert_eq!(party.tx_outputs(&txid)[1].script_pubkey, colored_script);
    let change = Outpoint { txid, vout: 1 };
    assert!(!party.db_txo(&change).unwrap().pending_witness);
    assert!(party.db_txos().iter().all(|t| !t.pending_witness));
}

/// The bitcoin change of a send is told apart from a recipient paying the same script.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn change_found_by_position() {
    initialize();

    let mut party = get_reusing_party(false, true);
    let colored_script = party.first_script(KeychainKind::External);

    let inputs: HashSet<BdkOutPoint> = party
        .db_txos()
        .into_iter()
        .filter(|t| t.exists && !t.spent)
        .map(BdkOutPoint::from)
        .collect();
    let witness_recipients = vec![(colored_script.clone(), 1000)];
    let (psbt, btc_change) = party
        .wallet
        .prepare_psbt(
            inputs,
            &witness_recipients,
            FeeRate::from_sat_per_vb_u32(FEE_RATE as u32),
        )
        .unwrap();
    let outputs = &psbt.unsigned_tx.output;
    assert_eq!(outputs[1].script_pubkey, colored_script);
    assert_eq!(outputs[2].script_pubkey, colored_script);
    assert_eq!(btc_change.unwrap().vout, 2);
}

/// An output no transfer claims on a reused address can't be used while a witness receive on the
/// address is pending, and is released once none is.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn unclaimed_output_blocked_while_receive_pending() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let receive_data_1 = rcv_party.witness_receive();
    let address = receive_data_1.address.clone().unwrap();
    let receive_data_2 = rcv_party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(address.clone()))
        .unwrap();

    // a payment to the address that carries no transfer
    let txid = party.send_btc(&address, 5000);
    mine(false);
    rcv_party.colored_fast_sync();
    let txo = |party: &SinglesigParty| {
        party
            .db_txos()
            .into_iter()
            .find(|t| t.txid == txid)
            .unwrap()
    };
    assert!(txo(&rcv_party).pending_witness);

    // still blocked while a receive on the address is pending
    assert!(rcv_party.fail_transfers_single(receive_data_1.batch_transfer_idx));
    rcv_party.colored_fast_sync();
    assert!(txo(&rcv_party).pending_witness);

    // released once none is
    assert!(rcv_party.fail_transfers_single(receive_data_2.batch_transfer_idx));
    rcv_party.colored_fast_sync();
    assert!(!txo(&rcv_party).pending_witness);
}

/// After a dry-run begin, the outputs a wallet reusing its colored address creates for itself on it
/// are registered before the end broadcasts them, so they're not taken for the payment of a witness
/// receive pending on the address: the change of a donation send and of an inflation, whose new
/// supply goes to new addresses.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn own_outputs_registered_after_dry_run() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_reusing_party(false, true);
    let mut rcv_party = get_funded_party!();
    let colored_script = party.first_script(KeychainKind::External);

    // a witness receive pending on the colored address
    let first = party.first_address(KeychainKind::External);
    party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(first))
        .unwrap();

    // a donation send, begun as a dry run: its change follows the OP_RETURN output and the
    // recipient one
    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.witness_receive();
    let begin = party
        .wallet
        .send_begin(
            party.online,
            witness_recipient_map(
                &asset.asset_id,
                &receive_data,
                Assignment::Fungible(amount),
                1000,
            ),
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            default_send_expiration(),
            Some(PROXY_ENDPOINT.clone()),
            true,
        )
        .unwrap();
    assert!(begin.batch_transfer_idx.is_none());
    let signed_psbt = party.wallet.sign_psbt(begin.psbt, None).unwrap();
    let OperationResult { txid, .. } = party.wallet.send_end(party.online, signed_psbt).unwrap();
    assert_eq!(party.tx_outputs(&txid)[2].script_pubkey, colored_script);
    let change = party.db_txo(&Outpoint { txid, vout: 2 }).unwrap();
    assert!(change.exists);
    assert!(!change.pending_witness);

    // an inflation, begun as a dry run
    let asset_ifa = party.issue_asset_ifa(Some(&[AMOUNT]), Some(&[AMOUNT]), None);
    let inflation_amounts = [1, 2];
    let unsigned_psbt = party.inflate_begin(&asset_ifa.asset_id, &inflation_amounts);
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt, None).unwrap();
    let OperationResult { txid, .. } = party.inflate_end_result(&signed_psbt).unwrap();
    let outputs = party.tx_outputs(&txid);
    let change_vout = inflation_amounts.len() + 1;
    assert_eq!(outputs.len(), change_vout + 1);
    assert!(
        outputs[1..change_vout]
            .iter()
            .all(|o| o.script_pubkey != colored_script)
    );
    assert_eq!(outputs[change_vout].script_pubkey, colored_script);
    let change = party
        .db_txo(&Outpoint {
            txid,
            vout: change_vout as u32,
        })
        .unwrap();
    assert!(change.exists);
    assert!(!change.pending_witness);

    assert!(party.db_txos().iter().all(|t| !t.pending_witness));
    assert!(party.is_pending_witness_script(&colored_script));
}

/// A wallet reusing its colored address can send to itself on it: the recipient output and the
/// change pay the same script and both transfers settle.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_to_oneself_on_reused_address() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_reusing_party(false, true);
    let colored_script = party.first_script(KeychainKind::External);

    let asset = party.issue_asset_nia(None);
    let first = party.first_address(KeychainKind::External);
    let receive_data = party
        .witness_receive_result(TRANSPORT_ENDPOINTS.clone(), Some(first))
        .unwrap();
    let txid = party.send_retry(&witness_recipient_map(
        &asset.asset_id,
        &receive_data,
        Assignment::Fungible(amount),
        1000,
    ));
    party.wait_for_refresh(None);
    let outputs = party.tx_outputs(&txid);
    assert_eq!(outputs[1].script_pubkey, colored_script);
    assert_eq!(outputs[2].script_pubkey, colored_script);

    mine(false);
    party.wait_for_refresh(None);
    let batch_transfers = party.db_batch_transfers_filtered(&txid);
    assert_eq!(batch_transfers.len(), 2);
    assert!(
        batch_transfers
            .iter()
            .all(|t| t.status == TransferStatus::Settled)
    );
    party.check_receive(
        &asset.asset_id,
        &receive_data,
        amount,
        TransferStatus::Settled,
    );
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance.settled, AMOUNT);
    assert_eq!(balance.spendable, AMOUNT);
    assert!(party.db_txos().iter().all(|t| !t.pending_witness));
}

/// The colored UTXOs a wallet reusing its colored address registers before broadcasting them are
/// not left behind when the broadcast fails.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn create_utxos_broadcast_fail() {
    initialize();

    let mut party = get_reusing_party(false, true);
    let outpoints = |party: &SinglesigParty| -> HashSet<Outpoint> {
        party.db_txos().iter().map(|t| t.outpoint()).collect()
    };
    let outpoints_before = outpoints(&party);

    // a fee rate under the minimum relay one makes the broadcast fail
    MOCK_CHECK_FEE_RATE.replace(vec![true]);
    let result = party
        .wallet
        .create_utxos(party.online, false, Some(1), None, 0, false);
    assert_matches!(result, Err(Error::MinFeeNotMet { .. }));
    assert_eq!(outpoints(&party), outpoints_before);

    // the UTXOs can still be created
    party.create_utxos(false, Some(1), None, FEE_RATE, None);
}
