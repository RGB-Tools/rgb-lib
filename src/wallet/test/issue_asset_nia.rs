use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _receive_data = test_blind_receive(&wallet);

    let before_timestamp = now().unix_timestamp();
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let asset = test_issue_asset_nia(&mut wallet, &online, Some(&[AMOUNT, AMOUNT]));
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    show_unspent_colorings(&mut wallet, "after issuance");
    assert_eq!(asset.ticker, TICKER.to_string());
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.details, None);
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(asset.issued_supply, AMOUNT * 2);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT * 2,
            future: AMOUNT * 2,
            spendable: AMOUNT * 2,
        }
    );
    assert!(before_timestamp <= asset.added_at && asset.added_at <= now().unix_timestamp());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn multi_success() {
    initialize();

    let amounts: Vec<u64> = vec![111, 222, 333, 444, 555];
    let sum: u64 = amounts.iter().sum();

    let (mut wallet, online) = get_funded_wallet!();

    let asset = test_issue_asset_nia(&mut wallet, &online, Some(&amounts));

    // check balance is the sum of the amounts
    assert_eq!(asset.balance.settled, sum);

    // check each allocation ends up on a different UTXO
    let unspents: Vec<Unspent> = test_list_unspents(&mut wallet, None, true)
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .collect();
    let mut outpoints: Vec<Outpoint> = vec![];
    for unspent in &unspents {
        let outpoint = unspent.utxo.outpoint.clone();
        assert!(!outpoints.contains(&outpoint));
        outpoints.push(outpoint);
    }
    assert_eq!(outpoints.len(), amounts.len());

    // check all allocations are of the same asset
    assert!(unspents
        .iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .all(|u| {
            u.rgb_allocations.len() == 1
                && u.rgb_allocations.first().unwrap().asset_id == Some(asset.asset_id.clone())
        }));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn no_issue_on_pending_send() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // prepare UTXO
    let num_created = test_create_utxos(&mut wallet, &online, true, Some(1), Some(5000), FEE_RATE);
    assert_eq!(num_created, 1);

    // issue 1st asset
    let asset_1 = test_issue_asset_nia(&mut wallet, &online, None);
    // get 1st issuance UTXO
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspent_1 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        })
        .unwrap();
    // send 1st asset
    let receive_data = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // issuing a 2nd asset fails due to missing free allocation slot
    let result = test_issue_asset_nia_result(&mut wallet, &online, Some(&[AMOUNT * 2]));
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // create 1 more UTXO issue 2nd asset
    let num_created = test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    assert_eq!(num_created, 1);
    let asset_2 = test_issue_asset_nia(&mut wallet, &online, Some(&[AMOUNT * 2]));
    show_unspent_colorings(&mut wallet, "after 2nd issuance");
    // get 2nd issuance UTXO
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspent_2 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_2.asset_id.clone()))
        })
        .unwrap();
    // check 2nd issuance was not allocated to the same UTXO as the 1st one (now being spent)
    assert_ne!(unspent_1.utxo.outpoint, unspent_2.utxo.outpoint);

    // progress transfer to WaitingConfirmations
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset_1.asset_id), None);

    // issue 3rd asset
    let asset_3 = test_issue_asset_nia(&mut wallet, &online, Some(&[AMOUNT * 3]));
    show_unspent_colorings(&mut wallet, "after 3rd issuance");
    // get 3rd issuance UTXO
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspent_3 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_3.asset_id.clone()))
        })
        .unwrap();
    // check 3rd issuance was not allocated to the same UTXO as the 1st one (now being spent)
    assert_ne!(unspent_1.utxo.outpoint, unspent_3.utxo.outpoint);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallet
    let (mut wallet, online) = get_funded_wallet!();

    // supply overflow
    let result = test_issue_asset_nia_result(&mut wallet, &online, Some(&[u64::MAX, u64::MAX]));
    assert!(matches!(result, Err(Error::TooHighIssuanceAmounts)));

    // bad online object
    let other_online = Online {
        id: 1,
        indexer_url: wallet.online_data.as_ref().unwrap().indexer_url.clone(),
    };
    let result = wallet.issue_asset_nia(
        other_online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // invalid ticker: empty
    let result = wallet.issue_asset_nia(
        online.clone(),
        s!(""),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == EMPTY_MSG));

    // invalid ticker: too long
    let result = wallet.issue_asset_nia(
        online.clone(),
        s!("ABCDEFGHI"),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid ticker: lowercase
    let result = wallet.issue_asset_nia(
        online.clone(),
        s!("TiCkEr"),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == "ticker needs to be all uppercase")
    );

    // invalid ticker: with space
    let invalid_ticker = "TICKER WITH SPACE";
    let result = wallet.issue_asset_nia(
        online.clone(),
        invalid_ticker.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_ticker).replace("{1}", " ").replace("{2}", "6"))
    );

    // invalid ticker: unicode characters
    let invalid_ticker = "TICKERWITH℧NICODE";
    let result = wallet.issue_asset_nia(
        online.clone(),
        invalid_ticker.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_ticker).replace("{1}", "℧").replace("{2}", "10"))
    );

    // invalid name: empty
    let result = wallet.issue_asset_nia(
        online.clone(),
        TICKER.to_string(),
        s!(""),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == EMPTY_MSG));

    // invalid name: too long
    let result = wallet.issue_asset_nia(
        online.clone(),
        TICKER.to_string(),
        ("a").repeat(257),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid name: unicode characters
    let invalid_name = "name with ℧nicode characters";
    let result = wallet.issue_asset_nia(
        online.clone(),
        TICKER.to_string(),
        invalid_name.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(
        matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_name).replace("{1}", "℧").replace("{2}", "10"))
    );

    // invalid precision
    let result = wallet.issue_asset_nia(
        online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        19,
        vec![AMOUNT],
    );
    assert!(matches!(
        result,
        Err(Error::InvalidPrecision { details: m }) if m == "precision is too high"
    ));

    // invalid amount list
    let result = test_issue_asset_nia_result(&mut wallet, &online, Some(&[]));
    assert!(matches!(result, Err(Error::NoIssuanceAmounts)));

    // new wallet
    let (mut wallet, online) = get_empty_wallet!();

    // insufficient funds
    let result = test_issue_asset_nia_result(&mut wallet, &online, None);
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
    let result = test_issue_asset_nia_result(&mut wallet, &online, None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}
