use super::*;

fn fungible_sum<'a>(assignments: impl Iterator<Item = &'a Assignment>) -> u64 {
    assignments.map(|a| a.main_amount()).sum()
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount_blind: u64 = 66;
    let amount_witness: u64 = 77;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // send to a blinded and a witness recipient
    let asset = party.issue_asset_nia(None);
    let receive_data_blind = rcv_party.blind_receive();
    let receive_data_witness = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount_blind),
                recipient_id: receive_data_blind.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                public_key: Some(receive_data_blind.public_key.clone()),
                expiration_timestamp: Some(receive_data_blind.expiration_timestamp),
                encryption: Encryption::Optional,
            },
            Recipient {
                assignment: Assignment::Fungible(amount_witness),
                recipient_id: receive_data_witness.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                public_key: Some(receive_data_witness.public_key.clone()),
                expiration_timestamp: Some(receive_data_witness.expiration_timestamp),
                encryption: Encryption::Optional,
            },
        ],
    )]);
    let txid_1 = party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid_1)
        .to_string_lossy()
        .to_string();

    // inspect the consignment
    let inspection =
        inspect_consignment(consignment_path.clone(), BundleFilter::Terminals).unwrap();
    let consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    assert_eq!(
        inspection.consignment_id,
        consignment.consignment_id().to_string()
    );
    assert_eq!(inspection.asset_id, asset.asset_id);
    assert_eq!(inspection.asset_schema, AssetSchema::Nia);
    let genesis = &inspection.genesis;
    assert_eq!(genesis.chain_net, "bcrt");
    assert_eq!(genesis.timestamp, asset.timestamp);
    assert_eq!(genesis.name, asset.name);
    assert_eq!(genesis.ticker.as_ref(), Some(&asset.ticker));
    assert_eq!(genesis.details, asset.details);
    assert_eq!(genesis.precision, asset.precision);
    assert_eq!(genesis.outputs.len(), 1);
    let issuance_output = &genesis.outputs[0];
    assert_eq!(issuance_output.assignment, Assignment::Fungible(AMOUNT));
    assert!(issuance_output.txid.is_some() && issuance_output.vout.is_some());
    assert!(!issuance_output.is_concealed);
    assert_eq!(issuance_output.recipient_id, None);
    assert_eq!(issuance_output.is_ours, None);
    assert_eq!(issuance_output.is_terminal, Some(false));
    assert_eq!(inspection.version, 1);
    assert!(inspection.is_transfer);
    assert_eq!(inspection.witnesses.len(), 1);
    let witness = &inspection.witnesses[0];
    assert_eq!(witness.txid, txid_1);
    assert_eq!(witness.commitment_hex.len(), 64);
    assert_eq!(witness.operations.len(), 1);
    let operation = &witness.operations[0];
    assert_eq!(operation.asset_id, asset.asset_id);
    assert_eq!(operation.committed_transition_count, 1);
    assert_eq!(operation.committed_input_count, 1);
    assert_eq!(operation.transitions.len(), 1);
    let transition = &operation.transitions[0];
    assert_eq!(transition.r#type, TypeOfTransition::Transfer);
    assert!(transition.globals.is_empty());
    assert!(transition.metadata.is_empty());
    // the input is the issuance allocation
    assert_eq!(transition.inputs.len(), 1);
    assert_eq!(
        transition.inputs[0].assignment,
        Assignment::Fungible(AMOUNT)
    );
    // outputs: the two terminals and the change
    assert_eq!(transition.outputs.len(), 3);
    assert!(transition.outputs.iter().all(|o| o.is_ours.is_none()));
    let blind_output = transition.outputs.iter().find(|o| o.is_concealed).unwrap();
    assert_eq!(blind_output.assignment, Assignment::Fungible(amount_blind));
    assert_eq!(blind_output.vout, None);
    assert_eq!(blind_output.is_terminal, Some(true));
    assert_eq!(blind_output.txid, None);
    assert_eq!(
        blind_output.recipient_id.as_ref(),
        Some(&receive_data_blind.recipient_id)
    );
    let witness_output = transition
        .outputs
        .iter()
        .find(|o| o.assignment == Assignment::Fungible(amount_witness))
        .unwrap();
    assert!(!witness_output.is_concealed);
    assert!(witness_output.vout.is_some());
    assert_eq!(witness_output.is_terminal, Some(true));
    assert_eq!(witness_output.txid.as_ref(), Some(&txid_1));
    assert_eq!(witness_output.recipient_id, None);
    let change_output = transition
        .outputs
        .iter()
        .find(|o| o.is_terminal == Some(false))
        .unwrap();
    assert!(!change_output.is_concealed);
    assert_eq!(change_output.txid.as_ref(), Some(&txid_1));
    assert_eq!(
        change_output.assignment,
        Assignment::Fungible(AMOUNT - amount_blind - amount_witness)
    );

    // the contract consignment carries the genesis only
    let contract_file = tempfile::NamedTempFile::with_prefix("inspect_consignment::").unwrap();
    consignment
        .clone()
        .into_contract()
        .save_file(contract_file.path())
        .unwrap();
    let contract_inspection = inspect_consignment(
        contract_file.path().to_string_lossy().to_string(),
        BundleFilter::Terminals,
    )
    .unwrap();
    assert!(!contract_inspection.is_transfer);
    assert_eq!(contract_inspection.asset_id, asset.asset_id);
    assert_eq!(contract_inspection.genesis.name, asset.name);
    assert_eq!(contract_inspection.genesis.outputs.len(), 1);
    assert!(contract_inspection.witnesses.is_empty());

    // settle the transfer
    rcv_party.wait_for_refresh(None);

    // the validated consignment kept by the recipient while the transfer is pending is inspected
    // the same way, skipping the checks
    let rcv_consignment_path = rcv_party
        .wallet
        .get_receive_consignment_path(&receive_data_blind.recipient_id);
    let valid_consignment_path = rcv_party
        .wallet
        .get_receive_valid_consignment_path(&rcv_consignment_path);
    let valid_inspection = inspect_consignment(
        valid_consignment_path.to_string_lossy().to_string(),
        BundleFilter::Terminals,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&valid_inspection).unwrap(),
        serde_json::to_value(&inspection).unwrap()
    );

    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    // send the received allocations onward: inputs are resolved from the consignment history
    let receive_data = party.blind_receive();
    let amount_back = amount_blind + amount_witness;
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_back),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid_2 = rcv_party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = rcv_party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid_2)
        .to_string_lossy()
        .to_string();
    let inspection =
        inspect_consignment(consignment_path.clone(), BundleFilter::Terminals).unwrap();
    // only the witness of the terminals is inspected
    assert_eq!(inspection.witnesses.len(), 1);
    assert_eq!(inspection.witnesses[0].txid, txid_2);
    // all witnesses are inspected when requested, the history one having no terminals
    let full_inspection = inspect_consignment(consignment_path.clone(), BundleFilter::All).unwrap();
    assert_eq!(full_inspection.witnesses.len(), 2);
    let witness_1 = full_inspection
        .witnesses
        .iter()
        .find(|w| w.txid == txid_1)
        .unwrap();
    assert_eq!(witness_1.close_method, witness.close_method);
    assert_eq!(witness_1.commitment_hex, witness.commitment_hex);
    assert!(
        witness_1.operations[0].transitions[0]
            .outputs
            .iter()
            .all(|o| o.is_terminal == Some(false))
    );
    let full_witness_2 = full_inspection
        .witnesses
        .iter()
        .find(|w| w.txid == txid_2)
        .unwrap();
    assert_eq!(
        serde_json::to_value(full_witness_2).unwrap(),
        serde_json::to_value(&inspection.witnesses[0]).unwrap()
    );
    // a single witness can be selected, also from the history
    let witness_1_inspection = inspect_consignment(
        consignment_path.clone(),
        BundleFilter::Witness(txid_1.clone()),
    )
    .unwrap();
    assert_eq!(witness_1_inspection.witnesses.len(), 1);
    assert_eq!(
        serde_json::to_value(&witness_1_inspection.witnesses[0]).unwrap(),
        serde_json::to_value(witness_1).unwrap()
    );
    // a history transition not having the operation ID it's listed with is not used to resolve
    // the inputs spending it, which would show assignments they don't spend
    let mut consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    let history_witness_id = consignment
        .bundled_witnesses()
        .map(|bw| bw.witness_id())
        .find(|id| id.to_string() == txid_1)
        .unwrap();
    assert!(
        consignment.modify_bundle(history_witness_id, |witness_bundle| {
            let bundle = witness_bundle.bundle_mut();
            let mut known_transitions = bundle.known_transitions.to_unconfined();
            known_transitions[0].transition.nonce =
                known_transitions[0].transition.nonce.wrapping_add(1);
            bundle.known_transitions = Confined::from_iter_checked(known_transitions);
        })
    );
    let forged_file = tempfile::NamedTempFile::with_prefix("inspect_consignment::").unwrap();
    consignment.save_file(forged_file.path()).unwrap();
    let result = inspect_consignment(
        forged_file.path().to_string_lossy().to_string(),
        BundleFilter::Terminals,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("unknown input"));
    let result = inspect_consignment(consignment_path, BundleFilter::Witness("00".repeat(32)));
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("not found"));
    // the new witness spends both received allocations
    let witness_2 = inspection
        .witnesses
        .iter()
        .find(|w| w.txid == txid_2)
        .unwrap();
    assert_eq!(witness_2.operations.len(), 1);
    let operation = &witness_2.operations[0];
    assert_eq!(operation.transitions.len(), 1);
    assert_eq!(operation.committed_transition_count, 1);
    assert_eq!(operation.committed_input_count, 2);
    let transition = &operation.transitions[0];
    assert_eq!(transition.inputs.len(), 2);
    let mut input_assignments: Vec<_> = transition.inputs.iter().map(|i| &i.assignment).collect();
    input_assignments.sort();
    assert_eq!(
        input_assignments,
        vec![
            &Assignment::Fungible(amount_blind),
            &Assignment::Fungible(amount_witness)
        ]
    );
    assert_ne!(transition.inputs[0].vin, transition.inputs[1].vin);
    assert_eq!(transition.outputs.len(), 1);
    assert_eq!(
        transition.outputs[0].assignment,
        Assignment::Fungible(amount_back)
    );
    assert!(transition.outputs[0].is_concealed);
    assert_eq!(transition.outputs[0].is_terminal, Some(true));
    assert_eq!(
        fungible_sum(transition.inputs.iter().map(|i| &i.assignment)),
        fungible_sum(transition.outputs.iter().map(|o| &o.assignment))
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

    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid = party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid);
    let invalid_file = tempfile::NamedTempFile::with_prefix("inspect_consignment::").unwrap();

    // a bundle not committed by its witness
    let mut consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    let witness_id = consignment.bundled_witnesses().next().unwrap().witness_id();
    assert!(consignment.modify_bundle(witness_id, |witness_bundle| {
        for output in witness_bundle.tx.output.iter_mut() {
            output.script_pubkey = ScriptBuf::new();
        }
    }));
    consignment.save_file(invalid_file.path()).unwrap();
    let result = inspect_consignment(
        invalid_file.path().to_string_lossy().to_string(),
        BundleFilter::Terminals,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("doesn't commit to"));

    // a transition not committed by its bundle
    let mut consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    assert!(consignment.modify_bundle(witness_id, |witness_bundle| {
        let bundle = witness_bundle.bundle_mut();
        let mut known_transitions = bundle.known_transitions.to_unconfined();
        known_transitions[0].transition.nonce =
            known_transitions[0].transition.nonce.wrapping_add(1);
        bundle.known_transitions = Confined::from_iter_checked(known_transitions);
    }));
    consignment.save_file(invalid_file.path()).unwrap();
    let result = inspect_consignment(
        invalid_file.path().to_string_lossy().to_string(),
        BundleFilter::Terminals,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("is not committed"));
}
