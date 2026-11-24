use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);
    let asset_2 = party.issue_asset_nia(None);
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
    let res = party.send_begin_result(&recipient_map).unwrap();
    let details = res.details;
    party
        .wallet
        .inspect_fascia(
            res.psbt.clone(),
            details.fascia_path.clone(),
            Some(details.entropy),
            MAX_FASCIA_SIZE,
            MAX_FASCIA_BUNDLES,
        )
        .unwrap();

    // an entropy that isn't the one the fascia carries
    let result = party.wallet.inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy.wrapping_add(1)),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d })
        if d.contains(&format!("built with entropy {}, not the given", details.entropy)));

    // inspect the PSBT against a tampered version of the fascia the wallet saved
    let fascia = read_fascia(Path::new(&details.fascia_path)).unwrap();
    let fascia_json = serde_json::to_value(&fascia).unwrap();
    let inspect_tampered = |wallet: &Wallet, tampered_json: Value, entropy: Option<u64>| {
        let tampered_fascia: Fascia = serde_json::from_value(tampered_json).unwrap();
        let tampered_file = tempfile::NamedTempFile::with_prefix("inspect_fascia::").unwrap();
        write_fascia(tampered_file.path(), &tampered_fascia).unwrap();
        wallet.inspect_fascia(
            res.psbt.clone(),
            tampered_file.path().to_string_lossy().to_string(),
            entropy,
            MAX_FASCIA_SIZE,
            MAX_FASCIA_BUNDLES,
        )
    };

    // a transition not committed by its bundle: the bundle commitment still matches the PSBT one
    let mut tampered_json = fascia_json.clone();
    let transition = tampered_json["bundles"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()["knownTransitions"][0]["transition"]
        .as_object_mut()
        .unwrap();
    let nonce = transition["nonce"].as_u64().unwrap();
    transition.insert(s!("nonce"), nonce.wrapping_add(1).into());
    let result = inspect_tampered(&party.wallet, tampered_json, Some(details.entropy));
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("is not committed"));

    // a bundle filed under the contract of another asset: nothing commits to the contract, so the
    // PSBT still commits to the bundle, which carries a transition of the sent asset
    let mut tampered_json = fascia_json.clone();
    let bundles = tampered_json["bundles"].as_object_mut().unwrap();
    assert_eq!(bundles.keys().collect::<Vec<_>>(), vec![&asset.asset_id]);
    let bundle = bundles.values().next().unwrap().clone();
    *bundles = serde_json::Map::from_iter([(asset_2.asset_id.clone(), bundle)]);
    let result = inspect_tampered(&party.wallet, tampered_json, Some(details.entropy));
    assert_matches!(result, Err(Error::RgbInspection { details: d })
        if d.contains(&format!("is of contract {}", asset.asset_id)));

    // a bundle listing the same transition twice: the bundle ID only commits to the input map, so
    // it's unchanged and the PSBT still commits to the bundle
    let mut tampered_json = fascia_json.clone();
    let known_transitions = tampered_json["bundles"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()["knownTransitions"]
        .as_array_mut()
        .unwrap();
    known_transitions.push(known_transitions[0].clone());
    let result = inspect_tampered(&party.wallet, tampered_json, Some(details.entropy));
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("is listed twice"));

    // a fascia file larger than the given maximum, with or without a wallet, while one as large is
    // inspected
    let fascia_size = fs::metadata(&details.fascia_path).unwrap().len();
    let result = party.wallet.inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy),
        fascia_size - 1,
        MAX_FASCIA_BUNDLES,
    );
    assert_matches!(result, Err(Error::FileTooLarge { file_path, max_size })
        if file_path == details.fascia_path && max_size == fascia_size - 1);
    let result = inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy),
        fascia_size - 1,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Err(Error::FileTooLarge { file_path, max_size })
        if file_path == details.fascia_path && max_size == fascia_size - 1);
    party
        .wallet
        .inspect_fascia(
            res.psbt.clone(),
            details.fascia_path.clone(),
            Some(details.entropy),
            fascia_size,
            MAX_FASCIA_BUNDLES,
        )
        .unwrap();

    // a fascia file with data after the fascia
    let trailing_file = tempfile::NamedTempFile::with_prefix("inspect_fascia::").unwrap();
    let mut trailing_bytes = fs::read(&details.fascia_path).unwrap();
    trailing_bytes.push(0);
    fs::write(trailing_file.path(), trailing_bytes).unwrap();
    let trailing_path = trailing_file.path().to_string_lossy().to_string();
    let result = inspect_fascia(
        res.psbt.clone(),
        trailing_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Err(Error::InvalidFilePath { file_path }) if file_path == trailing_path);

    // a fascia with more bundles than the given maximum, with or without a wallet, while one with
    // as many is inspected
    let result = party.wallet.inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        0,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d })
        if d == "fascia bundle count 1 exceeds the maximum of 0");
    let result = inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        0,
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d })
        if d == "fascia bundle count 1 exceeds the maximum of 0");
    party
        .wallet
        .inspect_fascia(
            res.psbt.clone(),
            details.fascia_path.clone(),
            Some(details.entropy),
            MAX_FASCIA_SIZE,
            1,
        )
        .unwrap();

    // a fascia whose MPC commitment doesn't carry the entropy it was built with can only be
    // inspected by giving the entropy
    let mut tampered_json = fascia_json;
    tampered_json["sealWitness"]["mpcMerkleBlock"]["entropy"] = Value::Null;
    let result = inspect_tampered(&party.wallet, tampered_json.clone(), None);
    assert_matches!(result, Err(Error::RgbInspection { details: d })
        if d.contains("doesn't carry the entropy"));
    inspect_tampered(&party.wallet, tampered_json.clone(), Some(details.entropy)).unwrap();

    // with no entropy to check the given one against, a wrong one is only caught by the
    // commitment it gives
    let result = inspect_tampered(
        &party.wallet,
        tampered_json,
        Some(details.entropy.wrapping_add(1)),
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("doesn't commit to"));
}

