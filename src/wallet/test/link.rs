use super::*;

#[cfg(feature = "electrum")]
fn link_begin_result(
    party: &mut SinglesigParty,
    asset_id: &str,
    linked_to: &str,
    dry_run: bool,
) -> Result<LinkBeginResult, Error> {
    party.wallet.link_begin(
        party.online,
        asset_id.to_string(),
        linked_to.to_string(),
        FEE_RATE,
        MIN_CONFIRMATIONS,
        false,
        dry_run,
    )
}

#[cfg(feature = "electrum")]
fn link_end_result(
    party: &mut SinglesigParty,
    signed_psbt: &str,
) -> Result<OperationResult, Error> {
    party.wallet.link_end(party.online, signed_psbt.to_string())
}

#[cfg(feature = "electrum")]
fn link_result(
    party: &mut SinglesigParty,
    asset_id: &str,
    linked_to: &str,
) -> Result<OperationResult, Error> {
    party.wallet.link(
        party.online,
        asset_id.to_string(),
        linked_to.to_string(),
        FEE_RATE,
        MIN_CONFIRMATIONS,
        false,
    )
}

/// An IFA asset can be issued with its link right, which can be sent and then spent by its holder
/// to link the asset to another contract (e.g. one replacing it), as the asset metadata reports.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    // more UTXOs for the issuances, each taking one for the link right
    party.create_utxos_default();

    // the contract the asset is linked from (the one it replaces)
    let linked_from = issue_asset_ifa_link(&mut party, None, None).asset_id;

    // issue linking from a contract and with the link right
    let asset = issue_asset_ifa_link(&mut party, Some(linked_from.clone()), None);
    // the contract replacing the asset, which it will be linked to
    let replacement = issue_asset_ifa_link(&mut party, Some(asset.asset_id.clone()), None);
    let linked_to = replacement.asset_id.clone();
    let metadata = party.get_asset_metadata(&asset.asset_id);
    assert_eq!(metadata.linked_from_contract, Some(linked_from.clone()));
    assert_eq!(metadata.linked_to_contract, None);
    assert_eq!(link_rights(&mut party, &asset.asset_id), 1);
    let issuance = party.list_transfers(Some(&asset.asset_id));
    assert!(issuance[0].assignments.contains(&Assignment::LinkRight));

    // the receiver gets some of the replacement contract, which it needs to know to link to it
    send_and_settle(
        &mut party,
        &mut rcv_party,
        &linked_to,
        Assignment::Fungible(1),
    );

    // send the link right to the receiver, which doesn't know the asset yet
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![blind_recipient(&receive_data, Assignment::LinkRight)],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    party.wait_for_transfer_status_sender(Some(&asset.asset_id), &txid, TransferStatus::Settled);
    assert_eq!(link_rights(&mut party, &asset.asset_id), 0);
    assert_eq!(link_rights(&mut rcv_party, &asset.asset_id), 1);
    let rcv_transfer = rcv_party
        .list_transfers(Some(&asset.asset_id))
        .into_iter()
        .find(|t| t.kind == TransferKind::ReceiveBlind)
        .unwrap();
    assert_eq!(rcv_transfer.assignments, vec![Assignment::LinkRight]);
    assert_eq!(
        rcv_party
            .get_asset_metadata(&asset.asset_id)
            .linked_from_contract,
        Some(linked_from.clone())
    );

    // the receiver links the asset, spending the link right
    let res = link_result(&mut rcv_party, &asset.asset_id, &linked_to).unwrap();
    mine(false);
    assert!(rcv_party.refresh_asset(&asset.asset_id));
    let transfer = rcv_party
        .list_transfers(Some(&asset.asset_id))
        .into_iter()
        .find(|t| t.txid.as_ref() == Some(&res.txid))
        .unwrap();
    assert_eq!(transfer.kind, TransferKind::Link);
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(transfer.requested_assignment, Some(Assignment::LinkRight));
    assert_eq!(link_rights(&mut rcv_party, &asset.asset_id), 0);
    let metadata = rcv_party.get_asset_metadata(&asset.asset_id);
    assert_eq!(metadata.linked_from_contract, Some(linked_from));
    assert_eq!(metadata.linked_to_contract, Some(linked_to.clone()));

    // the link consignment proves the link transition, spending the link right
    let inspection = inspect_consignment(
        transfer.consignment_path.clone().unwrap(),
        BundleFilter::Terminals,
    )
    .unwrap();
    let link = inspection
        .witnesses
        .iter()
        .flat_map(|w| w.operations.iter().flat_map(|o| o.transitions.iter()))
        .find(|t| t.r#type == TypeOfTransition::Link)
        .unwrap();
    let inputs: Vec<_> = link.inputs.iter().map(|i| &i.assignment).collect();
    assert_eq!(inputs, vec![&Assignment::LinkRight]);
    assert!(link.outputs.is_empty());
    assert_eq!(
        link.globals,
        vec![TransitionGlobal::LinkedToContract(linked_to)]
    );
}

