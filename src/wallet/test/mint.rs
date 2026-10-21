#[cfg(feature = "electrum")]
use super::*;

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn success() {
    initialize();

    // issue an ERC-20 on the EVM chain representing the asset to bridge
    let token_supply = 1_000_000;
    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, token_supply);
    let token_balance = erc20_balance_of(&token_contract.address, &token_contract.deployer);
    assert_eq!(token_balance, token_supply);
    println!("token_contract.address: {}", token_contract.address);

    // deploy the MinimalBridge contract with the ERC-20 as the supported token
    let bridge_contract = deploy_bridge(&token_contract.address);
    println!("bridge_contract.address: {}", bridge_contract.address);

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue with 3 mint rights, as each mint spends one and the last one cannot be spent
    let num_rights = 3;
    let asset = party.issue_asset_bfa(num_rights, bridge_contract.address.clone(), None);
    party.show_unspent_colorings("after issue");
    assert_eq!(asset.initial_supply, 0);
    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);
    let unspents: Vec<Unspent> = party
        .list_unspents(false)
        .into_iter()
        .filter(|u| u.utxo.colorable)
        .collect();
    assert_eq!(unspents.len(), 5);
    println!("unspents: {:?}", unspents);
    let with_allocation = unspents
        .iter()
        .filter(|u| u.rgb_allocations.len() == 1)
        .count();
    let without_allocation = unspents
        .iter()
        .filter(|u| u.rgb_allocations.is_empty())
        .count();
    assert_eq!(with_allocation, num_rights as usize);
    assert_eq!(without_allocation, unspents.len() - num_rights as usize);

    // mint to self
    party.create_utxos_default();
    let receive_data = party.blind_receive();
    let bak_info_before = party.db_backup_info();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    assert_eq!(res.details.opid.len(), 64);
    // check the opid matches the one of the mint transition found via inspection
    let rgb_inspection = party
        .wallet
        .inspect_rgb_transfer(
            res.psbt.clone(),
            res.details.fascia_path.clone(),
            res.details.entropy,
        )
        .unwrap();
    assert_eq!(rgb_inspection.operations.len(), 1);
    assert_eq!(rgb_inspection.operations[0].asset_id, asset.asset_id);
    let mint_transitions: Vec<&RgbTransitionInfo> = rgb_inspection.operations[0]
        .transitions
        .iter()
        .filter(|t| t.r#type == TypeOfTransition::Mint)
        .collect();
    assert_eq!(mint_transitions.len(), 1);
    assert_eq!(mint_transitions[0].opid, res.details.opid);
    let mint_metadata = mint_transitions[0].metadata.clone();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    party.show_unspent_colorings("after mint");

    // until the mint event exists on the external chain the consignment cannot be accepted: the
    // transfer stays pending, to be retried at the next refresh, but the recipient checks the mint
    // right away, assuming the lock will be made, and tells what the lock has to match
    let received_mint = |party: &SinglesigParty| {
        party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
    };
    assert!(received_mint(&party).is_none());
    assert!(!party.refresh_all());
    let mint = received_mint(&party).unwrap();
    assert_eq!(mint.opid, res.details.opid);
    assert_eq!(mint.amount, AMOUNT);
    // the inspected mint commits to the external chain block the recipient expects the lock after
    assert_eq!(
        mint_metadata,
        vec![TransitionMetadata::AfterBlock(mint.after_block)]
    );
    assert_eq!(mint.evm_chain_id, ANVIL_CHAIN_ID);
    assert_eq!(
        mint.bridge_contract.to_lowercase(),
        bridge_contract.address.to_lowercase()
    );

    // lock ERC-20 tokens on the MinimalBridge contract via fundsIn
    println!("\nlocking ERC-20 tokens\n");
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);

    // the event exists but isn't in a finalized block yet: still pending
    assert!(!party.refresh_all());

    // once the event is finalized the consignment can be accepted, but only through an endpoint
    // on the bridge's chain: with none configured for it the transfer keeps waiting, without
    // failing the refresh
    evm_mine(EVM_FINALITY_DEPTH);
    party.set_evm_rpc_urls(HashMap::new());
    assert!(!party.refresh_all());
    party.set_evm_rpc_urls(HashMap::from([(ANVIL_CHAIN_ID, EVM_RPC_URL.to_string())]));
    println!("\nwaiting for refresh\n");
    party.wait_for_refresh(None);

    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let res = party.mint_end(party.online, signed_psbt);
    println!("RES {res:?}");

    println!(
        "list transfers {:?}",
        party.list_transfers(Some(&asset.asset_id))
    );

    // check updated balance
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: AMOUNT,
            spendable: 0,
        }
    );

    mine(false);

    assert!(party.refresh_asset(&asset.asset_id));
    party.show_unspent_colorings("after mint mine + refresh");

    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 3);
    // the blind receive transfer received the bridged amount
    let rcv_transfer = transfers
        .iter()
        .find(|t| t.kind == TransferKind::ReceiveBlind)
        .unwrap();
    assert_eq!(rcv_transfer.status, TransferStatus::Settled);
    assert_eq!(
        rcv_transfer.requested_assignment.as_ref().unwrap(),
        &Assignment::Any
    );
    assert_eq!(rcv_transfer.assignments, vec![Assignment::Fungible(AMOUNT)]);
    assert_eq!(rcv_transfer.txid.as_ref().unwrap(), &res.txid);
    assert!(rcv_transfer.receive_utxo.is_some());
    // the outgoing mint transfer, spending a mint right without allocating a new one
    let mint_transfer = transfers
        .iter()
        .find(|t| t.kind == TransferKind::Mint)
        .unwrap();
    assert_eq!(mint_transfer.status, TransferStatus::Settled);
    assert_eq!(
        mint_transfer.requested_assignment.as_ref().unwrap(),
        &Assignment::Fungible(AMOUNT)
    );
    assert!(mint_transfer.assignments.is_empty());
    assert_eq!(mint_transfer.txid.as_ref().unwrap(), &res.txid);
    assert!(mint_transfer.recipient_id.is_some());
    assert!(mint_transfer.receive_utxo.is_none());
    assert!(mint_transfer.consignment_path.is_some());
    assert!(mint_transfer.change_utxo.is_none());

    // check balance + remaining mint rights
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT,
            spendable: AMOUNT,
        }
    );
    let unspents = party.list_unspents(false);
    let mint_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::MintRight))
    });
    assert_eq!(mint_allocations.count(), num_rights as usize - 1);

    // send
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    party.show_unspent_colorings("after send");
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let tte_data = party.db_transfer_transport_endpoints_data(transfer.idx);
    assert_eq!(tte_data.len(), 1);
    let ce = tte_data.first().unwrap();
    assert_eq!(ce.1.endpoint, PROXY_URL);
    assert!(ce.0.used);

    // transfers progress to status WaitingConfirmations after a refresh (v0.3: the recipient's ACK
    // releases the TX, which it then decrypts and broadcasts)
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _rcv_asset_transfer) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // asset has been received correctly
    let rcv_assets = rcv_party.list_assets(&[]);
    let bfa_assets = rcv_assets.bfa.unwrap();
    assert_eq!(bfa_assets.len(), 1);
    let rcv_asset = bfa_assets.last().unwrap();
    assert_eq!(rcv_asset.asset_id, asset.asset_id);
    assert_eq!(rcv_asset.ticker, TICKER);
    assert_eq!(rcv_asset.name, NAME);
    assert_eq!(rcv_asset.precision, PRECISION);
    assert_eq!(
        rcv_asset.balance,
        Balance {
            settled: 0,
            future: AMOUNT,
            spendable: 0,
        }
    );
    assert_eq!(rcv_asset.initial_supply, 0);

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    party.show_unspent_colorings("after send mine + refresh 2");

    // check balance (no assets left) + remaining mint rights
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );
    let unspents = party.list_unspents(false);
    let mint_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::MintRight))
    });
    assert_eq!(mint_allocations.count(), num_rights as usize - 1);

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    // check there's no change (sent all)
    assert!(transfer_data.change_utxo.is_none());

    // mint to another wallet
    let mut party_2 = get_funded_party!();
    let receive_data_2 = party_2.blind_receive();
    let amt = 42;
    let recipient = Recipient {
        assignment: Assignment::Fungible(amt),
        recipient_id: receive_data_2.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data_2.public_key.clone()),
        expiration_timestamp: Some(receive_data_2.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);

    // lock the bridged amount of ERC-20 tokens on the MinimalBridge contract via fundsIn
    println!("\nlocking ERC-20 tokens\n");
    erc20_approve(&token_contract.address, &bridge_contract.address, amt);
    bridge_funds_in(&bridge_contract.address, amt, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    party_2.wait_for_refresh(None);

    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let res = party.mint_end(party.online, signed_psbt);

    // the minter lists a mint transfer, the recipient a receive one
    let transfers = party.list_transfers(Some(&asset.asset_id));
    let mint_transfer = transfers.last().unwrap();
    assert_eq!(mint_transfer.kind, TransferKind::Mint);
    assert_eq!(mint_transfer.txid.as_ref().unwrap(), &res.txid);
    assert_eq!(
        mint_transfer.recipient_id.as_ref().unwrap(),
        &receive_data_2.recipient_id
    );
    assert!(mint_transfer.change_utxo.is_none());
    let rcv_transfers = party_2.list_transfers(Some(&asset.asset_id));
    assert_eq!(rcv_transfers.len(), 1);
    assert_eq!(rcv_transfers[0].kind, TransferKind::ReceiveBlind);

    // check updated balance
    let balance = party_2.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: amt,
            spendable: 0,
        }
    );
    mine(false);

    assert!(party_2.refresh_asset(&asset.asset_id));
    let balance = party_2.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: amt,
            future: amt,
            spendable: amt,
        }
    );

    // the last mint right cannot be spent, which would leave the wallet unable to mint
    party.wait_for_refresh(Some(&asset.asset_id));
    let unspents = party.list_unspents(false);
    let mint_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::MintRight))
    });
    assert_eq!(mint_allocations.count(), 1);
    let receive_data_3 = party_2.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(amt),
        recipient_id: receive_data_3.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data_3.public_key.clone()),
        expiration_timestamp: Some(receive_data_3.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let result = party.mint_begin_result(party.online, &asset.asset_id, recipient);
    assert_matches!(result, Err(Error::LastMintRight));
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn witness_recipient() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    // mint to a witness invoice: the TX pays the recipient's script, which receives the assets
    let amount_sat = 1000;
    let receive_data = rcv_party.witness_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: Some(WitnessData {
            amount_sat,
            blinding: None,
        }),
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    let psbt = Psbt::from_str(&res.psbt).unwrap();
    let script_pubkey = script_buf_from_recipient_id(receive_data.recipient_id.clone())
        .unwrap()
        .unwrap();
    assert!(
        psbt.unsigned_tx
            .output
            .iter()
            .any(|o| o.script_pubkey == script_pubkey && o.value.to_sat() == amount_sat)
    );

    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_refresh(None);
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Settled
        )
    );
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn revalidate_received_mint() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);

    // without its validated copy, the received mint consignment is validated again, confirming
    // its external anchor on the bridge's chain
    let consignment_path = rcv_party
        .wallet
        .get_receive_consignment_path(&receive_data.recipient_id);
    let valid_consignment_path = rcv_party
        .wallet
        .get_receive_valid_consignment_path(&consignment_path);
    fs::remove_file(&valid_consignment_path).unwrap();

    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_refresh(None);
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Settled
        )
    );
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn save_new_asset_mint_scan_window() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    let txid = Psbt::from_str(&res.psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid()
        .to_string();
    let consignment = RgbTransfer::load_file(
        party
            .wallet
            .get_send_consignment_path(&asset.asset_id, &txid),
    )
    .unwrap();
    // save the asset of the mint consignment, which validates it, taking its (not broadcast)
    // witness from it
    let save_new_asset = |rcv_party: &SinglesigParty| {
        rcv_party
            .wallet
            .save_new_asset(rcv_party.online, consignment.clone(), txid.clone())
    };

    // the lock hasn't been made yet: the mint may still be confirmed
    let result = save_new_asset(&rcv_party);
    assert_matches!(result, Err(Error::MintEventNotFound { opid }) if opid == res.details.opid);

    // the lock is finalized, but outside the scanned blocks: the event follows the block the mint
    // commits to, so it's never within 0 blocks from it, and the mint will never be confirmed
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    MOCK_MAX_MINT_SCAN_BLOCKS.replace(Some(0));
    let result = save_new_asset(&rcv_party);
    MOCK_MAX_MINT_SCAN_BLOCKS.replace(None);
    assert_matches!(result, Err(Error::MintEventOutOfScanWindow { opid, max_scan_blocks: 0 }) if opid == res.details.opid);
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn history_mint_scan_limit() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    // mint to a wallet
    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_refresh(None);
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);

    // send it on, to a wallet scanning no block after the one the mint commits to: the mint in
    // the history is never found within the limit, so the consignment is refused rather than
    // waited for
    let receive_data_2 = rcv_party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data_2.public_key.clone()),
            expiration_timestamp: Some(receive_data_2.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    rcv_party.send_retry(&recipient_map);
    MOCK_MAX_MINT_SCAN_BLOCKS.replace(Some(0));
    rcv_party_2.refresh_all();
    MOCK_MAX_MINT_SCAN_BLOCKS.replace(None);
    assert!(rcv_party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::Failed
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn terminal_mint_scan_limit() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    // the lock is made and finalized, but the recipient scans no block after the one the mint
    // commits to: the mint is refused, as whoever the asset is sent to next would refuse it
    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    MOCK_MAX_MINT_SCAN_BLOCKS.replace(Some(0));
    rcv_party.refresh_all();
    MOCK_MAX_MINT_SCAN_BLOCKS.replace(None);
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_none()
    );
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn concurrent_mints() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();

    // issue with 3 mint rights (the last one cannot be spent), each on a UTXO with not enough
    // bitcoins for a mint
    let num_rights = 3;
    party.create_utxos(
        false,
        Some(num_rights),
        Some(TINY_BTC_AMOUNT),
        FEE_RATE,
        None,
    );
    let asset = party.issue_asset_bfa(num_rights, bridge_contract.address.clone(), None);
    let right_outpoints: HashSet<Outpoint> = party
        .list_unspents(false)
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .map(|u| u.utxo.outpoint)
        .collect();
    assert_eq!(right_outpoints.len(), num_rights as usize);

    // create 2 UTXOs with no allocations, to be used as additional bitcoin inputs
    party.create_utxos(false, Some(2), Some(5000), FEE_RATE, None);

    // begin 2 mints to witness recipients, without ending them
    let mut mints = vec![];
    for _ in 0..2 {
        let receive_data = rcv_party.witness_receive();
        let recipient = Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        };
        let res = party.mint_begin(party.online, &asset.asset_id, recipient);
        let psbt = Psbt::from_str(&res.psbt).unwrap();
        let inputs: HashSet<Outpoint> = psbt
            .unsigned_tx
            .input
            .iter()
            .map(|i| Outpoint::from(i.previous_output))
            .collect();
        // each mint needs an additional bitcoin input
        assert!(inputs.iter().any(|o| !right_outpoints.contains(o)));
        mints.push((receive_data, res, inputs));
    }

    // the 2 pending mints must not spend the same UTXO
    assert!(mints[0].2.is_disjoint(&mints[1].2));

    // lock the bridged amounts on the external chain
    for (_, res, _) in &mints {
        erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
        bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    }
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);

    // both mints can be completed
    for (_, res, _) in &mints {
        let signed_psbt = party.wallet.sign_psbt(res.psbt.clone(), None).unwrap();
        party.mint_end(party.online, signed_psbt);
    }
    assert!(party.db_reserved_txos().is_empty());
    mine(false);
    rcv_party.wait_for_refresh(None);
    for (receive_data, _, _) in &mints {
        assert!(rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Settled
        ));
    }
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id).settled,
        AMOUNT * 2
    );
}

