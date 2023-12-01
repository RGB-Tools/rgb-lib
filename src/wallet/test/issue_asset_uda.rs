use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let settled = 1;
    let file_str = "README.md";
    let image_str = ["tests", "qrcode.png"].join(&MAIN_SEPARATOR.to_string());

    let (wallet, online) = get_funded_wallet!();

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _receive_data = test_blind_receive(&wallet);

    // required fields only
    println!("\nasset 1");
    let before_timestamp = now().unix_timestamp();
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let asset_1 = wallet
        .issue_asset_uda(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            None,
            PRECISION,
            None,
            vec![],
        )
        .unwrap();
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    show_unspent_colorings(&wallet, "after issuance 1");
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
        &wallet,
        &online,
        Some(file_str.to_string()),
        vec![image_str.to_string(), file_str.to_string()],
    );
    show_unspent_colorings(&wallet, "after issuance 2");
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
    let src_bytes = std::fs::read(PathBuf::from(image_str)).unwrap();
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
}
