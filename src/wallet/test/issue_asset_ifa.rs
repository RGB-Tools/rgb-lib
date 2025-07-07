use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // create more UTXOs to host all issued allocations
    let num_created = test_create_utxos(&mut wallet, &online, true, Some(9), None, FEE_RATE);
    assert_eq!(num_created, 4);

    let before_timestamp = now().unix_timestamp();
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let asset = test_issue_asset_ifa(
        &mut wallet,
        &online,
        Some(&[AMOUNT, AMOUNT]),
        Some(&[AMOUNT, AMOUNT, AMOUNT]),
        4,
    );
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // check the asset has been saved with the correct schema
    let ifa_asset_list = test_list_assets(&wallet, &[AssetSchema::Ifa]).ifa.unwrap();
    assert!(
        ifa_asset_list
            .into_iter()
            .any(|a| a.asset_id == asset.asset_id)
    );

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _receive_data = test_blind_receive(&wallet);
    show_unspent_colorings(&mut wallet, "after issuance + blind receive");

    // checks
    let balance = test_get_asset_balance(&wallet, &asset.asset_id);
    assert_eq!(asset.ticker, TICKER.to_string());
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.details, None);
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(asset.issued_supply, AMOUNT * 2);
    assert_eq!(
        balance,
        Balance {
            settled: AMOUNT * 2,
            future: AMOUNT * 2,
            spendable: AMOUNT,
        }
    );
    assert!(before_timestamp <= asset.added_at && asset.added_at <= now().unix_timestamp());
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspents_fungible = unspents.iter().filter(|u| {
        u.rgb_allocations.iter().any(|a| {
            a.asset_id == Some(asset.asset_id.clone())
                && a.assignment == Assignment::Fungible(AMOUNT)
        })
    });
    assert_eq!(unspents_fungible.count(), 2);
    let unspents_inflation = unspents.iter().filter(|u| {
        u.rgb_allocations.iter().any(|a| {
            a.asset_id == Some(asset.asset_id.clone())
                && a.assignment == Assignment::InflationRight(AMOUNT)
        })
    });
    assert_eq!(unspents_inflation.count(), 3);
    let unspents_replace = unspents.iter().filter(|u| {
        u.rgb_allocations.iter().any(|a| {
            a.asset_id == Some(asset.asset_id.clone()) && a.assignment == Assignment::ReplaceRight
        })
    });
    assert_eq!(unspents_replace.count(), 4);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn multi_success() {
    initialize();

    let amounts: Vec<u64> = vec![111, 222, 333, 444, 555];
    let sum: u64 = amounts.iter().sum();

    let (mut wallet, online) = get_funded_wallet!();

    // create more UTXOs
    let _ = test_create_utxos_default(&mut wallet, &online);

    let asset = test_issue_asset_ifa(&mut wallet, &online, Some(&amounts), None, 0);

    // check balance is the sum of the amounts
    assert_eq!(asset.balance.settled, sum);

    // check each allocation ends up on a different UTXO
    let unspents: Vec<Unspent> = test_list_unspents(&mut wallet, None, true)
        .into_iter()
        .filter(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| matches!(a.assignment, Assignment::Fungible(_)))
        })
        .collect();
    let mut outpoints: Vec<Outpoint> = vec![];
    for unspent in &unspents {
        let outpoint = unspent.utxo.outpoint.clone();
        assert!(!outpoints.contains(&outpoint));
        outpoints.push(outpoint);
    }
    assert_eq!(outpoints.len(), amounts.len());

    // check all allocations are of the same asset
    assert!(
        unspents
            .iter()
            .filter(|u| !u.rgb_allocations.is_empty())
            .all(|u| {
                u.rgb_allocations.len() == 1
                    && u.rgb_allocations.first().unwrap().asset_id == Some(asset.asset_id.clone())
            })
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn no_issue_on_pending_send() {
    initialize();

    let amount: u64 = 66;
    let amount_inflation: u64 = 190;

    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // prepare UTXOs
    let num_created = test_create_utxos(&mut wallet, &online, true, Some(3), Some(5000), FEE_RATE);
    assert_eq!(num_created, 3);

    // issue 1st asset
    let asset_1 = test_issue_asset_ifa(&mut wallet, &online, None, None, 1);
    show_unspent_colorings(&mut wallet, "after 1st issuance");
    // get 1st issuance UTXOs
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspent_1_fungible = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations.iter().any(|a| {
                a.asset_id == Some(asset_1.asset_id.clone())
                    && a.assignment == Assignment::Fungible(AMOUNT)
            })
        })
        .unwrap();
    let unspent_1_inflation = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations.iter().any(|a| {
                a.asset_id == Some(asset_1.asset_id.clone())
                    && a.assignment == Assignment::InflationRight(AMOUNT_INFLATION)
            })
        })
        .unwrap();
    let unspent_1_replace = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations.iter().any(|a| {
                a.asset_id == Some(asset_1.asset_id.clone())
                    && a.assignment == Assignment::ReplaceRight
            })
        })
        .unwrap();
    assert_ne!(
        unspent_1_fungible.utxo.outpoint,
        unspent_1_inflation.utxo.outpoint
    );
    assert_ne!(
        unspent_1_fungible.utxo.outpoint,
        unspent_1_replace.utxo.outpoint
    );

    // send 1st asset
    let receive_data_1 = test_witness_receive(&mut rcv_wallet);
    let receive_data_2 = test_witness_receive(&mut rcv_wallet);
    let receive_data_3 = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::InflationRight(amount_inflation),
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::ReplaceRight,
                recipient_id: receive_data_3.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    show_unspent_colorings(&mut wallet, "after send");

    // issuing a 2nd asset fails due to insufficient allocation slots (pending transfers)
    let result =
        test_issue_asset_ifa_result(&mut wallet, &online, Some(&[AMOUNT * 2]), Some(&[]), 0);
    assert_matches!(result, Err(Error::InsufficientAllocationSlots));

    // create 1 more UTXO + issue 2nd asset
    let num_created = test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    assert_eq!(num_created, 1);
    let asset_2 = test_issue_asset_ifa(&mut wallet, &online, Some(&[AMOUNT * 2]), Some(&[]), 0);
    show_unspent_colorings(&mut wallet, "after 2nd issuance");
    // get 2nd issuance UTXO
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspent_2_fungible = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations.iter().any(|a| {
                a.asset_id == Some(asset_2.asset_id.clone())
                    && a.assignment == Assignment::Fungible(AMOUNT * 2)
            })
        })
        .unwrap();
    // check 2nd issuance was not allocated to the same UTXO as the 1st one (now being spent)
    assert_ne!(
        unspent_2_fungible.utxo.outpoint,
        unspent_1_fungible.utxo.outpoint
    );

    // progress transfer to WaitingConfirmations
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset_1.asset_id), None);

    // issue 3rd asset
    let asset_3 = test_issue_asset_ifa(&mut wallet, &online, Some(&[AMOUNT * 3]), Some(&[]), 0);
    show_unspent_colorings(&mut wallet, "after 3rd issuance");
    // get 3rd issuance UTXO
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspent_3_fungible = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations.iter().any(|a| {
                a.asset_id == Some(asset_3.asset_id.clone())
                    && a.assignment == Assignment::Fungible(AMOUNT * 3)
            })
        })
        .unwrap();
    // check 3rd issuance was not allocated to the same UTXO as the 1st one (now being spent)
    assert_ne!(
        unspent_3_fungible.utxo.outpoint,
        unspent_1_fungible.utxo.outpoint
    );
    assert_eq!(
        unspent_3_fungible.utxo.outpoint,
        unspent_2_fungible.utxo.outpoint
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallet
    let (mut wallet, online) = get_funded_wallet!();

    // supply overflow
    let result =
        test_issue_asset_ifa_result(&mut wallet, &online, Some(&[u64::MAX, u64::MAX]), None, 0);
    assert!(matches!(result, Err(Error::TooHighIssuanceAmounts)));

    // invalid ticker: empty
    let result =
        wallet.issue_asset_ifa(s!(""), NAME.to_string(), PRECISION, vec![AMOUNT], vec![], 0);
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == EMPTY_MSG));

    // invalid ticker: too long
    let result = wallet.issue_asset_ifa(
        s!("ABCDEFGHI"),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
        vec![],
        0,
    );
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid ticker: lowercase
    let result = wallet.issue_asset_ifa(
        s!("TiCkEr"),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
        vec![],
        0,
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == "ticker needs to be all uppercase")
    );

    // invalid ticker: with space
    let invalid_ticker = "TICKER WITH SPACE";
    let result = wallet.issue_asset_ifa(
        invalid_ticker.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
        vec![],
        0,
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_ticker).replace("{1}", " ").replace("{2}", "6"))
    );

    // invalid ticker: unicode characters
    let invalid_ticker = "TICKERWITH℧NICODE";
    let result = wallet.issue_asset_ifa(
        invalid_ticker.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
        vec![],
        0,
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_ticker).replace("{1}", "℧").replace("{2}", "10"))
    );

    // invalid name: empty
    let result = wallet.issue_asset_ifa(
        TICKER.to_string(),
        s!(""),
        PRECISION,
        vec![AMOUNT],
        vec![],
        0,
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == EMPTY_MSG));

    // invalid name: too long
    let result = wallet.issue_asset_ifa(
        TICKER.to_string(),
        ("a").repeat(257),
        PRECISION,
        vec![AMOUNT],
        vec![],
        0,
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid name: unicode characters
    let invalid_name = "name with ℧nicode characters";
    let result = wallet.issue_asset_ifa(
        TICKER.to_string(),
        invalid_name.to_string(),
        PRECISION,
        vec![AMOUNT],
        vec![],
        0,
    );
    assert!(
        matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_name).replace("{1}", "℧").replace("{2}", "10"))
    );

    // invalid precision
    let result = wallet.issue_asset_ifa(
        TICKER.to_string(),
        NAME.to_string(),
        19,
        vec![AMOUNT],
        vec![],
        0,
    );
    assert!(matches!(
        result,
        Err(Error::InvalidPrecision { details: m }) if m == "precision is too high"
    ));

    // invalid amount list
    let result = test_issue_asset_ifa_result(&mut wallet, &online, Some(&[]), None, 0);
    assert!(matches!(result, Err(Error::NoIssuanceAmounts)));

    // new wallet
    let (mut wallet, online) = get_empty_wallet!();

    // insufficient funds
    let result = test_issue_asset_ifa_result(&mut wallet, &online, None, None, 0);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    fund_wallet(test_get_address(&mut wallet));
    mine(false, false);

    // insufficient allocations
    let result = test_issue_asset_ifa_result(&mut wallet, &online, None, None, 0);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}