/// A mint that has failed or progressed cannot be ended again, as that would broadcast a failed
/// mint (whose inputs have been released) or bring a progressed one back to waiting for
/// confirmations.
#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn mint_end_after_fail() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // begin a mint, lock its amount on the external chain and sign its PSBT
    let mint_begin = |party: &mut SinglesigParty, rcv_party: &mut SinglesigParty| {
        let receive_data = rcv_party.blind_receive();
        let recipient = Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        };
        let res = party.mint_begin(party.online, &asset.asset_id, recipient);
        erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
        bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
        evm_mine(EVM_FINALITY_DEPTH);
        let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
        (res.batch_transfer_idx, signed_psbt)
    };

    // a failed mint is not ended, even if its lock is on the external chain
    let (batch_transfer_idx, signed_psbt) = mint_begin(&mut party, &mut rcv_party);
    let txid = Psbt::from_str(&signed_psbt)
        .unwrap()
        .extract_tx()
        .unwrap()
        .compute_txid()
        .to_string();
    assert!(party.fail_transfers_single(batch_transfer_idx));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));
    let result = party.mint_end_result(party.online, signed_psbt);
    assert_matches!(result, Err(Error::CannotEndTransfer { txid: t }) if t == txid);
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));

    // a settled mint is not ended again
    let (_, signed_psbt) = mint_begin(&mut party, &mut rcv_party);
    let res = party.mint_end(party.online, signed_psbt.clone());
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    assert!(party.check_test_transfer_status_sender(&res.txid, TransferStatus::Settled));
    let result = party.mint_end_result(party.online, signed_psbt);
    assert_matches!(result, Err(Error::CannotEndTransfer { txid: t }) if t == res.txid);
    assert!(party.check_test_transfer_status_sender(&res.txid, TransferStatus::Settled));
}

