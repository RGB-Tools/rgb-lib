use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let settled = 1;
    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);

    let mut party = get_funded_noutxo_party!();

    // prepare UTXOs
    party.create_utxos(true, Some(1), Some(5000), FEE_RATE, None);

    // required fields only
    println!("\nasset 1");
    let before_timestamp = now().unix_timestamp();
    let bak_info_before = party.db_backup_info();
    let asset_1 = party.issue_asset_uda(None, None, vec![]);
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // check the asset has been saved with the correct schema
    let uda_asset_list = party.list_assets(&[AssetSchema::Uda]).uda.unwrap();
    assert!(
        uda_asset_list
            .into_iter()
            .any(|a| a.asset_id == asset_1.asset_id)
    );

    // add a pending operation to the UTXO so spendable balance will be != settled / future
    let _receive_data = party.blind_receive();
    party.show_unspent_colorings("after issuance 1");

    // checks
    let balance_1 = party.get_asset_balance(&asset_1.asset_id);
    assert_eq!(asset_1.ticker, TICKER.to_string());
    assert_eq!(asset_1.name, NAME.to_string());
    assert_eq!(asset_1.details, None);
    assert_eq!(asset_1.precision, PRECISION);
    assert_eq!(
        balance_1,
        Balance {
            settled,
            future: settled,
            spendable: 0,
        }
    );
    let token = asset_1.token.unwrap();
    assert_eq!(token.index, UDA_FIXED_INDEX);
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
    let asset_2 = party.issue_asset_uda(Some(DETAILS), Some(FILE_STR), vec![&image_str, FILE_STR]);
    party.show_unspent_colorings("after issuance 2");
    let balance_2 = party.get_asset_balance(&asset_2.asset_id);
    assert_eq!(asset_2.ticker, TICKER.to_string());
    assert_eq!(asset_2.name, NAME.to_string());
    assert_eq!(asset_2.details, Some(DETAILS.to_string()));
    assert_eq!(asset_2.precision, PRECISION);
    assert_eq!(
        balance_2,
        Balance {
            settled,
            future: settled,
            spendable: 0,
        }
    );
    let token = asset_2.token.unwrap();
    assert!(token.media.is_some());
    assert!(!token.attachments.is_empty());
    // check media file contents match
    let media = token.media.unwrap();
    assert_eq!(media.mime, "text/plain");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(FILE_STR)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check digest for provided file matches
    let src_digest = hash_bytes_hex(&src_bytes[..]);
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
    // check attachments
    let media = token.attachments.get(&0).unwrap();
    assert_eq!(media.mime, "image/png");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(image_str.clone())).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_digest = hash_bytes_hex(&src_bytes[..]);
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
    let media = token.attachments.get(&1).unwrap();
    assert_eq!(media.mime, "text/plain");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(FILE_STR)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_digest = hash_bytes_hex(&src_bytes[..]);
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);

    // maximum number of attachments
    println!("\nmatching max attachment number");
    let asset_3 = party.issue_asset_uda(
        None,
        None,
        [image_str.clone()]
            .iter()
            .cycle()
            .take(MAX_ATTACHMENTS)
            .map(|a| a.as_str())
            .collect(),
    );
    party.show_unspent_colorings("after issuance 3");
    let balance_3 = party.get_asset_balance(&asset_3.asset_id);
    assert_eq!(
        balance_3,
        Balance {
            settled,
            future: settled,
            spendable: 0,
        }
    );
    let token = asset_3.token.unwrap();
    assert!(!token.embedded_media);
    assert_eq!(token.media, None);
    let attachments_bytes = std::fs::read(PathBuf::from(image_str.clone())).unwrap();
    let attachment_digest = hash_bytes_hex(&attachments_bytes[..]);
    let attachment_path = party
        .wallet
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

    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_empty_party!();

    // prepare UTXO
    party.create_utxos(true, Some(1), Some(5000), FEE_RATE, None);

    // issue 1st asset
    let asset_1 = party.issue_asset_uda(None, None, vec![]);
    // get 1st issuance UTXO
    let unspents = party.list_unspents(false);
    let unspent_1 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        })
        .unwrap();
    // send 1st asset
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // issuing a 2nd asset fails due to missing free allocation slot
    let result = party.issue_asset_uda_result(None, None, vec![]);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // create 1 more UTXO issue 2nd asset
    party.create_utxos(false, Some(1), None, FEE_RATE, None);
    let asset_2 = party.issue_asset_uda(None, None, vec![]);
    party.show_unspent_colorings("after 2nd issuance");
    // get 2nd issuance UTXO
    let unspents = party.list_unspents(false);
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
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset_1.asset_id));

    // issue 3rd asset
    let asset_3 = party.issue_asset_uda(None, None, vec![]);
    party.show_unspent_colorings("after 3rd issuance");
    // get 3rd issuance UTXO
    let unspents = party.list_unspents(false);
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
    let missing_str = "missing";
    let empty_path = tempfile::NamedTempFile::with_prefix("issue_asset_uda::fail_").unwrap();
    fs::File::create(&empty_path).unwrap();
    let empty_str = empty_path.path().to_str().unwrap().to_string();

    // wallet
    let mut party = get_funded_party!();

    // invalid ticker: empty
    let result =
        party
            .wallet
            .issue_asset_uda(s!(""), NAME.to_string(), None, PRECISION, None, vec![]);
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == EMPTY_MSG));

    // invalid ticker: too long
    let result = party.wallet.issue_asset_uda(
        s!("ABCDEFGHI"),
        NAME.to_string(),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid ticker: lowercase
    let result = party.wallet.issue_asset_uda(
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
    let result = party.wallet.issue_asset_uda(
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

    // invalid ticker: starting with a number
    let invalid_ticker = "1TICKER";
    let result = party.wallet.issue_asset_uda(
        invalid_ticker.to_string(),
        NAME.to_string(),
        None,
        PRECISION,
        None,
        vec![],
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_START_MSG
            .replace("{0}", invalid_ticker).replace("{1}", "1"))
    );

    // invalid name: empty
    let result =
        party
            .wallet
            .issue_asset_uda(TICKER.to_string(), s!(""), None, PRECISION, None, vec![]);
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == EMPTY_MSG));

    // invalid name: too long
    let result = party.wallet.issue_asset_uda(
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
    let result = party.wallet.issue_asset_uda(
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
    let result = party.issue_asset_uda_result(Some(""), None, vec![]);
    assert!(matches!(result, Err(Error::InvalidDetails { details: m }) if m == IDENT_EMPTY_MSG));

    // invalid precision
    let result =
        party
            .wallet
            .issue_asset_uda(TICKER.to_string(), NAME.to_string(), None, 19, None, vec![]);
    assert!(
        matches!(result, Err(Error::InvalidPrecision { details: m }) if m == "precision is too high")
    );

    // invalid media: missing
    let result = party.issue_asset_uda_result(None, Some(missing_str), vec![]);
    assert!(matches!(result, Err(Error::InvalidFilePath { file_path: m }) if m == missing_str));

    // invalid media: empty
    let result = party.issue_asset_uda_result(None, Some(&empty_str), vec![]);
    assert!(matches!(result, Err(Error::EmptyFile { file_path: m }) if m == empty_str));

    // invalid attachment: missing
    let result = party.issue_asset_uda_result(None, None, vec![missing_str]);
    assert!(matches!(result, Err(Error::InvalidFilePath { file_path: m }) if m == missing_str));

    // invalid attachment: empty
    let result = party.issue_asset_uda_result(None, None, vec![&empty_str]);
    assert!(matches!(result, Err(Error::EmptyFile { file_path: m }) if m == empty_str));

    // new wallet
    let mut empty_party = get_empty_party!();

    // insufficient funds
    let result = empty_party.issue_asset_uda_result(None, None, vec![]);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    fund_wallet(empty_party.get_address());
    mine(false);

    // insufficient allocations
    let result = empty_party.issue_asset_uda_result(None, None, vec![]);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // too many attachments
    let result = empty_party.issue_asset_uda_result(
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