/// A BFA asset can be issued with its link right too, which its holder can spend to link the
/// asset to another contract, leaving its mint rights untouched.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn bfa_success() {
    initialize();

    let mut party = get_funded_party!();
    // more UTXOs for the issuances, each taking one for the link right
    party.create_utxos_default();

    // the contract the asset is linked from, which has to support links and be unlinked
    let linked_from = issue_asset_ifa_link(&mut party, None, None).asset_id;

    // issue with the link right
    let asset = party
        .wallet
        .issue_asset_bfa(
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            1,
            ANVIL_CHAIN_ID,
            FAKE_EVM_ADDRESS.to_string(),
            None,
            true,
            Some(linked_from.clone()),
            false,
        )
        .unwrap();
    let metadata = party.get_asset_metadata(&asset.asset_id);
    assert_eq!(metadata.linked_from_contract, Some(linked_from.clone()));
    assert_eq!(metadata.linked_to_contract, None);
    assert_eq!(link_rights(&mut party, &asset.asset_id), 1);
    let issuance = party.list_transfers(Some(&asset.asset_id));
    assert!(issuance[0].assignments.contains(&Assignment::LinkRight));
    assert!(issuance[0].assignments.contains(&Assignment::MintRight));

    // the contract to link to, which has to have been issued linked from this asset
    let linked_to = issue_asset_ifa_link(&mut party, Some(asset.asset_id.clone()), None).asset_id;

    // link, spending the link right
    let res = link_result(&mut party, &asset.asset_id, &linked_to).unwrap();
    mine(false);
    assert!(party.refresh_asset(&asset.asset_id));
    let transfer = party
        .list_transfers(Some(&asset.asset_id))
        .into_iter()
        .find(|t| t.txid.as_ref() == Some(&res.txid))
        .unwrap();
    assert_eq!(transfer.kind, TransferKind::Link);
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(link_rights(&mut party, &asset.asset_id), 0);
    let mint_rights = party
        .list_unspents(false)
        .iter()
        .flat_map(|u| u.rgb_allocations.iter())
        .filter(|a| {
            a.asset_id.as_deref() == Some(&asset.asset_id) && a.assignment == Assignment::MintRight
        })
        .count();
    assert_eq!(mint_rights, 1);
    let metadata = party.get_asset_metadata(&asset.asset_id);
    assert_eq!(metadata.linked_from_contract, Some(linked_from));
    assert_eq!(metadata.linked_to_contract, Some(linked_to));
}