/// An input spending another transition of the same bundle is resolved by the bundle itself, so
/// concealing the seal it's assigned to, which changes no commitment, makes the inspection fail,
/// also without a wallet.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail_concealed_bundle_input() {
    initialize();

    let mut party = get_funded_party!();

    // a burn, which only an inflatable or bridged asset supports, can't return change, so its
    // inputs are first split by another transition of the same bundle, whose output it spends
    let asset = party.issue_asset_ifa(None, None, None);
    party.create_utxos_default();
    let res = party
        .burn_begin_result(&asset.asset_id, AMOUNT_SMALL, None)
        .unwrap();
    let details = res.details;
    party
        .wallet
        .inspect_fascia(
            res.psbt.clone(),
            details.fascia_path.clone(),
            Some(details.entropy),
            MAX_FASCIA_SIZE,
            MAX_FASCIA_BUNDLES,
        )
        .unwrap();

    // the split transition, the one whose output the burn spends
    let fascia = read_fascia(Path::new(&details.fascia_path)).unwrap();
    let bundle = fascia.bundles().values().next().unwrap();
    assert_eq!(bundle.known_transitions.len(), 2);
    let spent_opids: Vec<OpId> = bundle
        .known_transitions
        .iter()
        .flat_map(|kt| kt.transition.inputs().iter().map(|i| i.op))
        .collect();
    let split = bundle
        .known_transitions
        .iter()
        .find(|kt| spent_opids.contains(&kt.opid))
        .unwrap();

    // conceal the seals of the split: the operation ID commits to the concealed seals, so the
    // bundle ID, and the commitment the PSBT carries, are unchanged
    let mut fascia_json = serde_json::to_value(&fascia).unwrap();
    let known_transitions = fascia_json["bundles"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()["knownTransitions"]
        .as_array_mut()
        .unwrap();
    let split_json = known_transitions
        .iter_mut()
        .find(|kt| kt["opid"] == serde_json::to_value(split.opid).unwrap())
        .unwrap();
    let assignments_json = split_json["transition"]["assignments"]
        .as_object_mut()
        .unwrap();
    for (ass_type, assignment_json) in assignments_json.iter_mut() {
        let ass_type = AssignmentType::with(ass_type.parse().unwrap());
        let assignment = split.transition.assignments.get(&ass_type).unwrap();
        *assignment_json = serde_json::to_value(assignment.conceal()).unwrap();
    }
    let tampered_fascia: Fascia = serde_json::from_value(fascia_json).unwrap();
    let tampered_file = tempfile::NamedTempFile::with_prefix("inspect_fascia::").unwrap();
    write_fascia(tampered_file.path(), &tampered_fascia).unwrap();
    let tampered_path = tampered_file.path().to_string_lossy().to_string();

    // the allocation the burn spends can no longer be resolved, with or without a wallet
    let result = party.wallet.inspect_fascia(
        res.psbt.clone(),
        tampered_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("isn't assigned by the bundle"));
    let result = inspect_fascia(
        res.psbt,
        tampered_path,
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("isn't assigned by the bundle"));
}

