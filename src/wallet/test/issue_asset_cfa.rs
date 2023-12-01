use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let file_str = "README.md";
    let image_str = ["tests", "qrcode.png"].join(&MAIN_SEPARATOR.to_string());

    let (mut wallet, online) = get_funded_wallet!();

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _receive_data = test_blind_receive(&mut wallet);

    // required fields only
    println!("\nasset 1");
    let before_timestamp = now().unix_timestamp();
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let asset_1 = wallet
        .issue_asset_cfa(
            online.clone(),
            NAME.to_string(),
            None,
            PRECISION,
            vec![AMOUNT, AMOUNT],
            None,
        )
        .unwrap();
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    show_unspent_colorings(&wallet, "after issuance 1");
    assert_eq!(asset_1.name, NAME.to_string());
    assert_eq!(asset_1.description, None);
    assert_eq!(asset_1.precision, PRECISION);
    assert_eq!(
        asset_1.balance,
        Balance {
            settled: AMOUNT * 2,
            future: AMOUNT * 2,
            spendable: AMOUNT * 2,
        }
    );
    assert_eq!(asset_1.media, None);
    assert!(before_timestamp <= asset_1.added_at && asset_1.added_at <= now().unix_timestamp());

    // include a text file
    println!("\nasset 2");
    let asset_2 = test_issue_asset_cfa(
        &mut wallet,
        &online,
        Some(&[AMOUNT * 2]),
        Some(file_str.to_string()),
    );
    show_unspent_colorings(&wallet, "after issuance 2");
    assert_eq!(asset_2.name, NAME.to_string());
    assert_eq!(asset_2.description, Some(DESCRIPTION.to_string()));
    assert_eq!(asset_2.precision, PRECISION);
    assert_eq!(
        asset_2.balance,
        Balance {
            settled: AMOUNT * 2,
            future: AMOUNT * 2,
            spendable: AMOUNT * 2,
        }
    );
    assert!(asset_2.media.is_some());
    // check attached file contents match
    let media = asset_2.media.unwrap();
    assert_eq!(media.mime, "text/plain");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check attachment id for provided file matches
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_attachment_id = src_hash.to_string();
    let dst_attachment_id = Path::new(&dst_path)
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy();
    assert_eq!(src_attachment_id, dst_attachment_id);

    // include an image file
    println!("\nasset 3");
    let asset_3 = test_issue_asset_cfa(
        &mut wallet,
        &online,
        Some(&[AMOUNT * 3]),
        Some(image_str.to_string()),
    );
    show_unspent_colorings(&wallet, "after issuance 3");
    assert_eq!(
        asset_3.balance,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3,
            spendable: AMOUNT * 3,
        }
    );
    assert!(asset_3.media.is_some());
    // check attached file contents match
    let media = asset_3.media.unwrap();
    assert_eq!(media.mime, "image/png");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(image_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check attachment id for provided file matches
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_attachment_id = src_hash.to_string();
    let dst_attachment_id = Path::new(&dst_path)
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy();
    assert_eq!(src_attachment_id, dst_attachment_id);
}

#[test]
#[parallel]
fn multi_success() {
    initialize();

    let amounts: Vec<u64> = vec![111, 222, 333, 444, 555];
    let sum: u64 = amounts.iter().sum();
    let file_str = "README.md";

    let (mut wallet, online) = get_funded_wallet!();

    let asset = test_issue_asset_cfa(
        &mut wallet,
        &online,
        Some(&amounts),
        Some(file_str.to_string()),
    );

    // check balance is the sum of the amounts
    assert_eq!(asset.balance.settled, sum);

    // check each allocation ends up on a different UTXO
    let unspents: Vec<Unspent> = test_list_unspents(&wallet, None, true)
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

    // check the allocated asset has one attachment
    let cfa_asset_list = test_list_assets(&mut wallet, &[]).cfa.unwrap();
    let cfa_asset = cfa_asset_list
        .into_iter()
        .find(|a| a.asset_id == asset.asset_id)
        .unwrap();
    assert!(cfa_asset.media.is_some());
}

#[test]
#[parallel]
fn no_issue_on_pending_send() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue 1st asset
    let asset_1 = test_issue_asset_cfa(&mut wallet, &online, None, None);
    // get 1st issuance UTXO
    let unspents = test_list_unspents(&wallet, None, false);
    let unspent_1 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        })
        .unwrap();
    // send 1st asset
    let receive_data = test_blind_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // issue 2nd asset
    let asset_2 = test_issue_asset_cfa(&mut wallet, &online, Some(&[AMOUNT * 2]), None);
    show_unspent_colorings(&wallet, "after 2nd issuance");
    // get 2nd issuance UTXO
    let unspents = test_list_unspents(&wallet, None, false);
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
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    test_refresh_asset(&mut wallet, &online, &asset_1.asset_id);
    // issue 3rd asset
    let asset_3 = test_issue_asset_cfa(&mut wallet, &online, Some(&[AMOUNT * 3]), None);
    show_unspent_colorings(&wallet, "after 3rd issuance");
    // get 3rd issuance UTXO
    let unspents = test_list_unspents(&wallet, None, false);
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

#[test]
#[parallel]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // supply overflow
    let result =
        test_issue_asset_cfa_result(&mut wallet, &online, Some(&[u64::MAX, u64::MAX]), None);
    assert!(matches!(result, Err(Error::TooHighIssuanceAmounts)));

    // bad online object
    let other_online = Online {
        id: 1,
        electrum_url: wallet.online_data.as_ref().unwrap().electrum_url.clone(),
    };
    let result = test_issue_asset_cfa_result(&mut wallet, &other_online, None, None);
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // invalid name: empty
    let result = wallet.issue_asset_cfa(
        online.clone(),
        s!(""),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_EMPTY_MSG));

    // invalid name: too long
    let result = wallet.issue_asset_cfa(
        online.clone(),
        ("a").repeat(257),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid name: unicode characters
    let result = wallet.issue_asset_cfa(
        online.clone(),
        s!("name with ℧nicode characters"),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_NOT_ASCII_MSG));

    // invalid description: empty
    let result = wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(s!("")),
        PRECISION,
        vec![AMOUNT],
        None,
    );
    assert!(
        matches!(result, Err(Error::InvalidDescription { details: m }) if m == IDENT_EMPTY_MSG)
    );

    // invalid description: too long
    let result = wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(("a").repeat(256)),
        PRECISION,
        vec![AMOUNT],
        None,
    );
    assert!(
        matches!(result, Err(Error::InvalidDescription { details: m }) if m == IDENT_TOO_LONG_MSG)
    );

    // invalid precision
    let result = wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        19,
        vec![AMOUNT],
        None,
    );
    assert!(matches!(
        result,
        Err(Error::InvalidPrecision { details: m }) if m == "precision is too high"
    ));

    // invalid amount list
    let result = test_issue_asset_cfa_result(&mut wallet, &online, Some(&[]), None);
    assert!(matches!(result, Err(Error::NoIssuanceAmounts)));

    // invalid file_path
    let invalid_file_path = s!("invalid");
    let result =
        test_issue_asset_cfa_result(&mut wallet, &online, None, Some(invalid_file_path.clone()));
    assert!(matches!(
        result,
        Err(Error::InvalidFilePath { file_path: t }) if t == invalid_file_path
    ));

    drain_wallet(&wallet, &online);

    // insufficient funds
    let result = test_issue_asset_cfa_result(&mut wallet, &online, None, None);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    fund_wallet(test_get_address(&wallet));
    mine(false);

    // insufficient allocations
    let result = test_issue_asset_cfa_result(&mut wallet, &online, None, None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}