/// The link right can be sent beside other assignments, here to a witness recipient, while some
/// assets go to a blind one: the link right is moved by a transfer of link rights beside the
/// transfer moving the amount.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_beside_fungible() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = issue_asset_ifa_link(&mut party, None, None);

    let receive_data_blind = rcv_party.blind_receive();
    let receive_data_witness = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            blind_recipient(&receive_data_blind, Assignment::Fungible(amount)),
            witness_recipient(&receive_data_witness, Assignment::LinkRight, 1000),
        ],
    )]);

    // the operation has a transfer moving some assets and one moving the link right
    let res = party.send_begin_result(&recipient_map).unwrap();
    let inspection = party
        .wallet
        .inspect_rgb_transfer(
            res.psbt.clone(),
            res.details.fascia_path.clone(),
            res.details.entropy,
        )
        .unwrap();
    let mut transition_types: Vec<_> = inspection
        .operations
        .iter()
        .flat_map(|o| o.transitions.iter().map(|t| t.r#type))
        .collect();
    transition_types.sort_by_key(|t| *t as u8);
    assert_eq!(
        transition_types,
        vec![TypeOfTransition::Transfer, TypeOfTransition::TransferLink]
    );
    // complete the inspected send
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let txid = party
        .wallet
        .send_end(party.online, signed_psbt)
        .unwrap()
        .txid;
    assert!(!txid.is_empty());
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    party.wait_for_transfer_status_sender(Some(&asset.asset_id), &txid, TransferStatus::Settled);

    // the receiver got both, each on the invoice it was sent to
    assert_eq!(link_rights(&mut party, &asset.asset_id), 0);
    assert_eq!(link_rights(&mut rcv_party, &asset.asset_id), 1);
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, amount);
    let rcv_transfers = rcv_party.list_transfers(Some(&asset.asset_id));
    let assignments_of = |kind: TransferKind| {
        rcv_transfers
            .iter()
            .find(|t| t.kind == kind)
            .unwrap()
            .assignments
            .clone()
    };
    assert_eq!(
        assignments_of(TransferKind::ReceiveBlind),
        vec![Assignment::Fungible(amount)]
    );
    assert_eq!(
        assignments_of(TransferKind::ReceiveWitness),
        vec![Assignment::LinkRight]
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    // more UTXOs to have free allocation slots
    party.create_utxos(false, Some(UTXO_NUM * 2), None, FEE_RATE, None);

    let asset_nia = party.issue_asset_nia(None);
    let asset_no_link = party.issue_asset_ifa(None, None, None);
    let (asset, replacement) = issue_asset_ifa_link_pair(&mut party);
    // a contract of another wallet, which this one doesn't know
    let unknown_contract = rcv_party.issue_asset_ifa(None, None, None).asset_id;

    // issue an IFA asset with the link right, linked from the contract with the given ID
    let issue_linked_from = |party: &mut SinglesigParty, linked_from: &str| {
        party.wallet.issue_asset_ifa(
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
            vec![],
            None,
            true,
            Some(linked_from.to_string()),
            false,
        )
    };

    // issuance: invalid contract the asset is linked from
    let result = issue_linked_from(&mut party, "invalid");
    assert_matches!(result, Err(Error::InvalidContractId { contract_id }) if contract_id == "invalid");
    // issuance: contract the asset is linked from unknown to the wallet
    let result = issue_linked_from(&mut party, &unknown_contract);
    assert_matches!(
        result,
        Err(Error::UnknownLinkedContract { ref contract_id }) if contract_id == &unknown_contract
    );
    // issuance: contract the asset is linked from cannot link (its schema has no link right)
    let result = issue_linked_from(&mut party, &asset_nia.asset_id);
    assert_matches!(
        result,
        Err(Error::InvalidLinkedContract { ref contract_id, .. })
            if contract_id == &asset_nia.asset_id
    );

    // a BFA issuance can be linked from a contract too, and checks it the same way
    let issue_bfa_linked_from = |party: &mut SinglesigParty, linked_from: &str| {
        party.wallet.issue_asset_bfa(
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            1,
            ANVIL_CHAIN_ID,
            FAKE_EVM_ADDRESS.to_string(),
            None,
            true,
            Some(linked_from.to_string()),
            false,
        )
    };
    let result = issue_bfa_linked_from(&mut party, "invalid");
    assert_matches!(result, Err(Error::InvalidContractId { contract_id }) if contract_id == "invalid");
    let result = issue_bfa_linked_from(&mut party, &unknown_contract);
    assert_matches!(
        result,
        Err(Error::UnknownLinkedContract { ref contract_id }) if contract_id == &unknown_contract
    );
    let result = issue_bfa_linked_from(&mut party, &asset_nia.asset_id);
    assert_matches!(
        result,
        Err(Error::InvalidLinkedContract { ref contract_id, .. })
            if contract_id == &asset_nia.asset_id
    );

    // link: unknown asset
    let result = link_begin_result(&mut party, "rgb1inexistent", &asset.asset_id, true);
    assert_matches!(result, Err(Error::AssetNotFound { .. }));
    // link: schema without a link right
    let result = link_begin_result(&mut party, &asset_nia.asset_id, &asset.asset_id, true);
    assert_matches!(
        result,
        Err(Error::UnsupportedLink {
            asset_schema: AssetSchema::Nia
        })
    );
    // link: invalid contract to link to
    let result = link_begin_result(&mut party, &asset.asset_id, "invalid", true);
    assert_matches!(result, Err(Error::InvalidContractId { contract_id }) if contract_id == "invalid");
    // link: the asset's own contract
    let result = link_begin_result(&mut party, &asset.asset_id, &asset.asset_id, true);
    assert_matches!(result, Err(Error::CannotLinkToItself));
    // link: contract to link to unknown to the wallet
    let result = link_begin_result(&mut party, &asset.asset_id, &unknown_contract, true);
    assert_matches!(
        result,
        Err(Error::UnknownLinkedContract { ref contract_id }) if contract_id == &unknown_contract
    );
    // link: contract to link to not issued linked from the asset
    let result = link_begin_result(&mut party, &asset.asset_id, &asset_nia.asset_id, true);
    assert_matches!(
        result,
        Err(Error::InvalidLinkedContract { ref contract_id, .. })
            if contract_id == &asset_nia.asset_id
    );
    // link: no link right (never had one)
    let no_link_replacement =
        issue_asset_ifa_link(&mut party, Some(asset_no_link.asset_id.clone()), None).asset_id;
    let result = link_begin_result(
        &mut party,
        &asset_no_link.asset_id,
        &no_link_replacement,
        true,
    );
    assert_matches!(result, Err(Error::InsufficientAssignments { .. }));

    // link_end: invalid PSBT
    let result = link_end_result(&mut party, "");
    assert_matches!(result, Err(Error::InvalidPsbt { .. }));
    // link_end: no prior link_begin
    let res = link_begin_result(&mut party, &asset.asset_id, &replacement.asset_id, true).unwrap();
    let signed_link_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let link_txid = psbt_txid(&signed_link_psbt);
    let mut party_2 = get_empty_party!();
    let result = link_end_result(&mut party_2, &signed_link_psbt);
    assert_matches!(result, Err(Error::UnknownTransfer { txid }) if txid == link_txid);
    // link_end: PSBT of another operation
    let unsigned_psbt = party.burn_begin(&asset.asset_id, 1, None);
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt, None).unwrap();
    let txid = psbt_txid(&signed_psbt);
    let result = link_end_result(&mut party, &signed_psbt);
    assert_matches!(result, Err(Error::WrongOperation { txid: t }) if t == txid);
    // another operation's end: PSBT of a link
    let result = party.burn_end_result(&signed_link_psbt);
    assert_matches!(result, Err(Error::WrongOperation { txid: t }) if t == link_txid);

    // send: the link right to more than one recipient
    let receive_data_1 = rcv_party.blind_receive();
    let receive_data_2 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            blind_recipient(&receive_data_1, Assignment::LinkRight),
            blind_recipient(&receive_data_2, Assignment::LinkRight),
        ],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert_matches!(result, Err(Error::InvalidRecipientData { .. }));

    // link: no link right (sent to another wallet)
    let (asset_sent, replacement_sent) = issue_asset_ifa_link_pair(&mut party);
    send_and_settle(
        &mut party,
        &mut rcv_party,
        &asset_sent.asset_id,
        Assignment::LinkRight,
    );
    let result = link_begin_result(
        &mut party,
        &asset_sent.asset_id,
        &replacement_sent.asset_id,
        true,
    );
    assert_matches!(result, Err(Error::InsufficientAssignments { .. }));

    // link: an asset already linked
    link_result(&mut party, &asset.asset_id, &replacement.asset_id).unwrap();
    mine(false);
    assert!(party.refresh_asset(&asset.asset_id));
    let result = link_begin_result(&mut party, &asset.asset_id, &replacement.asset_id, true);
    assert_matches!(result, Err(Error::InsufficientAssignments { .. }));

    // issuance: contract the asset is linked from already linked to another contract
    let result = issue_linked_from(&mut party, &asset.asset_id);
    assert_matches!(
        result,
        Err(Error::InvalidLinkedContract { ref contract_id, .. }) if contract_id == &asset.asset_id
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_end() {
    initialize();

    let mut party = get_funded_party!();
    let (asset, replacement) = issue_asset_ifa_link_pair(&mut party);
    let linked_to = replacement.asset_id;

    // begin does not update backup_info with dry_run=true
    let bak_info_before = party.db_backup_info();
    link_begin_result(&mut party, &asset.asset_id, &linked_to, true).unwrap();
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // begin does update backup_info with dry_run=false
    let bak_info_before = party.db_backup_info();
    let res = link_begin_result(&mut party, &asset.asset_id, &linked_to, false).unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();

    // end updates backup_info
    let bak_info_before = party.db_backup_info();
    link_end_result(&mut party, &signed_psbt).unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
}

/// when `dry_run` is `false`, `link_begin` persists the operation to DB as `Initiated`, before
/// generating the consignment: `link_end` still completes it, recording the link only then
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn no_dry_run() {
    initialize();

    let mut party = get_funded_party!();
    let (asset, replacement) = issue_asset_ifa_link_pair(&mut party);
    let linked_to = replacement.asset_id;

    let res = link_begin_result(&mut party, &asset.asset_id, &linked_to, false).unwrap();
    let batch_transfer_idx = res.batch_transfer_idx.unwrap();
    let txid = psbt_txid(&res.psbt);
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Initiated));
    assert_eq!(
        party.get_asset_metadata(&asset.asset_id).linked_to_contract,
        None
    );

    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let end_res = link_end_result(&mut party, &signed_psbt).unwrap();
    assert_eq!(end_res.batch_transfer_idx, batch_transfer_idx);
    assert_eq!(
        party.get_asset_metadata(&asset.asset_id).linked_to_contract,
        Some(linked_to.clone())
    );

    // settle
    mine(false);
    assert!(party.refresh_asset(&asset.asset_id));
    let transfers = party.list_transfers(Some(&asset.asset_id));
    let transfer = transfers.last().unwrap();
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(transfer.kind, TransferKind::Link);
    assert_eq!(transfer.txid.as_ref().unwrap(), &end_res.txid);
    assert!(transfer.consignment_path.is_some());
    assert_eq!(link_rights(&mut party, &asset.asset_id), 0);

    // the consignment_hash backfill in update_or_save_transfers must have populated the DB row
    // even though the file didn't exist yet when link_begin (dry_run=false) first persisted it
    let asset_transfer = party
        .db_asset_transfers_filtered(batch_transfer_idx)
        .into_iter()
        .find(|a| a.asset_id.as_deref() == Some(asset.asset_id.as_str()))
        .unwrap();
    let db_transfer = party
        .db_transfers_filtered(asset_transfer.idx)
        .into_iter()
        .next()
        .unwrap();
    assert!(db_transfer.consignment_hash.is_some());
}

