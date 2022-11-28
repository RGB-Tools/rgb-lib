use super::*;

#[test]
fn success() {
    initialize();

    let file_str = "README.md";

    let (mut wallet, online) = get_funded_wallet!();

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _blind_data = wallet.blind(None, None);

    // required fields only
    println!("asset 1");
    let asset_1 = wallet
        .issue_asset_rgb121(
            online.clone(),
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT, AMOUNT],
            None,
            None,
        )
        .unwrap();
    show_unspent_colorings(&wallet, "after issuance 1");
    assert_eq!(asset_1.name, NAME.to_string());
    assert_eq!(asset_1.description, Some(DESCRIPTION.to_string()));
    assert_eq!(asset_1.precision, PRECISION);
    assert_eq!(
        asset_1.balance,
        Balance {
            settled: AMOUNT * 2,
            future: AMOUNT * 2,
            spendable: AMOUNT,
        }
    );
    assert!(asset_1.parent_id.is_none());
    let empty_data_paths = vec![];
    assert_eq!(asset_1.data_paths, empty_data_paths);

    // check the asset type is correct
    let asset_type = wallet
        .database
        .get_asset_or_fail(asset_1.asset_id.clone())
        .unwrap();
    assert_eq!(asset_type, AssetType::Rgb121);

    // include a parent_id and a file
    println!("asset 2");
    let asset_2 = wallet
        .issue_asset_rgb121(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            Some(asset_1.asset_id.clone()),
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
            spendable: 0,
        }
    );
    assert_eq!(asset_2.parent_id, Some(asset_1.asset_id));
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
    let src_attachment_id = AttachmentId::commit(&src_hash).to_string();
    let dst_attachment_id = Path::new(&dst_path)
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy();
    assert_eq!(src_attachment_id, dst_attachment_id);
}

#[test]
fn multi_success() {
    initialize();

    let amounts: Vec<u64> = vec![111, 222, 333, 444, 555];
    let sum: u64 = amounts.iter().sum();
    let file_str = "README.md";

    let (mut wallet, online) = get_funded_wallet!();

    let asset = wallet
        .issue_asset_rgb121(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            amounts.clone(),
            None,
            Some(file_str.to_string()),
        )
        .unwrap();

    // check balance is the sum of the amounts
    assert_eq!(asset.balance.settled, sum);

    // check each allocation ends up on a different UTXO
    let unspents: Vec<Unspent> = wallet
        .list_unspents(true)
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
    let rgb121_asset_list = wallet.list_assets(vec![]).unwrap().rgb121.unwrap();
    let rgb121_asset = rgb121_asset_list
        .into_iter()
        .find(|a| a.asset_id == asset.asset_id)
        .unwrap();
    assert_eq!(rgb121_asset.data_paths.len(), 1);
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // bad online object
    let (_other_wallet, other_online) = get_funded_wallet!();
    let result = wallet.issue_asset_rgb121(
        other_online,
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::InvalidOnline())));

    // invalid name: too short
    let result = wallet.issue_asset_rgb121(
        online.clone(),
        s!(""),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid name: too long
    let result = wallet.issue_asset_rgb121(
        online.clone(),
        ("a").repeat(257),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid name: unicode characters
    let result = wallet.issue_asset_rgb121(
        online.clone(),
        s!("name with ℧nicode characters"),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::InvalidName(_))));

    // invalid description: unicode characters
    let result = wallet.issue_asset_rgb121(
        online.clone(),
        NAME.to_string(),
        Some(s!("description with ℧nicode characters")),
        PRECISION,
        vec![AMOUNT],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::InvalidDescription(_))));

    // invalid precision
    let result = wallet.issue_asset_rgb121(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        19,
        vec![AMOUNT],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid amount list
    let result = wallet.issue_asset_rgb121(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        19,
        vec![],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::NoIssuanceAmounts)));

    // invalid parent_id list
    let result = wallet.issue_asset_rgb121(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        19,
        vec![AMOUNT],
        Some(s!("")),
        None,
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid file_path
    let invalid_file_path = s!("invalid");
    let result = wallet.issue_asset_rgb121(
        online,
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        19,
        vec![AMOUNT],
        None,
        Some(invalid_file_path.clone()),
    );
    assert!(matches!(
        result,
        Err(Error::InvalidFilePath(t)) if t == invalid_file_path
    ));

    // insufficient funds
    let (mut empty_wallet, empty_online) = get_empty_wallet!();
    let result = empty_wallet.issue_asset_rgb121(
        empty_online,
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::InsufficientBitcoins)));

    // insufficient allocations
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let result = wallet.issue_asset_rgb121(
        online,
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![AMOUNT],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[test]
#[ignore = "currently succeeds"]
fn zero_amount_fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // invalid amount
    let result = wallet.issue_asset_rgb121(
        online,
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        vec![0],
        None,
        None,
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));
}
