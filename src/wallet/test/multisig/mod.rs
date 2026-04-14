#[macro_use]
mod utils;

use super::*;
use utils::*;

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn success() {
    initialize();
    op_counter_reset();

    let bitcoin_network = BitcoinNetwork::Regtest;
    let threshold_colored = 3;
    let threshold_vanilla = 3;
    let random_str: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();

    // multisig wallet keys
    let wlt_1_keys = generate_keys(bitcoin_network);
    let wlt_2_keys = generate_keys(bitcoin_network);
    let wlt_3_keys = generate_keys(bitcoin_network);
    let wlt_4_keys = generate_keys(bitcoin_network);

    // cosigners
    let cosigners = vec![
        Cosigner::from_keys(&wlt_1_keys, None),
        Cosigner::from_keys(&wlt_2_keys, None),
        Cosigner::from_keys(&wlt_3_keys, None),
        Cosigner::from_keys(&wlt_4_keys, None),
    ];
    let cosigner_xpubs: Vec<String> = cosigners
        .iter()
        .map(|c| c.account_xpub_colored.clone())
        .collect();

    // biscuit token setup
    // - roots
    let root_keypair = KeyPair::new();
    let root_public_key = root_keypair.public();
    // - cosigners
    let mut cosigner_tokens = vec![];
    for cosigner_xpub in &cosigner_xpubs {
        cosigner_tokens.push(create_token(
            &root_keypair,
            Role::Cosigner(cosigner_xpub.clone()),
            None,
        ));
    }
    // - watch-only
    let wo_token = create_token(&root_keypair, Role::WatchOnly, None);

    // hub setup
    write_hub_config(
        &cosigner_xpubs,
        threshold_colored,
        threshold_vanilla,
        root_public_key.to_bytes_hex(),
        None,
    );
    restart_multisig_hub();

    // multisig wallets
    let multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, threshold_vanilla);
    let mut wlt_1_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_1"));
    let wlt_1_multisig_online = ms_go_online(&mut wlt_1_multisig, &cosigner_tokens[0]);
    let mut wlt_2_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_2"));
    let wlt_2_multisig_online = ms_go_online(&mut wlt_2_multisig, &cosigner_tokens[1]);
    let mut wlt_3_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_3"));
    let wlt_3_multisig_online = ms_go_online(&mut wlt_3_multisig, &cosigner_tokens[2]);
    let mut wlt_4_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_4"));
    let wlt_4_multisig_online = ms_go_online(&mut wlt_4_multisig, &cosigner_tokens[3]);

    // singlesig wallets (for signing)
    let wlt_1_singlesig = get_test_wallet_with_keys(&wlt_1_keys);
    let wlt_2_singlesig = get_test_wallet_with_keys(&wlt_2_keys);
    let wlt_3_singlesig = get_test_wallet_with_keys(&wlt_3_keys);
    let wlt_4_singlesig = get_test_wallet_with_keys(&wlt_4_keys);

    // watch-only wallet
    let mut wlt_wo_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_wo"));
    let wlt_wo_multisig_online = ms_go_online(&mut wlt_wo_multisig, &wo_token);

    // multisig parties
    let mut wlt_1 = ms_party!(
        &wlt_1_singlesig,
        &mut wlt_1_multisig,
        wlt_1_multisig_online,
        &cosigner_xpubs[0]
    );
    let mut wlt_2 = ms_party!(
        &wlt_2_singlesig,
        &mut wlt_2_multisig,
        wlt_2_multisig_online,
        &cosigner_xpubs[1]
    );
    let mut wlt_3 = ms_party!(
        &wlt_3_singlesig,
        &mut wlt_3_multisig,
        wlt_3_multisig_online,
        &cosigner_xpubs[2]
    );
    let mut wlt_4 = ms_party!(
        &wlt_4_singlesig,
        &mut wlt_4_multisig,
        wlt_4_multisig_online,
        &cosigner_xpubs[3]
    );

    // watch-only party
    let mut wlt_wo = ms_party!(&mut wlt_wo_multisig, wlt_wo_multisig_online);

    // check descriptors
    let descriptors = multisig_wlt_keys
        .build_descriptors(bitcoin_network)
        .unwrap();
    for wlt in [&wlt_1, &wlt_2, &wlt_3, &wlt_4] {
        let wlt_keys = wlt.multisig.get_keys();
        assert_eq!(wlt_keys, multisig_wlt_keys);
        let wlt_descriptors = wlt.multisig.get_descriptors();
        assert_eq!(wlt_descriptors, descriptors);
    }
    let wlt_keys = wlt_wo.multisig.get_keys();
    assert_eq!(wlt_keys, multisig_wlt_keys);
    let wlt_descriptors = wlt_wo.multisig.get_descriptors();
    assert_eq!(wlt_descriptors, descriptors);

    // fund wallet 1
    let sats = 30_000;
    send_sats_to_address(wlt_1.get_address(), Some(sats));
    mine(false, false);

    check_hub_info(&mut [&mut wlt_1, &mut wlt_2, &mut wlt_3]);

    println!("\n=== create UTXOs discarded (wlt_1) ===");
    check_wallets_up_to_date(&mut [&mut wlt_1, &mut wlt_2, &mut wlt_3]);
    let op_init = wlt_1.create_utxos_init(false, None, None, FEE_RATE);
    operation_complete::<CreateUtxosHandler>(
        op_init.operation_idx,
        &mut [&mut wlt_1],
        &mut [&mut wlt_2, &mut wlt_3],
        &mut [],
        false,
    );

    // check PSBT signature inspection
    inspect_create_utxos(
        &mut wlt_1,
        &op_init.psbt,
        None,
        &HashMap::from_iter([(0, sats)]),
        None,
        None,
        0,
    );
    let signed_1 = wlt_1.sign(&op_init.psbt);
    inspect_create_utxos(
        &mut wlt_1,
        &signed_1,
        None,
        &HashMap::from_iter([(0, sats)]),
        None,
        None,
        1,
    );
    let signed_2 = wlt_2.sign(&signed_1);
    inspect_create_utxos(
        &mut wlt_1,
        &signed_2,
        None,
        &HashMap::from_iter([(0, sats)]),
        None,
        None,
        2,
    );
    let signed_3 = wlt_3.sign(&signed_2);
    inspect_create_utxos(
        &mut wlt_1,
        &signed_3,
        None,
        &HashMap::from_iter([(0, sats)]),
        None,
        None,
        3,
    );
    let signed_4 = wlt_4.sign(&signed_3);
    inspect_create_utxos(
        &mut wlt_1,
        &signed_4,
        None,
        &HashMap::from_iter([(0, sats)]),
        None,
        None,
        4,
    );

    println!("\n=== create UTXOs (wlt_1) ===");
    sync_wallets_full(&mut [&mut wlt_4]);
    check_wallets_up_to_date(&mut [&mut wlt_1, &mut wlt_2, &mut wlt_3, &mut wlt_4]);
    let utxo_num = 20;
    let utxo_size = 1000;
    let op_init = wlt_1.create_utxos_init(false, Some(utxo_num), Some(utxo_size), FEE_RATE);
    inspect_create_utxos(
        &mut wlt_1,
        &op_init.psbt,
        None,
        &HashMap::from_iter([(0, sats)]),
        Some(utxo_num),
        Some(utxo_size),
        0,
    );
    operation_complete::<CreateUtxosHandler>(
        op_init.operation_idx,
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [&mut wlt_4],
        &mut [],
        true,
    );
    check_last_transaction(
        &mut [
            wlt_1.multisig_mut(),
            wlt_2.multisig_mut(),
            wlt_3.multisig_mut(),
        ],
        &op_init.psbt,
        &TransactionType::CreateUtxos,
    );

    println!("\n=== issue CFA ===");
    let IssuedAsset::Cfa(cfa_asset) = issue_asset(
        &mut wlt_2,
        &mut [&mut wlt_1, &mut wlt_3],
        AssetSchema::Cfa,
        Some(&[200, AMOUNT_SMALL]),
        None,
    ) else {
        unreachable!()
    };

    println!("\n=== send BTC discarded (wlt_3 → wlt_1) ===");
    check_wallets_up_to_date(&mut [&mut wlt_1, &mut wlt_2, &mut wlt_3]);
    let addr = wlt_1.get_address();
    let op_init = wlt_3.send_btc_init(&addr, 999);
    operation_complete::<SendBtcHandler>(
        op_init.operation_idx,
        &mut [],
        &mut [&mut wlt_3, &mut wlt_2],
        &mut [&mut wlt_1],
        false,
    );

    println!("\n=== issue NIA ===");
    let IssuedAsset::Nia(nia_asset_1) = issue_asset(
        &mut wlt_2,
        &mut [&mut wlt_1, &mut wlt_3],
        AssetSchema::Nia,
        Some(&[50, 70, 30]),
        None,
    ) else {
        unreachable!()
    };

    println!("\n=== issue UDA ===");
    let IssuedAsset::Uda(uda_asset) = issue_asset(
        &mut wlt_3,
        &mut [&mut wlt_1, &mut wlt_2],
        AssetSchema::Uda,
        None,
        None,
    ) else {
        unreachable!()
    };

    let (mut singlesig_wlt, singlesig_wlt_online) = get_funded_wallet!();
    let mut singlesig_wlt = party!(&mut singlesig_wlt, singlesig_wlt_online);

    println!("\n=== send UDA (wlt_1 → singlesig) ===");
    sync_wallets_full(&mut [&mut wlt_4]);
    check_wallets_up_to_date(&mut [&mut wlt_1, &mut wlt_2, &mut wlt_3, &mut wlt_4]);
    let rcv_data = test_blind_receive(singlesig_wlt.wallet);
    let recipient_map = HashMap::from([(
        uda_asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: rcv_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let op_init = wlt_1.send_init(recipient_map);
    let bt_before = wlt_2.bak_ts();
    operation_complete::<SendRgbHandler>(
        op_init.operation_idx,
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [],
        &mut [&mut wlt_4],
        true,
    );
    assert!(wlt_2.bak_ts() > bt_before);
    check_wallets_up_to_date(&mut [&mut wlt_1, &mut wlt_2, &mut wlt_3]);
    settle_transfer(
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [&mut singlesig_wlt],
        Some(&uda_asset.asset_id),
        None,
        Some(&op_init.psbt),
        true,
    );
    check_transfer_status(
        &[
            &wlt_1 as &dyn SigParty,
            &wlt_2 as &dyn SigParty,
            &wlt_3 as &dyn SigParty,
            &singlesig_wlt as &dyn SigParty,
        ],
        &[Some(&uda_asset.asset_id)],
        None,
        TransferStatus::Settled,
    );
    check_asset_balance(&[&singlesig_wlt], &uda_asset.asset_id, (1, 1, 1));
    check_asset_balance(&[&wlt_1, &wlt_2, &wlt_3], &uda_asset.asset_id, (0, 0, 0));

    let last_wlt_4_op = op_init.operation_idx;

    println!("\n=== witness receive UDA (singlesig → wlt_3) ===");
    let receive_data = wlt_3.witness_receive();
    sync_wallets_full(&mut [&mut wlt_1, &mut wlt_2]);
    let recipient_map = HashMap::from([(
        uda_asset.asset_id.clone(),
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
    let txid = test_send(singlesig_wlt.wallet, singlesig_wlt.online, &recipient_map);
    settle_transfer(
        &mut [&mut singlesig_wlt],
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        Some(&uda_asset.asset_id),
        Some(&txid),
        None,
        true,
    );
    check_transfer_status(
        &[
            &wlt_1 as &dyn SigParty,
            &wlt_2 as &dyn SigParty,
            &wlt_3 as &dyn SigParty,
            &singlesig_wlt as &dyn SigParty,
        ],
        &[Some(&uda_asset.asset_id)],
        None,
        TransferStatus::Settled,
    );
    check_asset_balance(&[&wlt_1, &wlt_2, &wlt_3], &uda_asset.asset_id, (1, 1, 1));
    check_asset_balance(&[&singlesig_wlt], &uda_asset.asset_id, (0, 0, 0));

    println!("\n=== send RGB discarded (wlt_1 → singlesig) ===");
    let rcv_data_1 = test_witness_receive(singlesig_wlt.wallet);
    let rcv_data_2 = test_blind_receive(singlesig_wlt.wallet);
    let rcv_data_3 = test_blind_receive(singlesig_wlt.wallet);
    let cfa_amount_witness = AMOUNT_SMALL;
    let cfa_amount_blind = 20;
    let recipient_map = HashMap::from([
        (
            cfa_asset.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(cfa_amount_witness),
                    recipient_id: rcv_data_1.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: 1000,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(cfa_amount_blind),
                    recipient_id: rcv_data_3.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            nia_asset_1.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(AMOUNT_SMALL),
                recipient_id: rcv_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let op_init = wlt_1.send_init(recipient_map);
    operation_complete::<SendRgbHandler>(
        op_init.operation_idx,
        &mut [],
        &mut [&mut wlt_1, &mut wlt_2],
        &mut [&mut wlt_3],
        false,
    );

    println!("\n=== blind receive new asset (singlesig → wlt_1) ===");
    let nia_asset_2 = test_issue_asset_nia(singlesig_wlt.wallet, singlesig_wlt.online, None);
    let receive_data = wlt_1.blind_receive();
    sync_wallets_full(&mut [&mut wlt_2, &mut wlt_3]);
    let recipient_map = HashMap::from([(
        nia_asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(singlesig_wlt.wallet, singlesig_wlt.online, &recipient_map);
    settle_transfer(
        &mut [&mut singlesig_wlt],
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        Some(&nia_asset_2.asset_id),
        Some(&txid),
        None,
        true,
    );
    check_transfer_status(
        &[
            &wlt_1 as &dyn SigParty,
            &wlt_2 as &dyn SigParty,
            &wlt_3 as &dyn SigParty,
            &singlesig_wlt as &dyn SigParty,
        ],
        &[Some(&uda_asset.asset_id)],
        None,
        TransferStatus::Settled,
    );
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &nia_asset_2.asset_id,
        (AMOUNT_SMALL, AMOUNT_SMALL, AMOUNT_SMALL),
    );
    let change = AMOUNT - AMOUNT_SMALL;
    check_asset_balance(
        &[&singlesig_wlt],
        &nia_asset_2.asset_id,
        (change, change, change),
    );

    println!("\n=== send RGB (wlt_1 → singlesig) ===");
    let rcv_data_1 = test_witness_receive(singlesig_wlt.wallet);
    let rcv_data_2 = test_blind_receive(singlesig_wlt.wallet);
    let rcv_data_3 = test_blind_receive(singlesig_wlt.wallet);
    let rcv_data_4 = test_blind_receive(singlesig_wlt.wallet);
    let cfa_amount_witness = AMOUNT_SMALL;
    let cfa_amount_blind = 20;
    let nia_2_amount = 30;
    let recipient_map = HashMap::from([
        (
            cfa_asset.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(cfa_amount_witness),
                    recipient_id: rcv_data_1.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: 1000,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(cfa_amount_blind),
                    recipient_id: rcv_data_3.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            nia_asset_1.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(AMOUNT_SMALL),
                recipient_id: rcv_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            nia_asset_2.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(nia_2_amount),
                recipient_id: rcv_data_4.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let op_init = wlt_1.send_init(recipient_map);
    inspect_send(
        &wlt_2,
        &op_init,
        &cfa_asset,
        &nia_asset_1,
        &nia_asset_2,
        cfa_amount_blind,
        cfa_amount_witness,
        nia_2_amount,
    );
    operation_complete::<SendRgbHandler>(
        op_init.operation_idx,
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [],
        &mut [],
        true,
    );
    settle_transfer(
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [&mut singlesig_wlt],
        None,
        None,
        Some(&op_init.psbt),
        true,
    );
    check_transfer_status(
        &[
            &wlt_1 as &dyn SigParty,
            &wlt_2 as &dyn SigParty,
            &wlt_3 as &dyn SigParty,
            &singlesig_wlt as &dyn SigParty,
        ],
        &[
            Some(&cfa_asset.asset_id),
            Some(&nia_asset_1.asset_id),
            Some(&nia_asset_2.asset_id),
        ],
        None,
        TransferStatus::Settled,
    );
    check_asset_balance(&[&singlesig_wlt], &cfa_asset.asset_id, (86, 86, 66)); // pending receive (20)
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &cfa_asset.asset_id,
        (180, 180, 180),
    );
    check_asset_balance(&[&singlesig_wlt], &nia_asset_1.asset_id, (66, 66, 66));
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &nia_asset_1.asset_id,
        (84, 84, 84),
    );
    check_asset_balance(&[&singlesig_wlt], &nia_asset_2.asset_id, (630, 630, 600)); // pending receive (30)
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &nia_asset_2.asset_id,
        (36, 36, 36),
    );

    println!("\n=== backup (wlt_1) ===");
    // pre-backup state expectations + check
    let op_init_last_before_backup = op_init;
    let btc_pre_backup_vanilla = (7904, 7904, 7904);
    let btc_pre_backup_colored = (15538, 17914, 17914);
    let tx_type_pre_backup = TransactionType::RgbSend;
    #[rustfmt::skip]
    let assets_pre_backup = HashMap::from([
        (cfa_asset.asset_id.as_str(), (180, 180, 180, 3, TransferStatus::Settled)),
        (nia_asset_1.asset_id.as_str(), (84, 84, 84, 2, TransferStatus::Settled)),
        (nia_asset_2.asset_id.as_str(), (36, 36, 36, 2, TransferStatus::Settled)),
        (uda_asset.asset_id.as_str(), (1, 1, 1, 3, TransferStatus::Settled)),
    ]);
    check_wallet_state(
        wlt_1.multisig_mut(),
        &op_init_last_before_backup,
        &op_init_last_before_backup,
        btc_pre_backup_vanilla,
        btc_pre_backup_colored,
        &tx_type_pre_backup,
        &assets_pre_backup,
    );
    // actual backup
    let backup_file = backup(&wlt_1, &format!("{random_str}_1"));

    println!("\n=== send with extra (wlt_1 → singlesig) ===");
    // make sure there are allocations for other assets on the same UTXO that will be spent
    let unspents = wlt_1.list_unspents(false);
    let mut unspents_nia_asset_2 = unspents.iter().filter(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id == Some(nia_asset_2.asset_id.clone()))
    });
    assert!(
        unspents_nia_asset_2
            .next()
            .unwrap()
            .rgb_allocations
            .iter()
            .any(|a| a.asset_id != Some(nia_asset_2.asset_id.clone()))
    );
    assert!(unspents_nia_asset_2.next().is_none());
    // send the assets
    let rcv_data = test_blind_receive(singlesig_wlt.wallet);
    let recipient_map = HashMap::from([(
        nia_asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(10),
            recipient_id: rcv_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let op_init = wlt_1.send_init(recipient_map);
    operation_complete::<SendRgbHandler>(
        op_init.operation_idx,
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [],
        &mut [],
        true,
    );
    settle_transfer(
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [&mut singlesig_wlt],
        Some(&nia_asset_2.asset_id),
        None,
        Some(&op_init.psbt),
        true,
    );
    check_transfer_status(
        &[
            &wlt_1 as &dyn SigParty,
            &wlt_2 as &dyn SigParty,
            &wlt_3 as &dyn SigParty,
            &singlesig_wlt as &dyn SigParty,
        ],
        &[Some(&nia_asset_2.asset_id)],
        None,
        TransferStatus::Settled,
    );
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &cfa_asset.asset_id,
        (180, 180, 180),
    );
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &nia_asset_1.asset_id,
        (84, 84, 84),
    );
    check_asset_balance(&[&singlesig_wlt], &nia_asset_2.asset_id, (640, 640, 610)); // pending receive (30)
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &nia_asset_2.asset_id,
        (26, 26, 26),
    );

    let ifa_amounts = vec![100, 50];
    let IssuedAsset::Ifa(ifa_asset) = issue_asset(
        &mut wlt_1,
        &mut [&mut wlt_2, &mut wlt_3],
        AssetSchema::Ifa,
        Some(&ifa_amounts),
        Some(&[AMOUNT_INFLATION]),
    ) else {
        unreachable!()
    };
    let initial_supply = ifa_amounts.iter().sum::<u64>();
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &ifa_asset.asset_id,
        (initial_supply, initial_supply, initial_supply),
    );

    println!("\n=== inflate (wlt_2) ===");
    check_wallets_up_to_date(&mut [&mut wlt_1, &mut wlt_2, &mut wlt_3]);
    let inflation_amounts = [25, 26];
    let op_init = wlt_2.inflate_init(&ifa_asset.asset_id, &inflation_amounts);
    inspect_inflate(&wlt_3, &op_init, &ifa_asset, &inflation_amounts);
    operation_complete::<InflateHandler>(
        op_init.operation_idx,
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [],
        &mut [],
        true,
    );
    settle_transfer(
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [] as &mut [&mut MultisigParty],
        Some(&ifa_asset.asset_id),
        None,
        Some(&op_init.psbt),
        false,
    );
    let new_supply = initial_supply + inflation_amounts.iter().sum::<u64>();
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &ifa_asset.asset_id,
        (new_supply, new_supply, new_supply),
    );

    println!("\n=== inflate discarded (wlt_3) ===");
    let op_init = wlt_3.inflate_init(&ifa_asset.asset_id, &[1]);
    operation_complete::<InflateHandler>(
        op_init.operation_idx,
        &mut [],
        &mut [&mut wlt_1, &mut wlt_2],
        &mut [&mut wlt_3],
        false,
    );

    println!("\n=== send BTC (wlt_1 → singlesig) ===");
    check_wallets_up_to_date(&mut [&mut wlt_1, &mut wlt_2, &mut wlt_3]);
    let amount = 1000;
    let addr = test_get_address(singlesig_wlt.wallet);
    let op_init = wlt_1.send_btc_init(&addr, amount);
    operation_complete::<SendBtcHandler>(
        op_init.operation_idx,
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [],
        &mut [],
        true,
    );
    check_last_transaction(
        &mut [
            wlt_1.multisig_mut(),
            wlt_2.multisig_mut(),
            wlt_3.multisig_mut(),
        ],
        &op_init.psbt,
        &TransactionType::SendBtc,
    );
    check_btc_balance(
        &mut [
            wlt_1.multisig_mut(),
            wlt_2.multisig_mut(),
            wlt_3.multisig_mut(),
        ],
        (0, 6442, 6442),
        (16452, 16452, 16452),
    );
    let op_init_last_successful = op_init;

    println!("\n=== receive failed (wlt_1) ===");
    let receive_data = wlt_1.blind_receive();
    wlt_1
        .multisig
        .fail_transfers(
            wlt_1.online(),
            Some(receive_data.batch_transfer_idx),
            false,
            false,
        )
        .unwrap();
    sync_wallets_full(&mut [&mut wlt_2, &mut wlt_3]);
    check_transfer_status(
        &[
            &wlt_1 as &dyn SigParty,
            &wlt_2 as &dyn SigParty,
            &wlt_3 as &dyn SigParty,
        ],
        &[None],
        Some(receive_data.batch_transfer_idx),
        TransferStatus::Failed,
    );

    println!("\n=== send failed (wlt_1) ===");
    let receive_data = test_blind_receive(singlesig_wlt.wallet);
    let recipient_map = HashMap::from([(
        nia_asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(10),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let op_init = wlt_1.send_init(recipient_map);
    operation_complete::<SendRgbHandler>(
        op_init.operation_idx,
        &mut [&mut wlt_1, &mut wlt_2, &mut wlt_3],
        &mut [],
        &mut [],
        true,
    );
    check_transfer_status(
        &[
            &wlt_1 as &dyn SigParty,
            &wlt_2 as &dyn SigParty,
            &wlt_3 as &dyn SigParty,
        ],
        &[Some(&nia_asset_2.asset_id)],
        None,
        TransferStatus::WaitingCounterparty,
    );
    let transfers = wlt_1.list_transfers(Some(&nia_asset_2.asset_id));
    let transfer = transfers.last().unwrap();
    wlt_1
        .multisig
        .fail_transfers(
            wlt_1.online(),
            Some(transfer.batch_transfer_idx),
            false,
            false,
        )
        .unwrap();
    for wallet in [&mut wlt_2, &mut wlt_3] {
        wallet.refresh(Some(&nia_asset_2.asset_id));
    }
    check_transfer_status(
        &[
            &wlt_1 as &dyn SigParty,
            &wlt_2 as &dyn SigParty,
            &wlt_3 as &dyn SigParty,
        ],
        &[Some(&nia_asset_2.asset_id)],
        None,
        TransferStatus::Failed,
    );
    check_asset_balance(
        &[&wlt_1, &wlt_2, &wlt_3],
        &nia_asset_2.asset_id,
        (26, 26, 26),
    );

    // final state expectations
    let btc_final_vanilla = (0, 6442, 6442);
    let btc_final_colored = (16452, 16452, 16452);
    let tx_type_final = TransactionType::SendBtc;
    #[rustfmt::skip]
    let assets_final = HashMap::from([
        (cfa_asset.asset_id.as_str(), (180, 180, 180, 3, TransferStatus::Settled)),
        (ifa_asset.asset_id.as_str(), (201, 201, 201, 3, TransferStatus::Settled)),
        (nia_asset_1.asset_id.as_str(), (84, 84, 84, 2, TransferStatus::Settled)),
        (nia_asset_2.asset_id.as_str(), (26, 26, 26, 4, TransferStatus::Failed)),
        (uda_asset.asset_id.as_str(), (1, 1, 1, 3, TransferStatus::Settled)),
    ]);

    println!("\n=== sync wallet 4 (from scratch) ===");
    let last_processed_op = wlt_4
        .multisig
        .get_local_last_processed_operation_idx()
        .unwrap();
    assert_eq!(last_processed_op, last_wlt_4_op);
    sync_wallets_full(&mut [&mut wlt_4]);
    check_wallets_up_to_date(&mut [&mut wlt_4]);
    check_wallet_state(
        wlt_4.multisig_mut(),
        &op_init_last_successful,
        &op_init,
        btc_final_vanilla,
        btc_final_colored,
        &tx_type_final,
        &assets_final,
    );
    check_change_consistency(&mut wlt_1, &mut wlt_4);

    println!("\n=== watch-only sync ===");
    watch_only_wallet_sync(&mut wlt_wo);
    check_wallet_state(
        wlt_wo.multisig_mut(),
        &op_init_last_successful,
        &op_init,
        btc_final_vanilla,
        btc_final_colored,
        &tx_type_final,
        &assets_final,
    );

    println!("\n=== restore backup ===");
    let mut wlt_restored_multisig = backup_restore(&backup_file, &random_str, multisig_wlt_keys);
    let wlt_restored_multisig_online =
        ms_go_online(&mut wlt_restored_multisig, &cosigner_tokens[0]);
    let mut wlt_restored = ms_party!(
        &wlt_1_singlesig,
        &mut wlt_restored_multisig,
        wlt_restored_multisig_online,
        &cosigner_xpubs[0]
    );
    // post-restore checks
    check_wallet_state(
        wlt_restored.multisig_mut(),
        &op_init_last_before_backup,
        &op_init_last_before_backup,
        btc_pre_backup_vanilla,
        btc_pre_backup_colored,
        &tx_type_pre_backup,
        &assets_pre_backup,
    );
    // sync and check it aligns with other cosigners
    wlt_restored.sync_to_head();
    check_wallet_state(
        wlt_restored.multisig_mut(),
        &op_init_last_successful,
        &op_init,
        btc_final_vanilla,
        btc_final_colored,
        &tx_type_final,
        &assets_final,
    );
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn fail() {
    initialize();
    op_counter_reset();

    let bitcoin_network = BitcoinNetwork::Regtest;
    let threshold_colored = 2;
    let threshold_vanilla = 2;
    let random_str: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();
    let data_dir = get_test_data_dir_path()
        .join(format!("{random_str}_1"))
        .to_string_lossy()
        .to_string();
    let _ = fs::create_dir_all(&data_dir);

    // multisig wallet keys
    let wlt_1_keys = generate_keys(bitcoin_network);
    let wlt_2_keys = generate_keys(bitcoin_network);
    let wlt_3_keys = generate_keys(bitcoin_network);

    // cosigners
    let cosigners = vec![
        Cosigner::from_keys(&wlt_1_keys, None),
        Cosigner::from_keys(&wlt_2_keys, None),
        Cosigner::from_keys(&wlt_3_keys, None),
    ];
    let num_cosigners = cosigners.len() as u8;
    let cosigner_xpubs: Vec<String> = cosigners
        .iter()
        .map(|c| c.account_xpub_colored.clone())
        .collect();

    // biscuit token setup
    // - roots
    let root_keypair = KeyPair::new();
    let root_public_key = root_keypair.public();
    // - cosigners
    let mut cosigner_tokens = vec![];
    for cosigner_xpub in &cosigner_xpubs {
        cosigner_tokens.push(create_token(
            &root_keypair,
            Role::Cosigner(cosigner_xpub.clone()),
            None,
        ));
    }
    // - watch-only
    let wo_token = create_token(&root_keypair, Role::WatchOnly, None);

    // hub setup
    write_hub_config(
        &cosigner_xpubs,
        threshold_colored,
        threshold_vanilla,
        root_public_key.to_bytes_hex(),
        None,
    );
    restart_multisig_hub();

    // multisig wallets
    let multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, threshold_vanilla);
    let mut wlt_1_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_1"));
    let wlt_1_multisig_online = ms_go_online(&mut wlt_1_multisig, &cosigner_tokens[0]);
    let mut wlt_2_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_2"));
    let wlt_2_multisig_online = ms_go_online(&mut wlt_2_multisig, &cosigner_tokens[1]);
    let mut wlt_3_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_3"));
    let wlt_3_multisig_online = ms_go_online(&mut wlt_3_multisig, &cosigner_tokens[2]);

    // singlesig wallets (for signing)
    let wlt_1_singlesig = get_test_wallet_with_keys(&wlt_1_keys);
    let wlt_2_singlesig = get_test_wallet_with_keys(&wlt_2_keys);
    let wlt_3_singlesig = get_test_wallet_with_keys(&wlt_3_keys);

    // watch-only wallet
    let mut wlt_wo_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_wo"));
    let wlt_wo_multisig_online = ms_go_online(&mut wlt_wo_multisig, &wo_token);

    // multisig parties
    let mut wlt_1 = ms_party!(
        &wlt_1_singlesig,
        &mut wlt_1_multisig,
        wlt_1_multisig_online,
        &cosigner_xpubs[0]
    );
    let mut wlt_2 = ms_party!(
        &wlt_2_singlesig,
        &mut wlt_2_multisig,
        wlt_2_multisig_online,
        &cosigner_xpubs[1]
    );
    let mut wlt_3 = ms_party!(
        &wlt_3_singlesig,
        &mut wlt_3_multisig,
        wlt_3_multisig_online,
        &cosigner_xpubs[2]
    );

    // watch-only party
    let mut wlt_wo = ms_party!(&mut wlt_wo_multisig, wlt_wo_multisig_online);

    // no cosigners supplied
    let invalid_multisig_wlt_keys = MultisigKeys::new(vec![], threshold_colored, threshold_vanilla);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_eq!(res.err().unwrap(), Error::NoCosignersSupplied);

    // invalid thresholds: higher than total cosigners
    let invalid_threshold = num_cosigners + 1;
    // - colored threshold
    let invalid_multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), invalid_threshold, threshold_vanilla);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidMultisigThreshold { required, total } if *required == invalid_threshold && *total == num_cosigners);
    // - vanilla threshold
    let invalid_multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, invalid_threshold);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidMultisigThreshold { required, total } if *required == invalid_threshold && *total == num_cosigners);

    // invalid thresholds: k=0
    let invalid_threshold = 0;
    // - colored threshold
    let invalid_multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), invalid_threshold, threshold_vanilla);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidMultisigThreshold { required, total } if *required == invalid_threshold && *total == num_cosigners);
    // - vanilla threshold
    let invalid_multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, invalid_threshold);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidMultisigThreshold { required, total } if *required == invalid_threshold && *total == num_cosigners);

    // invalid fingerprint
    let mut invalid_cosigners = cosigners.clone();
    let invalid_fingerprint = s!("invalid");
    invalid_cosigners[1].master_fingerprint = invalid_fingerprint.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidCosigner { details: d } if *d == format!("invalid master_fingerprint '{invalid_fingerprint}'"));

    // invalid xpub content
    let invalid_xpub = s!("invalid");
    // - colored xpub
    let mut invalid_cosigners = cosigners.clone();
    invalid_cosigners[1].account_xpub_colored = invalid_xpub.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidCosigner { details: d } if *d == format!("invalid colored xpub '{invalid_xpub}'"));
    // - vanilla xpub
    let mut invalid_cosigners = cosigners.clone();
    invalid_cosigners[1].account_xpub_vanilla = invalid_xpub.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidCosigner { details: d } if *d == format!("invalid vanilla xpub '{invalid_xpub}'"));

    // invalid xpub network
    let invalid_keys = generate_keys(BitcoinNetwork::Mainnet);
    let invalid_cosigner = Cosigner::from_keys(&invalid_keys, None);
    // - colored xpub
    let mut invalid_cosigners = cosigners.clone();
    invalid_cosigners[1].account_xpub_colored = invalid_cosigner.account_xpub_colored.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidCosigner { details: d } if *d == format!("colored xpub '{}' is for the wrong network", invalid_cosigner.account_xpub_colored));
    // - vanilla xpub
    let mut invalid_cosigners = cosigners.clone();
    invalid_cosigners[1].account_xpub_vanilla = invalid_cosigner.account_xpub_vanilla.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    assert_matches!(res.as_ref().err().unwrap(), Error::InvalidCosigner { details: d } if *d == format!("vanilla xpub '{}' is for the wrong network", invalid_cosigner.account_xpub_vanilla));

    // invalid rgb-lib version
    println!("setting MOCK_LOCAL_VERSION");
    MOCK_LOCAL_VERSION.replace(Some(s!("0.2")));
    let mut wlt_badversion_multisig =
        get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_1"));
    let err = ms_go_online_res(&mut wlt_badversion_multisig, &cosigner_tokens[0]).unwrap_err();
    assert_matches!(err, Error::MultisigHubService { details: d } if d == "rgb-lib version mismatch: local version is 0.2 but hub requires 0.3");

    // expired token
    let mut wlt_badtoken_multisig =
        get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_3"));
    let expired_token = create_token(
        &root_keypair,
        Role::Cosigner(cosigner_xpubs[0].clone()),
        Some(Utc::now() - Duration::from_secs(1)),
    );
    let err = ms_go_online_res(&mut wlt_badtoken_multisig, &expired_token).unwrap_err();
    assert_matches!(err, Error::MultisigHubService { details: d } if d == "Missing or invalid credentials");

    // invalid token
    let invalid_token = s!("invalid");
    let err = ms_go_online_res(&mut wlt_badtoken_multisig, &invalid_token).unwrap_err();
    assert_matches!(err, Error::MultisigHubService { details: d } if d == "Missing or invalid credentials");

    // token for cosigner not in hub config
    let wlt_badtoken_keys = generate_keys(bitcoin_network);
    let invalid_cosigner_token = create_token(
        &root_keypair,
        Role::Cosigner(wlt_badtoken_keys.account_xpub_colored),
        None,
    );
    let err = ms_go_online_res(&mut wlt_badtoken_multisig, &invalid_cosigner_token).unwrap_err();
    assert_matches!(err, Error::MultisigHubService { details: d } if d == "Missing or invalid credentials");

    // token with no xpub nor role
    let invalid_token = biscuit!("")
        .build(&root_keypair)
        .unwrap()
        .to_base64()
        .unwrap();
    let err = ms_go_online_res(&mut wlt_badtoken_multisig, &invalid_token).unwrap_err();
    assert_matches!(err, Error::MultisigHubService { details: d } if d == "Missing or invalid credentials");

    // invalid hub URL
    let err = wlt_badtoken_multisig
        .go_online(
            false,
            ELECTRUM_URL.to_string(),
            s!("invalid"),
            cosigner_tokens[0].to_string(),
        )
        .unwrap_err();
    assert_matches!(err, Error::MultisigHubService { details: d } if d == "URL must be valid and start with http:// or https://");

    // respond with PSBT that has no signatures
    send_sats_to_address(wlt_1.get_address(), Some(10_000));
    mine(false, false);
    let op_init = wlt_1.create_utxos_init(false, None, None, FEE_RATE);
    let op_idx_1 = op_init.operation_idx;
    let unsigned_psbt = op_init.psbt.clone();
    let err = wlt_1
        .respond_to_operation_res(op_idx_1, RespondToOperation::Ack(unsigned_psbt.clone()))
        .unwrap_err();
    assert_matches!(
        err,
        Error::InvalidPsbt { details: d } if d == "PSBT has no signatures"
    );

    // cannot initiate a new operation if another is pending
    let err = wlt_1
        .create_utxos_init_res(false, None, None, FEE_RATE)
        .unwrap_err();
    assert_matches!(err, Error::MultisigOperationInProgress);

    // respond to a non-pending operation
    let signed_psbt = wlt_1_singlesig
        .sign_psbt(unsigned_psbt.clone(), None)
        .unwrap();
    wlt_1.respond_to_operation(op_idx_1, RespondToOperation::Ack(signed_psbt));
    let signed_psbt = wlt_2_singlesig
        .sign_psbt(unsigned_psbt.clone(), None)
        .unwrap();
    wlt_2.multisig_mut().sync_db_txos(false, false).unwrap();
    wlt_2.respond_to_operation(op_idx_1, RespondToOperation::Ack(signed_psbt.clone()));
    let err = wlt_3
        .respond_to_operation_res(op_idx_1, RespondToOperation::Nack)
        .unwrap_err();
    assert_matches!(
        err,
        Error::MultisigCannotRespondToOperation { details: d } if d == "not pending"
    );

    // respond with PSBT for the wrong operation (wrong TXID)
    wlt_1.sync();
    wlt_2.assert_up_to_date();
    let op_init = wlt_1.create_utxos_init(false, Some(5), None, FEE_RATE);
    let op_idx_2 = op_init.operation_idx;
    let err = wlt_1
        .respond_to_operation_res(op_idx_2, RespondToOperation::Ack(signed_psbt.to_string()))
        .unwrap_err();
    assert_matches!(
        err,
        Error::InvalidPsbt { details: d } if d == "PSBT unrelated to operation"
    );

    // respond to already responded
    wlt_1.respond_to_operation(op_idx_2, RespondToOperation::Nack);
    let err = wlt_1
        .respond_to_operation_res(op_idx_2, RespondToOperation::Nack)
        .unwrap_err();
    assert_matches!(
        err,
        Error::MultisigCannotRespondToOperation { details: d } if d == "already responded"
    );

    // respond to an operation that's not the next one
    wlt_2.respond_to_operation(op_idx_2, RespondToOperation::Nack);
    wlt_1.sync();
    wlt_2.assert_up_to_date();
    let op_init = wlt_1.create_utxos_init(false, Some(3), None, FEE_RATE);
    let op_idx_3 = op_init.operation_idx;
    let err = wlt_3
        .respond_to_operation_res(op_idx_3, RespondToOperation::Nack)
        .unwrap_err();
    assert_matches!(
        err,
        Error::MultisigCannotRespondToOperation { details: d } if d == "Cannot respond to operation: operation is not the next one to be processed"
    );

    // watch-only forbidden
    let err = wlt_wo
        .multisig_mut()
        .get_address(wlt_wo_multisig_online)
        .unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.issue_asset_cfa_res(None, None).unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.issue_asset_nia_res(None).unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.issue_asset_ifa_res(None, None, None).unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.issue_asset_uda_res(None, None, vec![]).unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.blind_receive_res().unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.witness_receive_res().unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.nack_res(0).unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo
        .create_utxos_init_res(false, None, None, FEE_RATE)
        .unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.send_btc_init_res("address", AMOUNT).unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.send_init_res(HashMap::new()).unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
    let err = wlt_wo.inflate_init_res("asset_id", &[]).unwrap_err();
    assert_eq!(err, Error::MultisigUserNotCosigner);
}