/// A link ended again (e.g. retried after a crash) is completed only while it's still initiated:
/// one that has failed or already been ended must not be broadcast again nor have its status
/// changed. A failed link releases the link right and links nothing.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn end_after_fail_or_end() {
    initialize();

    let mut party = get_funded_party!();
    let (asset, replacement) = issue_asset_ifa_link_pair(&mut party);
    let linked_to = replacement.asset_id;

    // begin a link and sign its PSBT
    let begin = |party: &mut SinglesigParty| {
        let res = link_begin_result(party, &asset.asset_id, &linked_to, false).unwrap();
        let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
        let txid = psbt_txid(&signed_psbt);
        (res.batch_transfer_idx.unwrap(), txid, signed_psbt)
    };

    // a failed link is not ended
    let (batch_transfer_idx, txid, signed_psbt) = begin(&mut party);
    assert!(party.fail_transfers_single(batch_transfer_idx));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));
    let result = link_end_result(&mut party, &signed_psbt);
    assert_matches!(result, Err(Error::CannotEndTransfer { txid: t }) if t == txid);
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));
    assert_eq!(
        party.get_asset_metadata(&asset.asset_id).linked_to_contract,
        None
    );

    // an ended link is not ended again
    let (_, txid, signed_psbt) = begin(&mut party);
    link_end_result(&mut party, &signed_psbt).unwrap();
    mine(false);
    party.wait_for_transfer_status_sender(None, &txid, TransferStatus::Settled);
    let result = link_end_result(&mut party, &signed_psbt);
    assert_matches!(result, Err(Error::CannotEndTransfer { txid: t }) if t == txid);
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Settled));
    assert_eq!(
        party.get_asset_metadata(&asset.asset_id).linked_to_contract,
        Some(linked_to)
    );
}

