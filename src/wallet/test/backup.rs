use crate::wallet::backup::restore_backup;

use super::*;

#[test]
fn success() {
    initialize();

    let amount: u64 = 66;
    let backup_file = format!("{TEST_DATA_DIR}/test_backup_success.rgb-lib_backup");
    let password = "password";

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let mut wallet_data = wallet.wallet_data.clone();
    let wallet_dir = wallet.wallet_dir.clone();

    // issue
    let asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let blind_data = rcv_wallet
        .blind(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            blinded_utxo: blind_data.blinded_utxo,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    stop_mining();
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(true);
    rcv_wallet
        .refresh(rcv_online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // pre-backup wallet data
    check_test_wallet_data(&mut wallet, online.clone(), &asset, None, 1, amount);

    // backup
    println!("\nbacking up...");
    wallet.backup(&backup_file, password).unwrap();

    // drop wallets
    drop(online);
    drop(wallet);

    // restore
    println!("\nrestoring...");
    restore_backup(&backup_file, password, RESTORE_DIR).unwrap();

    // check original and restored data are the same
    println!("\ncomparing data...");
    let restore_wallet_dir = PathBuf::from_str(RESTORE_DIR)
        .unwrap()
        .join(wallet_dir.file_name().unwrap());
    let (same, _msg) = compare_test_directories(&wallet_dir, &restore_wallet_dir, vec!["log"]);
    assert!(same);

    // post-restore wallet data
    wallet_data.data_dir = RESTORE_DIR.to_string();
    let mut wallet = Wallet::new(wallet_data).unwrap();
    let online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();
    check_test_wallet_data(&mut wallet, online.clone(), &asset, None, 1, amount);

    // spend asset once more and check wallet data again
    let blind_data = rcv_wallet
        .blind(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            blinded_utxo: blind_data.blinded_utxo,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());
    // take transfers from WaitingCounterparty to Settled
    stop_mining();
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(true);
    rcv_wallet
        .refresh(rcv_online, Some(asset.asset_id.clone()), vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    check_test_wallet_data(&mut wallet, online.clone(), &asset, None, 2, amount * 2);

    // issue a second asset with the restored wallet
    let _asset = wallet
        .issue_asset_rgb20(
            online,
            s!("AR"),
            s!("after restore"),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // cleanup
    std::fs::remove_file(&backup_file).unwrap_or_default();
}

#[test]
fn fail() {
    initialize();

    let backup_file = format!("{TEST_DATA_DIR}/test_backup_fail.rgb-lib_backup");

    let (wallet, _online) = get_empty_wallet!();

    // backup
    wallet.backup(&backup_file, "password").unwrap();

    // backup on same file twice
    let result = wallet.backup(&backup_file, "password");
    assert!(
        matches!(result, Err(Error::Internal { details: msg }) if msg.starts_with("The file already exists:"))
    );

    // restore with wrong password
    let result = restore_backup(&backup_file, "wrong password", RESTORE_DIR);
    assert!(matches!(result, Err(Error::Internal { details: _ })));

    std::fs::remove_file(&backup_file).unwrap_or_default();
}

#[test]
fn double_restore() {
    initialize();

    let amount: u64 = 66;
    let backup_file_1 = format!("{TEST_DATA_DIR}/test_double_restore_1.rgb-lib_backup");
    let backup_file_2 = format!("{TEST_DATA_DIR}/test_double_restore_2.rgb-lib_backup");
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
    let asset_1 = wallet_1
        .issue_asset_rgb20(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_2 = wallet_2
        .issue_asset_rgb20(
            online_2.clone(),
            s!("TICKER2"),
            s!("asset name 2"),
            PRECISION,
            vec![asset_2_supply],
        )
        .unwrap();

    // send
    let blind_data_1 = rcv_wallet
        .blind(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    let blind_data_2 = rcv_wallet
        .blind(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    let recipient_map_1 = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount,
            blinded_utxo: blind_data_1.blinded_utxo,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let recipient_map_2 = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount: amount * 2,
            blinded_utxo: blind_data_2.blinded_utxo,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send_default(&mut wallet_1, &online_1, recipient_map_1);
    let txid_2 = test_send_default(&mut wallet_2, &online_2, recipient_map_2);
    assert!(!txid_1.is_empty());
    assert!(!txid_2.is_empty());
    // take transfers from WaitingCounterparty to Settled
    stop_mining();
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_1.asset_id.clone()), vec![])
        .unwrap();
    wallet_2
        .refresh(online_2.clone(), Some(asset_2.asset_id.clone()), vec![])
        .unwrap();
    mine(true);
    rcv_wallet
        .refresh(rcv_online, Some(asset_1.asset_id.clone()), vec![])
        .unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_1.asset_id.clone()), vec![])
        .unwrap();
    wallet_2
        .refresh(online_2.clone(), Some(asset_2.asset_id.clone()), vec![])
        .unwrap();

    // pre-backup wallet data
    check_test_wallet_data(&mut wallet_1, online_1.clone(), &asset_1, None, 1, amount);
    check_test_wallet_data(
        &mut wallet_2,
        online_2.clone(),
        &asset_2,
        Some(asset_2_supply),
        1,
        amount * 2,
    );

    // backup
    println!("\nbacking up...");
    wallet_1.backup(&backup_file_1, password_1).unwrap();
    wallet_2.backup(&backup_file_2, password_2).unwrap();

    // drop wallets
    drop(online_1);
    drop(wallet_1);
    drop(online_2);
    drop(wallet_2);

    // restore
    println!("\nrestoring...");
    restore_backup(&backup_file_1, password_1, RESTORE_DIR).unwrap();
    restore_backup(&backup_file_2, password_2, RESTORE_DIR).unwrap();

    // check original and restored data are the same
    println!("\ncomparing data for wallet 1...");
    let restore_wallet_1_dir = PathBuf::from_str(RESTORE_DIR)
        .unwrap()
        .join(wallet_1_dir.file_name().unwrap());
    let (same, _msg) = compare_test_directories(&wallet_1_dir, &restore_wallet_1_dir, vec!["log"]);
    assert!(same);
    let restore_wallet_2_dir = PathBuf::from_str(RESTORE_DIR)
        .unwrap()
        .join(wallet_2_dir.file_name().unwrap());
    let (same, _msg) = compare_test_directories(&wallet_2_dir, &restore_wallet_2_dir, vec!["log"]);
    assert!(same);

    // post-restore wallet data
    wallet_1_data.data_dir = RESTORE_DIR.to_string();
    wallet_2_data.data_dir = RESTORE_DIR.to_string();
    let mut wallet_1 = Wallet::new(wallet_1_data).unwrap();
    let mut wallet_2 = Wallet::new(wallet_2_data).unwrap();
    let online_1 = wallet_1.go_online(true, ELECTRUM_URL.to_string()).unwrap();
    let online_2 = wallet_2.go_online(true, ELECTRUM_URL.to_string()).unwrap();
    check_test_wallet_data(&mut wallet_1, online_1.clone(), &asset_1, None, 1, amount);
    check_test_wallet_data(
        &mut wallet_2,
        online_2.clone(),
        &asset_2,
        Some(asset_2_supply),
        1,
        amount * 2,
    );

    // issue a second asset with the restored wallets
    wallet_1
        .issue_asset_rgb20(
            online_1,
            s!("ARW1"),
            s!("after restore wallet 1"),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    wallet_2
        .issue_asset_rgb20(
            online_2,
            s!("ARW2"),
            s!("after restore wallet 2"),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // cleanup
    std::fs::remove_file(&backup_file_1).unwrap_or_default();
    std::fs::remove_file(&backup_file_2).unwrap_or_default();
}