/// A fascia can be inspected without a wallet: the inspection reports the same operations as the
/// wallet one, minus what only a wallet knows (the allocations the transitions spend and which
/// outputs are its own).
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn without_wallet() {
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
            blind_recipient(&receive_data_blind, Assignment::Fungible(amount_blind)),
            witness_recipient(
                &receive_data_witness,
                Assignment::Fungible(amount_witness),
                1000,
            ),
        ],
    )]);
    let res = party.send_begin_result(&recipient_map).unwrap();
    let details = res.details;

    // the wallet-less inspection reports the same commitment and operations as the wallet one
    let wallet_inspection = party
        .wallet
        .inspect_fascia(
            res.psbt.clone(),
            details.fascia_path.clone(),
            Some(details.entropy),
            MAX_FASCIA_SIZE,
            MAX_FASCIA_BUNDLES,
        )
        .unwrap();
    let inspection = inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    )
    .unwrap();
    // the fascia carries the entropy its MPC commitment was built with, so giving it is optional
    let inspection_no_entropy = inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        None,
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&inspection_no_entropy).unwrap(),
        serde_json::to_value(&inspection).unwrap()
    );
    assert_eq!(inspection.txid, wallet_inspection.txid);
    assert_eq!(inspection.close_method, wallet_inspection.close_method);
    assert_eq!(inspection.commitment_hex, wallet_inspection.commitment_hex);
    assert_eq!(inspection.operations.len(), 1);
    let operation = &inspection.operations[0];
    let wallet_operation = &wallet_inspection.operations[0];
    assert_eq!(operation.asset_id, asset.asset_id);
    assert_eq!(
        operation.committed_transition_count,
        wallet_operation.committed_transition_count
    );
    assert_eq!(
        operation.committed_input_count,
        wallet_operation.committed_input_count
    );
    assert_eq!(operation.transitions.len(), 1);
    let transition = &operation.transitions[0];
    let wallet_transition = &wallet_operation.transitions[0];
    assert_eq!(transition.r#type, TypeOfTransition::Transfer);
    assert_eq!(transition.opid, wallet_transition.opid);
    assert_eq!(transition.globals, wallet_transition.globals);
    assert_eq!(transition.metadata, wallet_transition.metadata);

    // the inputs report the opouts the bundle commits to, but not the allocations they spend
    assert!(!transition.inputs.is_empty());
    let opouts: Vec<_> = transition.inputs.iter().map(|i| &i.opout).collect();
    let wallet_opouts: Vec<_> = wallet_transition.inputs.iter().map(|i| &i.opout).collect();
    assert_eq!(opouts, wallet_opouts);
    assert!(
        transition
            .inputs
            .iter()
            .all(|i| i.vin.is_none() && i.assignment.is_none())
    );
    assert!(
        wallet_transition
            .inputs
            .iter()
            .all(|i| i.vin.is_some() && i.assignment.is_some())
    );

    // the outputs are the same ones, without telling which are the sender's
    let outputs = |transition: &RgbTransitionInfo| -> Vec<_> {
        transition
            .outputs
            .iter()
            .map(|o| {
                (
                    o.txid.clone(),
                    o.vout,
                    o.blinding,
                    o.assignment.clone(),
                    o.is_concealed,
                    o.recipient_id.clone(),
                    o.is_terminal,
                )
            })
            .collect()
    };
    assert_eq!(transition.outputs.len(), 3);
    assert!(transition.outputs.iter().all(|o| o.is_ours.is_none()));
    assert!(
        wallet_transition
            .outputs
            .iter()
            .all(|o| o.is_ours.is_some())
    );
    assert_eq!(outputs(transition), outputs(wallet_transition));

    // the blinded receive is reported by its recipient ID, built for the given network
    let concealed: Vec<_> = transition
        .outputs
        .iter()
        .filter(|o| o.is_concealed)
        .collect();
    assert_eq!(concealed.len(), 1);
    assert_eq!(
        concealed[0].recipient_id.as_deref(),
        Some(receive_data_blind.recipient_id.as_str())
    );
    assert_eq!(concealed[0].assignment, Assignment::Fungible(amount_blind));
    let testnet_inspection = inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Testnet,
    )
    .unwrap();
    let testnet_recipient_ids: Vec<_> = testnet_inspection.operations[0].transitions[0]
        .outputs
        .iter()
        .filter_map(|o| o.recipient_id.clone())
        .collect();
    assert_eq!(testnet_recipient_ids.len(), 1);
    assert_ne!(
        testnet_recipient_ids[0],
        receive_data_blind.recipient_id.clone()
    );

    // a given entropy has to be the one the fascia carries
    let result = inspect_fascia(
        res.psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy.wrapping_add(1)),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d })
        if d.contains(&format!("built with entropy {}, not the given", details.entropy)));

    // the PSBT has to be the one of the fascia witness transaction
    let mut other_psbt = Psbt::from_str(&res.psbt).unwrap();
    let lock_time = other_psbt.unsigned_tx.lock_time.to_consensus_u32();
    other_psbt.unsigned_tx.lock_time = LockTime::from_consensus(lock_time.wrapping_add(1));
    let other_psbt = other_psbt.to_string();
    let result = inspect_fascia(
        other_psbt.clone(),
        details.fascia_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("does not match PSBT TXID"));
    let result = party.wallet.inspect_fascia(
        other_psbt,
        details.fascia_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("does not match PSBT TXID"));

    // the PSBT and the fascia file have to be valid
    let result = inspect_fascia(
        s!("invalid"),
        details.fascia_path.clone(),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Err(Error::InvalidPsbt { details: _ }));
    let result = inspect_fascia(
        res.psbt,
        s!("inexistent"),
        Some(details.entropy),
        MAX_FASCIA_SIZE,
        MAX_FASCIA_BUNDLES,
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == "inexistent");
}