/// A link doesn't spend a link right rejected by the asset's reject list, and once the link right
/// is allowed again the link consignment carries the link transition.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn ifa_reject_list() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let list_name = "reject_link.list";
    write_opouts_to_reject_list(list_name, &[]);
    let asset = issue_asset_ifa_link(
        &mut party,
        None,
        Some(format!("http://localhost:8140/lists/{list_name}")),
    );
    // the contract to be linked to, which the receiver gets some of, to be able to check it
    let linked_to = issue_asset_ifa_link(&mut party, Some(asset.asset_id.clone()), None).asset_id;
    send_and_settle(
        &mut party,
        &mut rcv_party,
        &linked_to,
        Assignment::Fungible(1),
    );

    // the receiver gets the link right, which is then rejected
    let txid = send_and_settle(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        Assignment::LinkRight,
    );
    let opouts = rcv_party.extract_opouts_from_transfer(&asset.asset_id, &txid);
    assert_eq!(opouts.len(), 1);
    write_opouts_to_reject_list(list_name, &[opouts[0].to_string()]);
    let result = link_result(&mut rcv_party, &asset.asset_id, &linked_to);
    assert_matches!(
        result,
        Err(Error::InsufficientAssignments { asset_id: ref t, .. }) if t == &asset.asset_id
    );
    assert_eq!(link_rights(&mut rcv_party, &asset.asset_id), 1);

    // once allowed by a later entry of the list, the link right is spent
    write_opouts_to_reject_list(
        list_name,
        &[opouts[0].to_string(), format!("!{}", opouts[0])],
    );
    let res = link_result(&mut rcv_party, &asset.asset_id, &linked_to).unwrap();
    mine(false);
    assert!(rcv_party.refresh_asset(&asset.asset_id));
    let transfer = rcv_party
        .list_transfers(Some(&asset.asset_id))
        .into_iter()
        .find(|t| t.txid.as_ref() == Some(&res.txid))
        .unwrap();
    assert_eq!(transfer.kind, TransferKind::Link);
    assert_eq!(transfer.status, TransferStatus::Settled);
    let inspection = inspect_consignment(
        transfer.consignment_path.clone().unwrap(),
        BundleFilter::Terminals,
    )
    .unwrap();
    assert!(
        inspection
            .witnesses
            .iter()
            .flat_map(|w| w.operations.iter().flat_map(|o| o.transitions.iter()))
            .any(|t| t.r#type == TypeOfTransition::Link)
    );
}

