use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let file_str = "README.md";
    let image_str = "tests/qrcode.png";

    let (mut wallet, online) = get_funded_wallet!();

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _receive_data = wallet.blind_receive(
        None,
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );

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
    let empty_data_paths = vec![];
    assert_eq!(asset_1.data_paths, empty_data_paths);
    assert!(before_timestamp <= asset_1.added_at && asset_1.added_at <= now().unix_timestamp());

    // include a text file
    println!("\nasset 2");
    let asset_2 = wallet
        .issue_asset_cfa(
            online.clone(),
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            Some(file_str.to_string()),
        )
        .unwrap();
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
    assert_eq!(asset_2.data_paths.len(), 1);
    // check attached file contents match
    let media = asset_2.data_paths.first().unwrap();
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
    let asset_3 = wallet
        .issue_asset_cfa(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 3],
            Some(image_str.to_string()),
        )
        .unwrap();
    show_unspent_colorings(&wallet, "after issuance 3");
    assert_eq!(
        asset_3.balance,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3,
            spendable: AMOUNT * 3,
        }
    );
    assert_eq!(asset_3.data_paths.len(), 1);
    // check attached file contents match
    let media = asset_3.data_paths.first().unwrap();
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

    let asset = wallet
        .issue_asset_cfa(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            amounts.clone(),
            Some(file_str.to_string()),
        )
        .unwrap();

    // check balance is the sum of the amounts
    assert_eq!(asset.balance.settled, sum);

    // check each allocation ends up on a different UTXO
    let unspents: Vec<Unspent> = wallet
        .list_unspents(None, true)
        .unwrap()
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
    let cfa_asset_list = wallet.list_assets(vec![]).unwrap().cfa.unwrap();
    let cfa_asset = cfa_asset_list
        .into_iter()
        .find(|a| a.asset_id == asset.asset_id)
        .unwrap();
    assert_eq!(cfa_asset.data_paths.len(), 1);
}

#[test]
#[parallel]
fn no_issue_on_pending_send() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue 1st asset
    let asset_1 = wallet
        .issue_asset_cfa(
            online.clone(),
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT],
            None,
        )
        .unwrap();
    // get 1st issuance UTXO
    let unspents = wallet.list_unspents(None, false).unwrap();
    let unspent_1 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        })
        .unwrap();
    // send 1st asset
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());

    // issue 2nd asset
    let asset_2 = wallet
        .issue_asset_cfa(
            online.clone(),
            s!("NAME2"),
            Some(s!("DESCRIPTION2")),
            PRECISION,
            vec![AMOUNT * 2],
            None,
        )
        .unwrap();
    show_unspent_colorings(&wallet, "after 2nd issuance");
    // get 2nd issuance UTXO
    let unspents = wallet.list_unspents(None, false).unwrap();
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
    wallet
        .refresh(online.clone(), Some(asset_1.asset_id.clone()), vec![])
        .unwrap();
    // issue 3rd asset
    let asset_3 = wallet
        .issue_asset_cfa(
            online,
            s!("NAME3"),
            Some(s!("DESCRIPTION3")),
            PRECISION,
            vec![AMOUNT * 3],
            None,
        )
        .unwrap();
    show_unspent_colorings(&wallet, "after 3rd issuance");
    // get 3rd issuance UTXO
    let unspents = wallet.list_unspents(None, false).unwrap();
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
    let result = wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![u64::MAX, u64::MAX],
        None,
    );
    assert!(matches!(result, Err(Error::TooHighIssuanceAmounts)));

    // bad online object
    let other_online = Online {
        id: 1,
        electrum_url: wallet.online_data.as_ref().unwrap().electrum_url.clone(),
    };
    let result = wallet.issue_asset_cfa(
        other_online,
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
    );
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
    let result = wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        19,
        vec![],
        None,
    );
    assert!(matches!(result, Err(Error::NoIssuanceAmounts)));

    // invalid file_path
    let invalid_file_path = s!("invalid");
    let result = wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        Some(invalid_file_path.clone()),
    );
    assert!(matches!(
        result,
        Err(Error::InvalidFilePath { file_path: t }) if t == invalid_file_path
    ));

    drain_wallet(&wallet, online.clone());

    // insufficient funds
    let result = wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
    );
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    fund_wallet(wallet.get_address().unwrap());
    mine(false);

    // insufficient allocations
    let result = wallet.issue_asset_cfa(
        online,
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
    );
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}
