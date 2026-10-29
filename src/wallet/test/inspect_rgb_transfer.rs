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
        .inspect_rgb_transfer(
            res.psbt.clone(),
            details.fascia_path.clone(),
            details.entropy,
        )
        .unwrap();

    // a different entropy gives a commitment the PSBT doesn't commit to
    let result = party.wallet.inspect_rgb_transfer(
        res.psbt.clone(),
        details.fascia_path.clone(),
        details.entropy.wrapping_add(1),
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("doesn't commit to"));

    // a transition not committed by its bundle: the bundle commitment still matches the PSBT one
    let fascia = read_fascia(Path::new(&details.fascia_path)).unwrap();
    let mut fascia_json = serde_json::to_value(&fascia).unwrap();
    let transition = fascia_json["bundles"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()["knownTransitions"][0]["transition"]
        .as_object_mut()
        .unwrap();
    let nonce = transition["nonce"].as_u64().unwrap();
    transition.insert(s!("nonce"), nonce.wrapping_add(1).into());
    let tampered_fascia: Fascia = serde_json::from_value(fascia_json).unwrap();
    let tampered_file = tempfile::NamedTempFile::with_prefix("inspect_rgb_transfer::").unwrap();
    write_fascia(tampered_file.path(), &tampered_fascia).unwrap();
    let result = party.wallet.inspect_rgb_transfer(
        res.psbt,
        tampered_file.path().to_string_lossy().to_string(),
        details.entropy,
    );
    assert_matches!(result, Err(Error::RgbInspection { details: d }) if d.contains("is not committed"));
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
        .inspect_rgb_transfer(res.psbt, res.details.fascia_path, res.details.entropy)
        .unwrap();
    assert_eq!(inspection.operations.len(), 1);
    let transitions = &inspection.operations[0].transitions;
    assert_eq!(transitions.len(), 1);
    let transition = &transitions[0];
    assert_eq!(transition.r#type, TypeOfTransition::TransferInflation);
    let inputs: Vec<_> = transition.inputs.iter().map(|i| &i.assignment).collect();
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
        .inspect_rgb_transfer(res.psbt, res.details.fascia_path, res.details.entropy)
        .unwrap();
    assert_eq!(inspection.operations.len(), 1);
    let transitions = &inspection.operations[0].transitions;
    assert_eq!(transitions.len(), 1);
    let transition = &transitions[0];
    assert_eq!(transition.r#type, TypeOfTransition::TransferLink);
    let inputs: Vec<_> = transition.inputs.iter().map(|i| &i.assignment).collect();
    assert_eq!(inputs, vec![&Assignment::LinkRight]);
    let outputs: Vec<_> = transition
        .outputs
        .iter()
        .map(|o| (o.assignment.clone(), o.is_ours))
        .collect();
    assert_eq!(outputs, vec![(Assignment::LinkRight, Some(false))]);
}