/// A link can spend a link right held by a UTXO with the maximum number of allocations of the
/// asset, as it doesn't split its inputs like a burn, which needs a slot free for that.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn link_right_utxo_allocations_limit() {
    initialize();

    let mut party = get_funded_party!();
    let (asset, replacement) = issue_asset_ifa_link_pair(&mut party);
    let linked_to = replacement.asset_id;

    // the UTXO holding the link right holds the maximum number of allocations of the asset
    MOCK_MAX_UTXO_ASSET_ALLOCATIONS.replace(Some(1));
    let res = link_result(&mut party, &asset.asset_id, &linked_to).unwrap();
    mine(false);
    assert!(party.refresh_asset(&asset.asset_id));
    let transfer = party
        .list_transfers(Some(&asset.asset_id))
        .into_iter()
        .find(|t| t.txid.as_ref() == Some(&res.txid))
        .unwrap();
    assert_eq!(transfer.kind, TransferKind::Link);
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(link_rights(&mut party, &asset.asset_id), 0);
}

/// The checks on the contract that is linked from or to can be skipped, to link contracts the
/// wallet cannot check: one it doesn't know, or one not pointing back, e.g. of a different
/// schema.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_contract_checks() {
    initialize();

    let mut party = get_funded_party!();
    let mut other_party = get_funded_party!();

    // issue linked from a contract of another wallet, which this one doesn't know
    let linked_from = other_party.issue_asset_ifa(None, None, None).asset_id;
    let asset = party
        .wallet
        .issue_asset_ifa(
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
            vec![],
            None,
            true,
            Some(linked_from.clone()),
            true,
        )
        .unwrap();
    assert_eq!(
        party
            .get_asset_metadata(&asset.asset_id)
            .linked_from_contract,
        Some(linked_from)
    );

    // link to a contract of another schema, not issued linked from (the linking contract)
    let linked_to = party.issue_asset_nia(None).asset_id;
    let res = party
        .wallet
        .link(
            party.online,
            asset.asset_id.clone(),
            linked_to.clone(),
            FEE_RATE,
            MIN_CONFIRMATIONS,
            true,
        )
        .unwrap();
    mine(false);
    assert!(party.refresh_asset(&asset.asset_id));
    let transfer = party
        .list_transfers(Some(&asset.asset_id))
        .into_iter()
        .find(|t| t.txid.as_ref() == Some(&res.txid))
        .unwrap();
    assert_eq!(transfer.kind, TransferKind::Link);
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(
        party.get_asset_metadata(&asset.asset_id).linked_to_contract,
        Some(linked_to)
    );
}