// The BDK changes of a mint (the change address revealed by the begin, the TX applied by the end)
// must reach the DB with the operation, so a crash right after it cannot roll them back and lead
// to address reuse.
#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn bdk_changes_persisted() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // the BDK wallet as a restart would load it, from the DB alone
    let check_persisted = |party: &SinglesigParty| {
        let wallet = &party.wallet;
        assert!(wallet.bdk_wallet().staged().is_none());
        assert!(!wallet.get_wallet_dir().join(BDK_PENDING_FILE).exists());
        let reloaded = setup_bdk(
            wallet.database(),
            wallet.get_wallet_dir(),
            wallet.get_descriptors().colored,
            wallet.get_descriptors().vanilla,
            BitcoinNetwork::Regtest,
        )
        .unwrap();
        for keychain in [KeychainKind::External, KeychainKind::Internal] {
            assert_eq!(
                reloaded.spk_index().last_revealed_index(keychain),
                wallet
                    .bdk_wallet()
                    .spk_index()
                    .last_revealed_index(keychain),
            );
        }
        reloaded
    };

    let revealed_before = party
        .wallet
        .bdk_wallet()
        .spk_index()
        .last_revealed_index(KeychainKind::External);
    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    // the begin revealed the change address and wrote it to the DB
    let reloaded = check_persisted(&party);
    assert_ne!(
        reloaded
            .spk_index()
            .last_revealed_index(KeychainKind::External),
        revealed_before,
    );

    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let res = party.mint_end(party.online, signed_psbt);
    // the end wrote the broadcast TX to the DB
    let reloaded = check_persisted(&party);
    let txid: bdk_wallet::bitcoin::Txid = res.txid.parse().unwrap();
    assert!(reloaded.get_tx(txid).is_some());
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn old_after_block() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // the latest finalized block when the mint begins precedes the invoice (by the finality
    // depth): with no grace, the recipient refuses the mint before scanning for its lock
    MOCK_MINT_AFTER_BLOCK_GRACE.replace(Some(0));
    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    party.mint_begin(party.online, &asset.asset_id, recipient);
    rcv_party.refresh_all();
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_none()
    );
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn received_mint_amount() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // invoices requesting an amount
    let mint_to_invoice = |party: &mut SinglesigParty,
                           rcv_party: &mut SinglesigParty,
                           requested: u64,
                           minted: u64| {
        let receive_data = rcv_party
            .wallet
            .blind_receive(
                Some(rcv_party.online),
                None,
                Assignment::Fungible(requested),
                default_rcv_expiration(),
                TRANSPORT_ENDPOINTS.clone(),
                MIN_CONFIRMATIONS,
                None,
            )
            .unwrap();
        let recipient = Recipient {
            assignment: Assignment::Fungible(minted),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        };
        party.mint_begin(party.online, &asset.asset_id, recipient);
        rcv_party.refresh_all();
        receive_data
    };

    // a mint of another amount than the requested one is refused before the lock is made
    let receive_data = mint_to_invoice(&mut party, &mut rcv_party, AMOUNT, AMOUNT + 1);
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_none()
    );

    // a mint of the requested amount is checked and waits for its lock
    let receive_data = mint_to_invoice(&mut party, &mut rcv_party, AMOUNT, AMOUNT);
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    let mint = rcv_party
        .wallet
        .get_received_mint(receive_data.recipient_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(mint.amount, AMOUNT);
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn lock_deadline() {
    initialize();

    // allow mints to invoices expiring within seconds
    MOCK_CLOCK_SKEW_MARGIN.replace(Some(1));
    MOCK_MINT_LOCK_WINDOW.replace(Some(1));

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // mint to an invoice expiring shortly, its expiration being the deadline for the lock, and
    // let the recipient check it
    let deadline_secs = 40;
    let mint_to_expiring_invoice = |party: &mut SinglesigParty,
                                    rcv_party: &mut SinglesigParty|
     -> (MintBeginResult, ReceiveData, i64) {
        let deadline = now().unix_timestamp() + deadline_secs;
        let receive_data = rcv_party.blind_receive_asset_expiry(None, Some(deadline as u64));
        let recipient = Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        };
        let res = party
            .wallet
            .mint_begin(
                party.online,
                asset.asset_id.clone(),
                recipient,
                FEE_RATE,
                MIN_CONFIRMATIONS,
                PROXY_ENDPOINT.clone(),
            )
            .unwrap();
        rcv_party.refresh_all();
        assert!(
            rcv_party
                .wallet
                .get_received_mint(receive_data.recipient_id.clone())
                .unwrap()
                .is_some()
        );
        (res, receive_data, deadline)
    };
    let sleep_past = |deadline: i64| {
        let secs = std::cmp::max(deadline - now().unix_timestamp() + 2, 0) as u64;
        std::thread::sleep(Duration::from_secs(secs));
    };

    // a lock included by the deadline, but finalized after it, is waited for: neither side fails
    // the mint, which is then broadcast past the deadline and completes
    let (res, receive_data, deadline) = mint_to_expiring_invoice(&mut party, &mut rcv_party);
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    sleep_past(deadline);
    assert!(!rcv_party.fail_transfers_all());
    assert!(!party.fail_transfers_all());
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_refresh(None);
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Settled
        )
    );
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);
    // the minter settles the mint
    party.wait_for_refresh(Some(&asset.asset_id));

    // a lock included after the deadline is too late: the mint is not broadcast and both sides
    // fail it
    let (res, receive_data, deadline) = mint_to_expiring_invoice(&mut party, &mut rcv_party);
    sleep_past(deadline);
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let result = party.mint_end_result(party.online, signed_psbt);
    assert_matches!(result, Err(Error::TransferExpired));
    assert!(rcv_party.fail_transfers_all());
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    assert!(party.fail_transfers_all());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // === offline tests

    let mut offline_party = {
        let wallet = get_test_wallet(true, None);
        party!(wallet, Online { id: 0 })
    };
    let offline_recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: s!(""),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(FAKE_PUBKEY.to_string()),
        expiration_timestamp: None,
        encryption: Encryption::Optional,
    };
    let result = offline_party.wallet.mint_begin(
        Online { id: 0 },
        s!(""),
        offline_recipient,
        FEE_RATE,
        MIN_CONFIRMATIONS,
        PROXY_ENDPOINT.clone(),
    );
    assert_matches!(result, Err(Error::Offline));
    let result = offline_party.wallet.mint_end(Online { id: 0 }, s!(""));
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    let mut party = get_funded_party!();

    let asset_bfa = party.issue_asset_bfa(2, FAKE_EVM_ADDRESS.to_string(), None);

    let receive_data = party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };

    // - wrong online
    let wrong_online = Online { id: 1 };

    // mint_begin input params
    // - check online is correct
    let result = party.mint_begin_result(wrong_online, &asset_bfa.asset_id, recipient.clone());
    assert_matches!(result, Err(Error::CannotChangeOnline));
    // - invalid asset_id
    let result = party.mint_begin_result(party.online, "malformed", recipient.clone());
    assert_matches!(result, Err(Error::AssetNotFound { asset_id: _ }));
    // - unsupported assignments
    for assignment in [
        Assignment::NonFungible,
        Assignment::InflationRight(1),
        Assignment::MintRight,
        Assignment::Any,
    ] {
        let mut invalid_recipient = recipient.clone();
        invalid_recipient.assignment = assignment;
        let result = party.mint_begin_result(party.online, &asset_bfa.asset_id, invalid_recipient);
        assert_matches!(result, Err(Error::InvalidAssignment));
    }
    // - zero mint amount
    let mut zero_recipient = recipient.clone();
    zero_recipient.assignment = Assignment::Fungible(0);
    let result = party.mint_begin_result(party.online, &asset_bfa.asset_id, zero_recipient);
    assert_matches!(result, Err(Error::InvalidAmountZero));
    // - check fee_rate
    //   - low
    let result = party.wallet.mint_begin(
        party.online,
        asset_bfa.asset_id.clone(),
        recipient.clone(),
        0,
        MIN_CONFIRMATIONS,
        PROXY_ENDPOINT.clone(),
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW);
    //   - overflow
    let result = party.wallet.mint_begin(
        party.online,
        asset_bfa.asset_id.clone(),
        recipient.clone(),
        u64::MAX,
        MIN_CONFIRMATIONS,
        PROXY_ENDPOINT.clone(),
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER);
    // - invoice leaving too little time for the lock
    let short_expiration = now().unix_timestamp() as u64 + MIN_MINT_LOCK_WINDOW_SECS as u64 - 60;
    let short_receive_data = party.blind_receive_asset_expiry(None, Some(short_expiration));
    let short_recipient = Recipient {
        recipient_id: short_receive_data.recipient_id.clone(),
        public_key: Some(short_receive_data.public_key.clone()),
        expiration_timestamp: Some(short_receive_data.expiration_timestamp),
        ..recipient.clone()
    };
    let result = party.mint_begin_result(party.online, &asset_bfa.asset_id, short_recipient);
    assert_matches!(result, Err(Error::InvoiceExpired));

    // mint_begin errors
    // - inexistent asset
    let result = party.mint_begin_result(party.online, "rgb1nexistent", recipient.clone());
    assert_matches!(result, Err(Error::AssetNotFound { asset_id: _ }));
    // - schema not supported by the wallet
    create_test_data_dir();
    let bitcoin_network = BitcoinNetwork::Regtest;
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);
    let mut wallet_bfa = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![AssetSchema::Nia, AssetSchema::Bfa],
        },
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let online_bfa = wallet_bfa
        .go_online(test_go_online_options(Some(ELECTRUM_URL)))
        .unwrap();
    let mut party_bfa = party!(wallet_bfa, online_bfa);
    fund_wallet(party_bfa.get_address());
    party_bfa.create_utxos_default();
    let asset_bfa_2 = party_bfa.issue_asset_bfa(1, FAKE_EVM_ADDRESS.to_string(), None);
    drop(party_bfa);
    let mut wallet_nia = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![AssetSchema::Nia],
        },
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let online_nia = wallet_nia
        .go_online(test_go_online_options(Some(ELECTRUM_URL)))
        .unwrap();
    let mut party_nia = party!(wallet_nia, online_nia);
    let result =
        party_nia.mint_begin_result(party_nia.online, &asset_bfa_2.asset_id, recipient.clone());
    assert_matches!(result, Err(Error::UnsupportedSchema { asset_schema: _ }));
    // - mint not supported by the asset schema
    party.create_utxos_default();
    for schema in AssetSchema::iter() {
        let supported = match schema {
            AssetSchema::Bfa => true,
            AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa | AssetSchema::Ifa => false,
        };
        if supported {
            continue;
        }
        let asset_id = party.issue_asset_of_schema(schema);
        let result = party.mint_begin_result(party.online, &asset_id, recipient.clone());
        assert_matches!(result, Err(Error::UnsupportedMint { asset_schema }) if asset_schema == schema);
    }

    // mint_end input params
    let address = party.get_address();
    let unsigned_psbt = party
        .wallet
        .send_btc_begin(party.online, address, 1000, FEE_RATE, false, true)
        .unwrap();
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt, None).unwrap();
    // - check online is correct
    let result = party.mint_end_result(wrong_online, signed_psbt.clone());
    assert_matches!(result, Err(Error::CannotChangeOnline));
    // - check signed_psbt is valid
    let result = party.mint_end_result(party.online, s!(""));
    assert_matches!(result, Err(Error::InvalidPsbt { details: _ }));

    // - no EVM RPC endpoint configured for the chain the asset's bridge lives on: a block bound
    //   taken from another chain could make the mint impossible to confirm, so nothing is asked
    //   to an endpoint not configured (and checked) for that chain
    party.create_utxos_default();
    let asset_wrong_chain = party
        .wlt()
        .issue_asset_bfa(
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            2,
            ANVIL_CHAIN_ID + 1,
            FAKE_EVM_ADDRESS.to_string(),
            None,
        )
        .unwrap();
    let result =
        party.mint_begin_result(party.online, &asset_wrong_chain.asset_id, recipient.clone());
    assert_matches!(
        result,
        Err(Error::NoEvmRpcUrl { chain_id }) if chain_id == ANVIL_CHAIN_ID + 1
    );
    // - last mint right
    party.create_utxos_default();
    let asset_single_right = party.issue_asset_bfa(1, FAKE_EVM_ADDRESS.to_string(), None);
    let result = party.mint_begin_result(
        party.online,
        &asset_single_right.asset_id,
        recipient.clone(),
    );
    assert_matches!(result, Err(Error::LastMintRight));

    // mint_end errors
    // - no lock event on the external chain
    let res = party.mint_begin(party.online, &asset_bfa.asset_id, recipient);
    let expected_opid = res.details.opid.clone();
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let result = party.mint_end_result(party.online, signed_psbt.clone());
    assert_matches!(result, Err(Error::MintEventNotFound { opid }) if opid == expected_opid);
    // - expired transfer (its deadline being the invoice expiration): the TX is not broadcast,
    //   since the recipient may have already failed the transfer
    assert!(party.fail_transfers_single(res.batch_transfer_idx));
    // allow mints to invoices expiring within seconds
    MOCK_CLOCK_SKEW_MARGIN.replace(Some(1));
    MOCK_MINT_LOCK_WINDOW.replace(Some(1));
    let expiration_secs: u64 = 5;
    let invoice_expiration = now().unix_timestamp() as u64 + expiration_secs;
    let receive_data = party.blind_receive_asset_expiry(None, Some(invoice_expiration));
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party
        .wallet
        .mint_begin(
            party.online,
            asset_bfa.asset_id.clone(),
            recipient,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            PROXY_ENDPOINT.clone(),
        )
        .unwrap();
    let expired_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(
        expiration_secs * 1000 + 2000,
    ));
    let result = party.mint_end_result(party.online, expired_psbt);
    assert_matches!(result, Err(Error::TransferExpired));
    // the expired mint is failed by the expiration-based cleanup
    assert!(party.fail_transfers_all());
    // - no prior mint_begin
    let psbt_txid = Psbt::from_str(&signed_psbt)
        .unwrap()
        .extract_tx()
        .unwrap()
        .compute_txid()
        .to_string();
    let mut party_2 = get_empty_party!();
    let result = party_2.mint_end_result(party_2.online, signed_psbt);
    assert_matches!(result, Err(Error::UnknownTransfer { txid }) if txid == psbt_txid);
    // - PSBT of another operation
    party.create_utxos(false, Some(2), None, FEE_RATE, None);
    let asset_ifa = party.issue_asset_ifa(None, None, None);
    let unsigned_psbt = party.burn_begin(&asset_ifa.asset_id, 1, None);
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt, None).unwrap();
    let txid = Psbt::from_str(&signed_psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid()
        .to_string();
    let result = party.mint_end_result(party.online, signed_psbt);
    assert_matches!(result, Err(Error::WrongOperation { txid: t }) if t == txid);
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn send_mint_right() {
    initialize();

    // issue an ERC-20 on the EVM chain representing the asset to bridge
    let token_supply = 1_000_000;
    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, token_supply);

    // deploy the MinimalBridge contract with the ERC-20 as the supported token
    let bridge_contract = deploy_bridge(&token_contract.address);

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue with 2 mint rights
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);
    party.show_unspent_colorings("after issue");

    // send 3 mint rights to the receiver, to both blinded and witness recipients: they are
    // duplicated from a single mint right of the sender
    let receive_data = rcv_party.blind_receive();
    let receive_data_b = rcv_party.blind_receive();
    let receive_data_w = rcv_party.witness_receive();
    let mint_right_recipient =
        |receive_data: &ReceiveData, witness_data: Option<WitnessData>| Recipient {
            assignment: Assignment::MintRight,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        };
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            mint_right_recipient(&receive_data, None),
            mint_right_recipient(&receive_data_b, None),
            mint_right_recipient(
                &receive_data_w,
                Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
            ),
        ],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    party.show_unspent_colorings("after send");

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    // the send, with a transfer per recipient
    let send_transfers = |party: &SinglesigParty| -> Vec<Transfer> {
        party
            .list_transfers(Some(&asset.asset_id))
            .into_iter()
            .filter(|t| t.kind == TransferKind::Send && t.txid.as_ref() == Some(&txid))
            .collect()
    };
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    let transfers = send_transfers(&party);
    assert_eq!(transfers.len(), 3);
    assert!(
        transfers
            .iter()
            .all(|t| t.status == TransferStatus::WaitingConfirmations)
    );

    // the asset has been received, with no balance
    let rcv_assets = rcv_party.list_assets(&[]);
    let bfa_assets = rcv_assets.bfa.unwrap();
    assert_eq!(bfa_assets.len(), 1);
    let rcv_asset = bfa_assets.last().unwrap();
    assert_eq!(rcv_asset.asset_id, asset.asset_id);
    assert_eq!(
        rcv_asset.balance,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    // no change: a single mint right has been spent
    for send_transfer in send_transfers(&party) {
        assert_eq!(send_transfer.status, TransferStatus::Settled);
        assert!(send_transfer.change_utxo.is_none());
        assert_eq!(
            send_transfer.requested_assignment,
            Some(Assignment::MintRight)
        );
        assert!(send_transfer.assignments.is_empty());
    }
    let rcv_transfers = rcv_party.list_transfers(Some(&asset.asset_id));
    let rcv_transfer = rcv_transfers
        .iter()
        .find(|t| t.kind == TransferKind::ReceiveBlind)
        .unwrap();
    assert_eq!(rcv_transfer.assignments, vec![Assignment::MintRight]);

    // the receiver now holds the mint rights
    rcv_party.show_unspent_colorings("after receive");
    let unspents = rcv_party.list_unspents(false);
    let mint_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::MintRight))
    });
    assert_eq!(mint_allocations.count(), 3);

    // the sender has a single mint right left, which cannot be spent by a mint
    let unspents = party.list_unspents(false);
    let mint_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::MintRight))
    });
    assert_eq!(mint_allocations.count(), 1);
    let receive_data_2 = party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data_2.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data_2.public_key.clone()),
        expiration_timestamp: Some(receive_data_2.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let result = party.mint_begin_result(party.online, &asset.asset_id, recipient);
    assert_matches!(result, Err(Error::LastMintRight));

    // the receiver can mint with a received right
    rcv_party.create_utxos_default();
    let receive_data_3 = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data_3.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data_3.public_key.clone()),
        expiration_timestamp: Some(receive_data_3.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = rcv_party.mint_begin(rcv_party.online, &asset.asset_id, recipient);

    // lock ERC-20 tokens on the MinimalBridge contract via fundsIn
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);

    let signed_psbt = rcv_party.wallet.sign_psbt(res.psbt, None).unwrap();
    let _res = rcv_party.mint_end(rcv_party.online, signed_psbt);

    // check the minted balance
    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: AMOUNT,
            spendable: 0,
        }
    );
    mine(false);
    assert!(rcv_party.refresh_asset(&asset.asset_id));
    let balance = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT,
            spendable: AMOUNT,
        }
    );

    // the mint spent one of the receiver's mint rights
    let unspents = rcv_party.list_unspents(false);
    let mint_allocations = unspents.iter().flat_map(|u| {
        u.rgb_allocations
            .iter()
            .filter(|a| matches!(a.assignment, Assignment::MintRight))
    });
    assert_eq!(mint_allocations.count(), 2);
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn reject_list() {
    initialize();

    // EVM contracts: the bridged token, its bridge and the reject list
    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);
    let reject_list_contract = deploy_reject_list();

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    // issue committing to the reject list contract, with 4 mint rights: 2 mints spend 2 of them,
    // leaving 2 to check mints against the reject list (the last one cannot be spent anyway)
    let asset = party_1.issue_asset_bfa(
        4,
        bridge_contract.address.clone(),
        Some(reject_list_contract.address.clone()),
    );

    // mint to party 2
    let amt = 60;
    let receive_data = party_2.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(amt),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party_1.mint_begin(party_1.online, &asset.asset_id, recipient);
    let mint_opid = res.details.opid.clone();
    erc20_approve(&token_contract.address, &bridge_contract.address, amt);
    bridge_funds_in(&bridge_contract.address, amt, &mint_opid);
    evm_mine(EVM_FINALITY_DEPTH);
    party_2.wait_for_refresh(None);
    let signed_psbt = party_1.wallet.sign_psbt(res.psbt, None).unwrap();
    let mint_txid = party_1.mint_end(party_1.online, signed_psbt).txid;
    mine(false);
    assert!(party_2.refresh_asset(&asset.asset_id));
    assert_eq!(party_2.get_asset_balance(&asset.asset_id).spendable, amt);

    // reject the minted allocation on the EVM contract
    let opouts = party_2.extract_opouts_from_transfer(&asset.asset_id, &mint_txid);
    assert_eq!(opouts.len(), 1);
    let minted_opout = opouts[0];
    assert_eq!(minted_opout.op.to_string(), mint_opid);
    reject_list_append(&reject_list_contract.address, &[(minted_opout, true)]);

    // fail to send from the rejected allocation
    let receive_data = party_3.blind_receive();
    let mut recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amt),
            recipient_id: receive_data.recipient_id.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party_2.send_result(&recipient_map);
    assert_matches!(
        result,
        Err(Error::InsufficientAssignments { asset_id: ref t, .. }) if t == &asset.asset_id
    );

    // skip the sender check to see that the receiver refuses the transfer
    MOCK_SKIP_BUILD_DAG.replace(Some(()));
    let _txid = party_2.send_retry(&recipient_map);
    party_3.refresh_all();
    assert!(
        party_3.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    party_2.refresh_all();

    // a later entry allowing the allocation supersedes the reject, but only once finalized
    reject_list_append(&reject_list_contract.address, &[(minted_opout, false)]);
    let receive_data = party_3.blind_receive(); // avoid RecipientIDAlreadyUsed
    recipient_map.entry(asset.asset_id.clone()).and_modify(|r| {
        r[0].recipient_id = receive_data.recipient_id.clone();
        r[0].public_key = Some(receive_data.public_key.clone());
    });
    let result = party_2.send_result(&recipient_map);
    assert_matches!(
        result,
        Err(Error::InsufficientAssignments { asset_id: ref t, .. }) if t == &asset.asset_id
    );
    evm_mine(EVM_FINALITY_DEPTH);

    // now the send works
    let _txid = party_2.send_retry(&recipient_map);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    assert!(
        party_3.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Settled
        )
    );
    assert_eq!(party_3.get_asset_balance(&asset.asset_id).settled, amt);

    // a further entry rejecting the minted allocation again supersedes the allow: the received
    // allocation, whose history passes through it, cannot be spent anymore
    reject_list_append(&reject_list_contract.address, &[(minted_opout, true)]);
    let receive_data = party_1.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amt),
            recipient_id: receive_data.recipient_id.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party_3.send_result(&recipient_map);
    assert_matches!(
        result,
        Err(Error::InsufficientAssignments { asset_id: ref t, .. }) if t == &asset.asset_id
    );

    // skip the sender check to see that the receiver refuses the transfer too
    MOCK_SKIP_BUILD_DAG.replace(Some(()));
    let _txid = party_3.send_retry(&recipient_map);
    party_1.refresh_all();
    assert!(
        party_1.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );

    // rejecting the minted allocation leaves the mint rights alone: a new mint is accepted
    let receive_data = party_3.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(amt),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let res = party_1.mint_begin(party_1.online, &asset.asset_id, recipient);
    erc20_approve(&token_contract.address, &bridge_contract.address, amt);
    bridge_funds_in(&bridge_contract.address, amt, &res.details.opid);
    evm_mine(EVM_FINALITY_DEPTH);
    party_3.wait_for_refresh(None);
    let signed_psbt = party_1.wallet.sign_psbt(res.psbt, None).unwrap();
    party_1.mint_end(party_1.online, signed_psbt);
    mine(false);
    party_3.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    assert!(
        party_3.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Settled
        )
    );

    // a mint is checked against the reject list too: with all its mint rights rejected the minter
    // cannot mint anymore, instead of producing a mint its recipient would refuse
    let mint_right_opouts: Vec<_> = party_1
        .extract_opouts_by_utxo(&asset.asset_id)
        .into_iter()
        .flat_map(|(_, opouts)| opouts)
        .map(|o| (o, true))
        .collect();
    assert_eq!(mint_right_opouts.len(), 2);
    reject_list_append(&reject_list_contract.address, &mint_right_opouts);
    let receive_data = party_3.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(amt),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        encryption: Encryption::Optional,
    };
    let result = party_1.mint_begin_result(party_1.online, &asset.asset_id, recipient);
    assert_matches!(
        result,
        Err(Error::InsufficientAssignments { asset_id: ref t, .. }) if t == &asset.asset_id
    );
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn reject_list_multi_right_utxo() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);
    let reject_list_contract = deploy_reject_list();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_bfa(
        4,
        bridge_contract.address.clone(),
        Some(reject_list_contract.address.clone()),
    );

    // mint to the receiver, checking the mint is accepted
    let mint = |party: &mut SinglesigParty, rcv_party: &mut SinglesigParty| {
        let receive_data = rcv_party.blind_receive();
        let recipient = Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        };
        let res = party.mint_begin(party.online, &asset.asset_id, recipient);
        erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
        bridge_funds_in(&bridge_contract.address, AMOUNT, &res.details.opid);
        evm_mine(EVM_FINALITY_DEPTH);
        rcv_party.wait_for_refresh(None);
        let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
        party.mint_end(party.online, signed_psbt);
        mine(false);
        rcv_party.wait_for_refresh(None);
        party.wait_for_refresh(None);
        assert!(rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Settled
        ));
    };

    // each mint right is on its own UTXO
    let rights = party.extract_opouts_by_utxo(&asset.asset_id);
    assert_eq!(rights.len(), 4);
    assert!(rights.iter().all(|(_, opouts)| opouts.len() == 1));

    // reject the first 2 mint rights to be selected: the mint retries with the next one and moves
    // the rejected rights to its change UTXO
    let rejected_rights = [(rights[0].1[0], true), (rights[1].1[0], true)];
    reject_list_append(&reject_list_contract.address, &rejected_rights);
    evm_mine(EVM_FINALITY_DEPTH);
    mint(&mut party, &mut rcv_party);

    // allow the moved rights again, so that 2 clean mint rights share the change UTXO, which comes
    // after the UTXO of the untouched mint right
    let allowed_rights = rejected_rights.map(|(opout, _)| (opout, false));
    reject_list_append(&reject_list_contract.address, &allowed_rights);
    evm_mine(EVM_FINALITY_DEPTH);
    let rights = party.extract_opouts_by_utxo(&asset.asset_id);
    assert_eq!(rights.len(), 2);
    assert_eq!(rights[0].1.len(), 1);
    assert_eq!(rights[1].1.len(), 2);

    // reject the untouched mint right: the retry, needing 2 mint rights to make up for it, selects
    // the shared UTXO too, but the mint still spends a single mint right, moving the other 2
    reject_list_append(&reject_list_contract.address, &[(rights[0].1[0], true)]);
    evm_mine(EVM_FINALITY_DEPTH);
    mint(&mut party, &mut rcv_party);
    let rights = party.extract_opouts_by_utxo(&asset.asset_id);
    assert_eq!(rights.len(), 1);
    assert_eq!(rights[0].1.len(), 2);
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id).settled,
        AMOUNT * 2
    );
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn reject_list_rpc_config() {
    initialize();

    let reject_list_contract = deploy_reject_list();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // sending a mint right is a transfer, checked against the reject list with no mint needed
    let send_mint_right = |party: &mut SinglesigParty,
                           rcv_party: &mut SinglesigParty,
                           asset_id: &str|
     -> (Result<OperationResult, Error>, ReceiveData) {
        let receive_data = rcv_party.blind_receive();
        let recipient_map = HashMap::from([(
            asset_id.to_string(),
            vec![Recipient {
                assignment: Assignment::MintRight,
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                public_key: Some(receive_data.public_key.clone()),
                expiration_timestamp: Some(receive_data.expiration_timestamp),
                encryption: Encryption::Optional,
            }],
        )]);
        (party.send_result(&recipient_map), receive_data)
    };

    // the reject list lives on a chain the wallet has no RPC URL for: the send is refused
    let asset = party.issue_asset_bfa(
        1,
        FAKE_EVM_ADDRESS.to_string(),
        Some(reject_list_contract.address.clone()),
    );
    party.set_evm_rpc_urls(HashMap::new());
    let (result, _) = send_mint_right(&mut party, &mut rcv_party, &asset.asset_id);
    assert_matches!(result, Err(Error::NoEvmRpcUrl { chain_id }) if chain_id == ANVIL_CHAIN_ID);

    // an RPC URL on another chain than the one it's configured for is refused upfront, so no
    // reject list is ever read from the wrong chain
    let other_chain_id = ANVIL_CHAIN_ID + 1;
    let mut online_options = test_go_online_options(None);
    online_options.evm_rpc_urls = HashMap::from([(other_chain_id, EVM_RPC_URL.to_string())]);
    let result = party.wallet.go_online(online_options);
    assert_matches!(
        result,
        Err(Error::EvmChainIdMismatch { expected, actual })
            if expected == other_chain_id && actual == ANVIL_CHAIN_ID
    );

    // skip the sender check: a receiver with no RPC URL for the reject list's chain cannot check
    // the transfer, which stays pending, to be checked once the URL is configured
    party.set_evm_rpc_urls(HashMap::from([(ANVIL_CHAIN_ID, EVM_RPC_URL.to_string())]));
    rcv_party.set_evm_rpc_urls(HashMap::new());
    MOCK_SKIP_BUILD_DAG.replace(Some(()));
    let (result, receive_data) = send_mint_right(&mut party, &mut rcv_party, &asset.asset_id);
    assert!(result.is_ok());
    let res = rcv_party.refresh_result(None, &[]).unwrap();
    assert!(
        res.values()
            .any(|rt| matches!(rt.failure, Some(Error::NoEvmRpcUrl { chain_id }) if chain_id == ANVIL_CHAIN_ID))
    );
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    rcv_party.set_evm_rpc_urls(HashMap::from([(ANVIL_CHAIN_ID, EVM_RPC_URL.to_string())]));
    rcv_party.wait_for_refresh(None);
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[serial(evm)]
fn reject_list_malicious_contract() {
    initialize();

    // let expired blind receives be failed shortly after their invoice expiration has passed: with
    // the transfers left pending on the recipient proxy (neither ACKed nor NACKed), the blinded
    // UTXOs stay reserved until the proxy retention grace has passed too, which is set to none
    let skew_margin: u32 = 5;
    MOCK_CLOCK_SKEW_MARGIN.replace(Some(skew_margin));
    MOCK_RETENTION_GRACE.replace(Some(0));

    // reject list contracts a sender can declare in a BFA contract to attack its receivers: an
    // address with no code and contracts breaking the IRgbRejectList read model
    let mut reject_lists = vec![(
        s!("no code"),
        s!("0x000000000000000000000000000000000000dEaD"),
    )];
    for mode in [
        MaliciousRejectListMode::LengthReverts,
        MaliciousRejectListMode::EntriesReverts,
        MaliciousRejectListMode::ShortPage,
        MaliciousRejectListMode::BadBool,
        MaliciousRejectListMode::LengthOverflow,
        MaliciousRejectListMode::Shrinking,
        MaliciousRejectListMode::HugeLength,
    ] {
        reject_lists.push((
            format!("{mode:?}"),
            deploy_malicious_reject_list(mode).address,
        ));
    }
    // a list is only shorter at the latest block than at the finalized one if it is finalized
    evm_mine(EVM_FINALITY_DEPTH);

    // wallets, with a UTXO to issue and send from (and to receive on) for each reject list
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    party.create_utxos(false, Some(reject_lists.len() as u8), None, FEE_RATE, None);
    rcv_party.create_utxos(false, Some(reject_lists.len() as u8), None, FEE_RATE, None);

    // send a mint right of an asset declaring each reject list, the sender skipping its own check
    // of the list; the invoices all expire at once, sufficiently after the operations (see
    // v1.0-changes-spec.md 6.1)
    let invoice_expiration = now().unix_timestamp() + 90;
    let expiration = (invoice_expiration - 2 * skew_margin as i64) as u64;
    let mut receives = vec![];
    for (name, reject_list) in &reject_lists {
        let asset =
            party.issue_asset_bfa(1, FAKE_EVM_ADDRESS.to_string(), Some(reject_list.clone()));
        let receive_data =
            rcv_party.blind_receive_asset_expiry(None, Some(invoice_expiration as u64));
        let recipient_map = HashMap::from([(
            asset.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::MintRight,
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                public_key: Some(receive_data.public_key.clone()),
                expiration_timestamp: Some(receive_data.expiration_timestamp),
                encryption: Encryption::Optional,
            }],
        )]);
        MOCK_SKIP_BUILD_DAG.replace(Some(()));
        let result = party.wallet.send(
            party.online,
            recipient_map,
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            expiration,
            Some(PROXY_ENDPOINT.clone()),
        );
        assert!(result.is_ok(), "{name}: {result:?}");
        receives.push((name, asset.asset_id, receive_data));
    }

    // the receiver cannot read any of the lists: refreshing returns (instead of hanging), every
    // transfer stays pending instead of being accepted and none of the assets is imported
    let refresh_res = rcv_party.refresh_result(None, &[]).unwrap();
    for (name, asset_id, receive_data) in &receives {
        let refreshed = refresh_res.get(&receive_data.batch_transfer_idx).unwrap();
        assert!(refreshed.updated_status.is_none(), "{name}: {refreshed:?}");
        assert!(
            matches!(refreshed.failure, Some(Error::Network { .. })),
            "{name}: {refreshed:?}"
        );
        assert!(
            rcv_party.check_test_transfer_status_recipient(
                &receive_data.recipient_id,
                TransferStatus::WaitingCounterparty
            ),
            "{name}"
        );
        let result = rcv_party.wallet.get_asset_metadata(asset_id.clone());
        assert!(
            matches!(result, Err(Error::AssetNotFound { .. })),
            "{name}: {result:?}"
        );
    }

    // before expiration the error could be transient, so the transfer cannot be failed yet
    let (name, _, receive_data) = &receives[0];
    let result = rcv_party.fail_transfers(Some(receive_data.batch_transfer_idx), false, false);
    assert!(
        matches!(result, Err(Error::Network { .. })),
        "{name}: {result:?}"
    );

    // once expired the receiver can fail every transfer, which leaves nothing pending
    let secs = invoice_expiration + skew_margin as i64 + 2 - now().unix_timestamp();
    std::thread::sleep(Duration::from_secs(std::cmp::max(secs, 0) as u64));
    assert!(rcv_party.fail_transfers_all());
    for (name, _, receive_data) in &receives {
        assert!(
            rcv_party.check_test_transfer_status_recipient(
                &receive_data.recipient_id,
                TransferStatus::Failed
            ),
            "{name}"
        );
    }
    assert!(!rcv_party.fail_transfers_all());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn many_rights_on_utxo() {
    // a UTXO can receive more mint rights than an issuance can create (u8::MAX)
    let num_rights = u8::MAX as u64 + 2;
    let opid = OpId::from([0u8; 32]);
    let mut from_opouts = AssignmentsCollection::default();
    let mut from_allocations = AssignmentsCollection::default();
    for no in 0..num_rights {
        let opout = Opout::new(opid, OS_MINT, no as u16);
        from_opouts.add_opout_state(&opout, &AllocatedState::Void);
        Assignment::MintRight.add_to_assignments(&mut from_allocations);
    }
    assert_eq!(from_opouts.mint, num_rights);
    assert_eq!(from_allocations.mint, num_rights);

    let needed = AssignmentsCollection {
        mint: 1,
        ..Default::default()
    };
    assert!(from_opouts.enough(&needed));
    assert_eq!(from_opouts.change(&needed).mint, num_rights - 1);
}
