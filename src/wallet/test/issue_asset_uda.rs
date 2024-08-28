use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let settled = 1;
    let file_str = "README.md";
    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);

    let (mut wallet, online) = get_funded_wallet!();

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _receive_data = test_blind_receive(&wallet);

    // required fields only
    println!("\nasset 1");
    let before_timestamp = now().unix_timestamp();
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let asset_1 = test_issue_asset_uda(&mut wallet, &online, None, None, vec![]);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    show_unspent_colorings(&mut wallet, "after issuance 1");
    assert_eq!(asset_1.ticker, TICKER.to_string());
    assert_eq!(asset_1.name, NAME.to_string());
    assert_eq!(asset_1.details, None);
    assert_eq!(asset_1.precision, PRECISION);
    assert_eq!(asset_1.issued_supply, settled);
    assert_eq!(
        asset_1.balance,
        Balance {
            settled,
            future: settled,
            spendable: settled,
        }
    );
    let token = asset_1.token.unwrap();
    assert_eq!(token.index, 0);
    assert_eq!(token.ticker, None);
    assert_eq!(token.name, None);
    assert_eq!(token.details, None);
    assert!(!token.embedded_media);
    assert_eq!(token.media, None);
    assert_eq!(token.attachments, HashMap::new());
    assert!(!token.reserves);
    assert!(before_timestamp <= asset_1.added_at && asset_1.added_at <= now().unix_timestamp());

    // include a token with a media and 2 attachments
    println!("\nasset 2");
    let asset_2 = test_issue_asset_uda(
        &mut wallet,
        &online,
        Some(DETAILS),
        Some(file_str),
        vec![&image_str, file_str],
    );
    show_unspent_colorings(&mut wallet, "after issuance 2");
    assert_eq!(asset_2.ticker, TICKER.to_string());
    assert_eq!(asset_2.name, NAME.to_string());
    assert_eq!(asset_2.details, Some(DETAILS.to_string()));
    assert_eq!(asset_2.precision, PRECISION);
    assert_eq!(
        asset_2.balance,
        Balance {
            settled,
            future: settled,
            spendable: settled,
        }
    );
    let token = asset_2.token.unwrap();
    assert!(token.media.is_some());
    assert!(!token.attachments.is_empty());
    // check media file contents match
    let media = token.media.unwrap();
    assert_eq!(media.mime, "text/plain");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check digest for provided file matches
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_digest = src_hash.to_string();
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
    // check attachments
    let media = token.attachments.get(&0).unwrap();
    assert_eq!(media.mime, "image/png");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(image_str.clone())).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_digest = src_hash.to_string();
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
    let media = token.attachments.get(&1).unwrap();
    assert_eq!(media.mime, "text/plain");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_digest = src_hash.to_string();
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);

    // maximum number of attachments
    println!("\nmatching max attachment number");
    let asset_3 = test_issue_asset_uda(
        &mut wallet,
        &online,
        None,
        None,
        [image_str.clone()]
            .iter()
            .cycle()
            .take(MAX_ATTACHMENTS)
            .map(|a| a.as_str())
            .collect(),
    );
    show_unspent_colorings(&mut wallet, "after issuance 3");
    assert_eq!(
        asset_3.balance,
        Balance {
            settled,
            future: settled,
            spendable: settled,
        }
    );
    let token = asset_3.token.unwrap();
    assert!(!token.embedded_media);
    assert_eq!(token.media, None);
    let attachments_bytes = std::fs::read(PathBuf::from(image_str.clone())).unwrap();
    let attachment_hash: sha256::Hash = Sha256Hash::hash(&attachments_bytes[..]);
    let attachment_digest = attachment_hash.to_string();
    let attachment_path = wallet
        .get_wallet_dir()
        .join(MEDIA_DIR)
        .join(&attachment_digest);
    let mut token_attachments = HashMap::new();
    for i in 0..MAX_ATTACHMENTS {
        token_attachments.insert(
            i as u8,
            Media {
                digest: attachment_digest.clone(),
                file_path: attachment_path.to_str().unwrap().to_string(),
                mime: s!("image/png"),
            },
        );
    }
    assert_eq!(token.attachments, token_attachments);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn no_issue_on_pending_send() {
    initialize();

    let amount: u64 = 1;

    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // prepare UTXO
    let num_created = test_create_utxos(&mut wallet, &online, true, Some(1), Some(5000), FEE_RATE);
    assert_eq!(num_created, 1);

    // issue 1st asset
    let asset_1 = test_issue_asset_uda(&mut wallet, &online, None, None, vec![]);
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
    let result = test_issue_asset_uda_result(&mut wallet, &online, None, None, vec![]);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // create 1 more UTXO issue 2nd asset
    let num_created = test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    assert_eq!(num_created, 1);
    let asset_2 = test_issue_asset_uda(&mut wallet, &online, None, None, vec![]);
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
    let asset_3 = test_issue_asset_uda(&mut wallet, &online, None, None, vec![]);
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

    let attachment_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let empty_str = ["tests", "empty"].join(MAIN_SEPARATOR_STR);
    let missing_str = "missing";

    // wallet
    let (mut wallet, online) = get_funded_wallet!();

    // bad online object
    let other_online = Online {
        id: 1,
        indexer_url: wallet.online_data.as_ref().unwrap().indexer_url.clone(),
    };
    let result = wallet.issue_asset_uda(
        other_online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // invalid ticker: empty
    let result = wallet.issue_asset_uda(
        online.clone(),
        s!(""),
        NAME.to_string(),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == EMPTY_MSG));

    // invalid ticker: too long
    let result = wallet.issue_asset_uda(
        online.clone(),
        s!("ABCDEFGHI"),
        NAME.to_string(),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid ticker: lowercase
    let result = wallet.issue_asset_uda(
        online.clone(),
        s!("TiCkEr"),
        NAME.to_string(),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == "ticker needs to be all uppercase")
    );

    // invalid ticker: unicode characters
    let invalid_ticker = "TICKER WITH ℧NICODE CHARACTERS";
    let result = wallet.issue_asset_uda(
        online.clone(),
        invalid_ticker.to_string(),
        NAME.to_string(),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_ticker).replace("{1}", " ").replace("{2}", "6"))
    );

    // invalid name: empty
    let result = wallet.issue_asset_uda(
        online.clone(),
        TICKER.to_string(),
        s!(""),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == EMPTY_MSG));

    // invalid name: too long
    let result = wallet.issue_asset_uda(
        online.clone(),
        TICKER.to_string(),
        ("a").repeat(257),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid name: unicode characters
    let invalid_name = "name with ℧nicode characters";
    let result = wallet.issue_asset_uda(
        online.clone(),
        TICKER.to_string(),
        invalid_name.to_string(),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(
        matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_name).replace("{1}", "℧").replace("{2}", "10"))
    );

    // invalid details
    let result = test_issue_asset_uda_result(&mut wallet, &online, Some(""), None, vec![]);
    assert!(matches!(result, Err(Error::InvalidDetails { details: m }) if m == IDENT_EMPTY_MSG));

    // invalid precision
    let result = wallet.issue_asset_uda(
        online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        None,
        19,
        None,
        vec![],
    );
    assert!(
        matches!(result, Err(Error::InvalidPrecision { details: m }) if m == "precision is too high")
    );

    // invalid media: missing
    let result = test_issue_asset_uda_result(&mut wallet, &online, None, Some(missing_str), vec![]);
    assert!(matches!(result, Err(Error::InvalidFilePath { file_path: m }) if m == missing_str));

    // invalid media: empty
    let result = test_issue_asset_uda_result(&mut wallet, &online, None, Some(&empty_str), vec![]);
    assert!(matches!(result, Err(Error::EmptyFile { file_path: m }) if m == empty_str));

    // invalid attachment: missing
    let result = test_issue_asset_uda_result(&mut wallet, &online, None, None, vec![missing_str]);
    assert!(matches!(result, Err(Error::InvalidFilePath { file_path: m }) if m == missing_str));

    // invalid attachment: empty
    let result = test_issue_asset_uda_result(&mut wallet, &online, None, None, vec![&empty_str]);
    assert!(matches!(result, Err(Error::EmptyFile { file_path: m }) if m == empty_str));

    // new wallet
    let (mut wallet, online) = get_empty_wallet!();

    // insufficient funds
    let result = test_issue_asset_uda_result(&mut wallet, &online, None, None, vec![]);
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
    let result = test_issue_asset_uda_result(&mut wallet, &online, None, None, vec![]);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // too many attachments
    let result = test_issue_asset_uda_result(
        &mut wallet,
        &online,
        None,
        None,
        [attachment_str.clone()]
            .iter()
            .cycle()
            .take(MAX_ATTACHMENTS + 1)
            .map(|a| a.as_str())
            .collect(),
    );
    let details = format!("no more than {MAX_ATTACHMENTS} attachments are supported");
    assert!(matches!(result, Err(Error::InvalidAttachments { details: m }) if m == details));
}