/// A send of inflation rights moves them with a transition of their own, a transfer of inflation
/// rights, which the inspection reports.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn inflation_rights() {
    initialize();

    let amount = 100;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_ifa(None, None, None);
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::InflationRight(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let res = party.send_begin_result(&recipient_map).unwrap();
    let inspection = party
        .wallet
        .inspect_fascia(
            res.psbt,
            res.details.fascia_path,
            Some(res.details.entropy),
            MAX_FASCIA_SIZE,
            MAX_FASCIA_BUNDLES,
        )
        .unwrap();
    assert_eq!(inspection.operations.len(), 1);
    let transitions = &inspection.operations[0].transitions;
    assert_eq!(transitions.len(), 1);
    let transition = &transitions[0];
    assert_eq!(transition.r#type, TypeOfTransition::TransferInflation);
    let inputs: Vec<_> = transition.inputs.iter().map(input_assignment).collect();
    assert_eq!(inputs, vec![&Assignment::InflationRight(AMOUNT_INFLATION)]);
    let mut outputs: Vec<_> = transition
        .outputs
        .iter()
        .map(|o| (o.assignment.clone(), o.is_ours))
        .collect();
    outputs.sort();
    assert_eq!(
        outputs,
        vec![
            (Assignment::InflationRight(amount), Some(false)),
            (
                Assignment::InflationRight(AMOUNT_INFLATION - amount),
                Some(true)
            ),
        ]
    );
}

/// A send of the link right moves it with a transition of its own, a transfer of link rights, which
/// the inspection reports.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn link_right() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = issue_asset_ifa_link(&mut party, None, None);
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::LinkRight,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let res = party.send_begin_result(&recipient_map).unwrap();
    let inspection = party
        .wallet
        .inspect_fascia(
            res.psbt,
            res.details.fascia_path,
            Some(res.details.entropy),
            MAX_FASCIA_SIZE,
            MAX_FASCIA_BUNDLES,
        )
        .unwrap();
    assert_eq!(inspection.operations.len(), 1);
    let transitions = &inspection.operations[0].transitions;
    assert_eq!(transitions.len(), 1);
    let transition = &transitions[0];
    assert_eq!(transition.r#type, TypeOfTransition::TransferLink);
    let inputs: Vec<_> = transition.inputs.iter().map(input_assignment).collect();
    assert_eq!(inputs, vec![&Assignment::LinkRight]);
    let outputs: Vec<_> = transition
        .outputs
        .iter()
        .map(|o| (o.assignment.clone(), o.is_ours))
        .collect();
    assert_eq!(outputs, vec![(Assignment::LinkRight, Some(false))]);
}
