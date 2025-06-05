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
    let password = "password";

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let mut wallet_data = wallet.wallet_data.clone();
    let wallet_dir = wallet.wallet_dir.clone();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, Some(&asset.asset_id), None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    // pre-backup wallet data
    check_test_wallet_data(&mut wallet, &asset, None, 1, amount);

    // backup
    println!("\nbacking up...");
    wallet.backup(backup_file, password).unwrap();

    // backup not required after doing one
    let backup_required = wallet.backup_info().unwrap();
    assert!(!backup_required);

    // drop wallets
    drop(online);
    drop(wallet);

    // restore
    println!("\nrestoring...");
    let target_dir_path = get_restore_dir_path(Some("success"));
    let target_dir = target_dir_path.to_str().unwrap();
    restore_backup(backup_file, password, target_dir).unwrap();

    // check original and restored data are the same
    println!("\ncomparing data...");
    let restore_wallet_dir = target_dir_path.join(wallet_dir.file_name().unwrap());
    compare_test_directories(&wallet_dir, &restore_wallet_dir, &["log"]);

    // post-restore wallet data
    wallet_data.data_dir = target_dir.to_string();
    let mut wallet = Wallet::new(wallet_data).unwrap();
    let online = test_go_online(&mut wallet, true, None);
    check_test_wallet_data(&mut wallet, &asset, None, 1, amount);

    // backup not required after restoring one
    let backup_required = wallet.backup_info().unwrap();
    assert!(!backup_required);

    // spend asset once more and check wallet data again
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, Some(&asset.asset_id), None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    check_test_wallet_data(&mut wallet, &asset, None, 2, amount * 2);

    // issue a second asset with the restored wallet
    let _asset = test_issue_asset_nia(&mut wallet, &online, None);
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
    wallet.backup(backup_file, "password").unwrap();

    // backup on same file twice
    let result = wallet.backup(backup_file, "password");
    assert!(matches!(result, Err(Error::FileAlreadyExists { path: _ })));

    // restore on existing wallet directory
    let wallet_dir = wallet.get_wallet_dir();
    let target_dir = wallet_dir.parent().unwrap();
    let result = restore_backup(backup_file, "password", target_dir.to_str().unwrap());
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
    let result = restore_backup(backup_file_wrong_ver, "password", target_dir);
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
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let mut wallet_1_data = wallet_1.wallet_data.clone();
    let mut wallet_2_data = wallet_2.wallet_data.clone();
    let wallet_1_dir = wallet_1.wallet_dir.clone();
    let wallet_2_dir = wallet_2.wallet_dir.clone();
    let asset_2_supply = AMOUNT * 2;

    // issue
    let asset_1 = test_issue_asset_nia(&mut wallet_1, &online_1, None);
    let asset_2 = test_issue_asset_nia(&mut wallet_2, &online_2, Some(&[asset_2_supply]));

    // send
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let receive_data_2 = test_blind_receive(&rcv_wallet);
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
    let txid_1 = test_send(&mut wallet_1, &online_1, &recipient_map_1);
    let txid_2 = test_send(&mut wallet_2, &online_2, &recipient_map_2);
    assert!(!txid_1.is_empty());
    assert!(!txid_2.is_empty());
    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset_1.asset_id), None);
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset_2.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, Some(&asset_1.asset_id), None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset_1.asset_id), None);
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset_2.asset_id), None);

    // pre-backup wallet data
    check_test_wallet_data(&mut wallet_1, &asset_1, None, 1, amount);
    check_test_wallet_data(&mut wallet_2, &asset_2, Some(asset_2_supply), 1, amount * 2);

    // backup
    println!("\nbacking up...");
    wallet_1.backup(backup_file_1, password_1).unwrap();
    let custom_params = ScryptParams::new(
        Some(Params::RECOMMENDED_LOG_N + 1),
        Some(Params::RECOMMENDED_R + 1),
        Some(Params::RECOMMENDED_P + 1),
    );
    wallet_2
        .backup_customize(backup_file_2, password_2, Some(custom_params))
        .unwrap();

    // drop wallets
    drop(online_1);
    drop(wallet_1);
    drop(online_2);
    drop(wallet_2);

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
    let mut wallet_1 = Wallet::new(wallet_1_data).unwrap();
    let mut wallet_2 = Wallet::new(wallet_2_data).unwrap();
    let online_1 = test_go_online(&mut wallet_1, true, None);
    let online_2 = test_go_online(&mut wallet_2, true, None);
    check_test_wallet_data(&mut wallet_1, &asset_1, None, 1, amount);
    check_test_wallet_data(&mut wallet_2, &asset_2, Some(asset_2_supply), 1, amount * 2);

    // issue a second asset with the restored wallets
    test_issue_asset_nia(&mut wallet_1, &online_1, None);
    test_issue_asset_nia(&mut wallet_2, &online_2, None);
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
