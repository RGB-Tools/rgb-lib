use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;
    let backup_file_path = get_test_data_dir_path().join("test_backup_success.rgb-lib_backup");
    let backup_file = backup_file_path.to_str().unwrap();
    let _ = std::fs::remove_file(backup_file);

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let mut wallet_data = party.get_wallet_data();
    let keys = party.get_keys();
    let wallet_dir = party.wallet.get_wallet_dir();

    // issue
    let asset = party.issue_asset_nia(None);

    // send
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    party.wait_for_refresh(Some(&asset.asset_id));

    // pre-backup wallet data
    party.check_test_wallet_data(&asset, None, 1, amount);

    // backup
    println!("\nbacking up...");
    party.wallet.backup(backup_file, PASSWORD).unwrap();

    // backup not required after doing one
    let backup_required = party.wallet.backup_info().unwrap();
    assert!(!backup_required);

    // drop wallets
    drop(party);

    // restore
    println!("\nrestoring...");
    let target_dir_path = get_restore_dir_path(Some("success"));
    let target_dir = target_dir_path.to_str().unwrap();
    restore_backup(backup_file, PASSWORD, target_dir).unwrap();

    // check original and restored data are the same
    println!("\ncomparing data...");
    let restore_wallet_dir = target_dir_path.join(wallet_dir.file_name().unwrap());
    compare_test_directories(&wallet_dir, &restore_wallet_dir, &["log"]);

    // post-restore wallet data
    wallet_data.data_dir = target_dir.to_string();
    let mut party = offline_party!(Wallet::new(wallet_data, keys.clone()).unwrap());
    let online = party.go_online(true, None);
    let mut party = party!(party.wallet, online);
    party.check_test_wallet_data(&asset, None, 1, amount);

    // backup not required after restoring one
    let backup_required = party.wallet.backup_info().unwrap();
    assert!(!backup_required);

    // spend asset once more and check wallet data again
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    party.wait_for_refresh(Some(&asset.asset_id));
    party.check_test_wallet_data(&asset, None, 2, amount * 2);

    // issue a second asset with the restored wallet
    let _asset = party.issue_asset_nia(None);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    // services are unnecessary here but this prevents removal of the test dir during exectution
    initialize();

    let backup_file_path = get_test_data_dir_path().join("test_backup_fail.rgb-lib_backup");
    let backup_file = backup_file_path.to_str().unwrap();
    let _ = std::fs::remove_file(backup_file);

    let wallet = get_test_wallet(true, None);

    // backup
    wallet.backup(backup_file, PASSWORD).unwrap();

    // backup on same file twice
    let result = wallet.backup(backup_file, PASSWORD);
    assert!(matches!(result, Err(Error::FileAlreadyExists { path: _ })));

    // restore on existing wallet directory
    let wallet_dir = wallet.get_wallet_dir();
    let target_dir = wallet_dir.parent().unwrap();
    let result = restore_backup(backup_file, PASSWORD, target_dir.to_str().unwrap());
    assert!(
        matches!(result, Err(Error::WalletDirAlreadyExists { path: td }) if td == wallet_dir.to_str().unwrap())
    );

    // restore with wrong password
    let target_dir_path = get_restore_dir_path(Some("wrong_password"));
    let target_dir = target_dir_path.to_str().unwrap();
    let result = restore_backup(backup_file, "wrong password", target_dir);
    assert!(matches!(result, Err(Error::WrongPassword)));

    // restore with wrong version
    let target_dir_path = get_restore_dir_path(Some("wrong_version"));
    let target_dir = target_dir_path.to_str().unwrap();
    let backup_parent = backup_file_path.parent().unwrap().to_path_buf();
    let files = get_backup_paths(&backup_parent).unwrap();
    std::fs::create_dir_all(target_dir).unwrap();
    let (logger, _logger_guard) =
        setup_logger(Path::new(&target_dir), Some("restore_bad_version")).unwrap();
    unzip(
        &backup_file_path,
        &PathBuf::from(files.tempdir.path()),
        &logger,
    )
    .unwrap();
    let json_pub_data = fs::read_to_string(&files.backup_pub_data).unwrap();
    let mut backup_pub_data: BackupPubData = serde_json::from_str(json_pub_data.as_str())
        .map_err(InternalError::from)
        .unwrap();
    let wrong_ver = 0;
    backup_pub_data.version = wrong_ver;
    fs::write(
        &files.backup_pub_data,
        serde_json::to_string(&backup_pub_data).unwrap(),
    )
    .unwrap();
    let backup_file_wrong_ver_path =
        get_test_data_dir_path().join("test_backup_fail.rgb-lib_backup.wrong_ver");
    let backup_file_wrong_ver = backup_file_wrong_ver_path.to_str().unwrap();
    let _ = std::fs::remove_file(backup_file_wrong_ver);
    zip_dir(
        &PathBuf::from(files.tempdir.path()),
        &PathBuf::from(backup_file_wrong_ver),
        false,
        &logger,
    )
    .unwrap();
    let result = restore_backup(backup_file_wrong_ver, PASSWORD, target_dir);
    assert!(
        matches!(result, Err(Error::UnsupportedBackupVersion { version: v }) if v == wrong_ver.to_string())
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn double_restore() {
    initialize();

    let amount: u64 = 66;
    let backup_file_1_path = get_test_data_dir_path().join("test_double_restore_1.rgb-lib_backup");
    let backup_file_1 = backup_file_1_path.to_str().unwrap();
    let _ = std::fs::remove_file(backup_file_1);
    let backup_file_2_path = get_test_data_dir_path().join("test_double_restore_2.rgb-lib_backup");
    let backup_file_2 = backup_file_2_path.to_str().unwrap();
    let _ = std::fs::remove_file(backup_file_2);
    let password_1 = "password1";
    let password_2 = "password2";

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let mut wallet_1_data = party_1.get_wallet_data();
    let keys_1 = party_1.get_keys();
    let mut wallet_2_data = party_2.get_wallet_data();
    let keys_2 = party_2.get_keys();
    let wallet_1_dir = party_1.wallet.get_wallet_dir();
    let wallet_2_dir = party_2.wallet.get_wallet_dir();
    let asset_2_supply = AMOUNT * 2;

    // issue
    let asset_1 = party_1.issue_asset_nia(None);
    let asset_2 = party_2.issue_asset_nia(Some(&[asset_2_supply]));

    // send
    let receive_data_1 = rcv_party.blind_receive();
    let receive_data_2 = rcv_party.blind_receive();
    let recipient_map_1 = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_1.recipient_id,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let recipient_map_2 = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount * 2),
            recipient_id: receive_data_2.recipient_id,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_1.send_retry(&recipient_map_1);
    let txid_2 = party_2.send_retry(&recipient_map_2);
    assert!(!txid_1.is_empty());
    assert!(!txid_2.is_empty());
    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset_1.asset_id));
    party_2.wait_for_refresh(Some(&asset_2.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(Some(&asset_1.asset_id));
    party_1.wait_for_refresh(Some(&asset_1.asset_id));
    party_2.wait_for_refresh(Some(&asset_2.asset_id));

    // pre-backup wallet data
    party_1.check_test_wallet_data(&asset_1, None, 1, amount);
    party_2.check_test_wallet_data(&asset_2, Some(asset_2_supply), 1, amount * 2);

    // backup
    println!("\nbacking up...");
    party_1.wallet.backup(backup_file_1, password_1).unwrap();
    let custom_params = ScryptParams::new(
        Some(Params::RECOMMENDED_LOG_N + 1),
        Some(Params::RECOMMENDED_R + 1),
        Some(Params::RECOMMENDED_P + 1),
    );
    party_2
        .wallet
        .backup_customize(backup_file_2, password_2, Some(custom_params))
        .unwrap();

    // drop wallets
    drop(party_1);
    drop(party_2);

    // restore
    println!("\nrestoring...");
    let target_dir_path_1 = get_restore_dir_path(Some("double_1"));
    let target_dir_path_2 = get_restore_dir_path(Some("double_2"));
    let target_dir_1 = target_dir_path_1.to_str().unwrap();
    let target_dir_2 = target_dir_path_2.to_str().unwrap();
    restore_backup(backup_file_1, password_1, target_dir_1).unwrap();
    restore_backup(backup_file_2, password_2, target_dir_2).unwrap();

    // check original and restored data are the same
    println!("\ncomparing data for wallet 1...");
    let restore_wallet_1_dir = target_dir_path_1.join(wallet_1_dir.file_name().unwrap());
    compare_test_directories(&wallet_1_dir, &restore_wallet_1_dir, &["log"]);
    let restore_wallet_2_dir = target_dir_path_2.join(wallet_2_dir.file_name().unwrap());
    compare_test_directories(&wallet_2_dir, &restore_wallet_2_dir, &["log"]);

    // post-restore wallet data
    wallet_1_data.data_dir = target_dir_1.to_string();
    wallet_2_data.data_dir = target_dir_2.to_string();
    let mut party_1 = offline_party!(Wallet::new(wallet_1_data, keys_1.clone()).unwrap());
    let mut party_2 = offline_party!(Wallet::new(wallet_2_data, keys_2.clone()).unwrap());
    let online_1 = party_1.go_online(true, None);
    let online_2 = party_2.go_online(true, None);
    let mut party_1 = party!(party_1.wallet, online_1);
    let mut party_2 = party!(party_2.wallet, online_2);
    party_1.check_test_wallet_data(&asset_1, None, 1, amount);
    party_2.check_test_wallet_data(&asset_2, Some(asset_2_supply), 1, amount * 2);

    // issue a second asset with the restored wallets
    party_1.issue_asset_nia(None);
    party_2.issue_asset_nia(None);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn backup_info() {
    // services are unnecessary here but this prevents removal of the test dir during exectution
    initialize();

    // wallets
    let wallet = get_test_wallet(true, None);

    // backup not required for new wallets
    let backup_required = wallet.backup_info().unwrap();
    assert!(!backup_required);
}
