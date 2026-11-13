#[cfg(feature = "electrum")]
use super::*;

// a bridged token on anvil and the bridge contract locking it
#[cfg(feature = "electrum")]
fn deploy_token_and_bridge() -> (DeployedContract, DeployedContract) {
    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);
    (token_contract, bridge_contract)
}

// a mint of AMOUNT paying the given (blind) receive
#[cfg(feature = "electrum")]
fn mint_recipient(receive_data: &ReceiveData) -> Recipient {
    Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: None,
        encryption: Encryption::Optional,
    }
}

// lock AMOUNT of the token on the bridge for the mint begun with the result `res`
#[cfg(feature = "electrum")]
fn lock_mint(
    token_contract: &DeployedContract,
    bridge_contract: &DeployedContract,
    res: &MintBeginResult,
) {
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &mint_lock_id(res));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
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
    assert_eq!(asset.known_circulating_supply, 0);
    // the circulating supply the wallet knows of, last read from the bridge contract
    let known_supply = |party: &SinglesigParty| {
        party
            .get_asset_metadata(&asset.asset_id)
            .known_circulating_supply
    };
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
    let recipient = mint_recipient(&receive_data);
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

    // until the mint lock exists on the external chain the consignment cannot be accepted: the
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
    // the lock has to be made for the mint in the TX the minter will broadcast
    assert_eq!(mint.lock_id, mint_lock_id(&res));
    assert_eq!(mint.amount, AMOUNT);
    // the inspected mint commits to the external chain block the lock has to be included before
    assert_eq!(
        mint_metadata,
        vec![TransitionMetadata::BeforeBlock(mint.before_block)]
    );
    assert_eq!(mint.evm_chain_id, ANVIL_CHAIN_ID);
    assert_eq!(
        mint.bridge_contract.to_lowercase(),
        bridge_contract.address.to_lowercase()
    );

    // lock ERC-20 tokens on the MinimalBridge contract via fundsIn
    println!("\nlocking ERC-20 tokens\n");
    lock_mint(&token_contract, &bridge_contract, &res);

    // the lock exists but isn't in a finalized block yet: still pending
    assert!(!party.refresh_all());

    // once the lock is finalized the consignment can be accepted, but only through an endpoint
    // on the bridge's chain: with none configured for it the transfer keeps waiting, without
    // failing the refresh
    evm_mine(EVM_FINALITY_DEPTH);
    party.set_evm_rpc_urls(HashMap::new());
    assert!(!party.refresh_all());
    party.set_evm_rpc_urls(HashMap::from([(ANVIL_CHAIN_ID, EVM_RPC_URL.to_string())]));
    assert_eq!(known_supply(&party), 0);
    println!("\nwaiting for refresh\n");
    party.wait_for_refresh(None);
    // the recipient (here the minter itself) reads the supply once the mint is confirmed
    assert_eq!(known_supply(&party), AMOUNT);

    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let res = party.mint_end(party.online, signed_psbt);
    println!("RES {res:?}");
    assert_eq!(known_supply(&party), AMOUNT);

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
    assert!(mint_transfer.change_utxos.is_empty());

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
    // the recipient of a send reads the supply from the bridge when validating the mint history
    assert_eq!(rcv_asset.known_circulating_supply, AMOUNT);
    assert_eq!(known_supply(&rcv_party), AMOUNT);

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
    assert!(transfer_data.change_utxos.is_empty());

    // release some of the locked tokens from the bridge, which lowers its circulating supply
    let released = 10;
    bridge_funds_out(&bridge_contract.address, &token_contract.deployer, released);

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
        expiration_timestamp: None,
        encryption: Encryption::Optional,
    };
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);

    // lock the bridged amount of ERC-20 tokens on the MinimalBridge contract via fundsIn
    println!("\nlocking ERC-20 tokens\n");
    erc20_approve(&token_contract.address, &bridge_contract.address, amt);
    bridge_funds_in(&bridge_contract.address, amt, &mint_lock_id(&res));
    evm_mine(EVM_FINALITY_DEPTH);
    party_2.wait_for_refresh(None);

    // both the recipient and the minter (at mint_end) read the supply, accounting the release
    let supply = AMOUNT - released + amt;
    assert_eq!(known_supply(&party_2), supply);
    assert_eq!(known_supply(&party), AMOUNT);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let res = party.mint_end(party.online, signed_psbt);
    assert_eq!(known_supply(&party), supply);
    // a wallet that didn't read the bridge since keeps the supply it knew of
    assert_eq!(known_supply(&rcv_party), AMOUNT);

    // the minter lists a mint transfer, the recipient a receive one
    let transfers = party.list_transfers(Some(&asset.asset_id));
    let mint_transfer = transfers.last().unwrap();
    assert_eq!(mint_transfer.kind, TransferKind::Mint);
    assert_eq!(mint_transfer.txid.as_ref().unwrap(), &res.txid);
    assert_eq!(
        mint_transfer.recipient_id.as_ref().unwrap(),
        &receive_data_2.recipient_id
    );
    assert!(mint_transfer.change_utxos.is_empty());
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
        expiration_timestamp: None,
        encryption: Encryption::Optional,
    };
    let result = party.mint_begin_result(party.online, &asset.asset_id, recipient);
    assert_matches!(result, Err(Error::LastMintRight));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn witness_recipient() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

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
        expiration_timestamp: None,
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

    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn revalidate_received_mint() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);

    // without its validated copy, the received mint consignment is validated again, confirming
    // its claim on the bridge's chain
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
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn save_new_asset_mint() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
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
    assert_matches!(result, Err(Error::MintLockNotFound { opid }) if opid == res.details.opid);

    // the lock isn't finalized yet
    lock_mint(&token_contract, &bridge_contract, &res);
    let result = save_new_asset(&rcv_party);
    assert_matches!(result, Err(Error::MintLockNotFound { opid }) if opid == res.details.opid);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn lock_for_other_tx() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let rcv_party_2 = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    let receive_data = rcv_party.blind_receive();
    let res = party.mint_begin(party.online, &asset.asset_id, mint_recipient(&receive_data));
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
    assert!(!rcv_party.refresh_all());
    let mint = rcv_party
        .wallet
        .get_received_mint(receive_data.recipient_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(mint.lock_id, mint_lock_id(&res));

    // locks of the minted amount that don't commit to the TX including the mint, as the operation
    // ID commits to the vout of a witness seal, not to the script it pays, so the same mint in
    // another TX could pay someone else: one for the operation ID alone, one for the operation in
    // another TX
    let opid = OpId::from_str(&res.details.opid).unwrap();
    let other_tx_lock_id = LockId::with(opid, RgbTxid::from_byte_array([1; 32])).to_string();
    for lock_id in [&res.details.opid, &other_tx_lock_id] {
        assert_ne!(*lock_id, mint.lock_id);
        erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
        bridge_funds_in(&bridge_contract.address, AMOUNT, lock_id);
    }
    evm_mine(EVM_FINALITY_DEPTH);

    // they don't confirm the mint, for anyone: its recipient keeps waiting for the lock, it's not
    // valid yet and it cannot be ended
    assert!(!rcv_party.refresh_all());
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    let result =
        rcv_party_2
            .wallet
            .save_new_asset(rcv_party_2.online, consignment.clone(), txid.clone());
    assert_matches!(result, Err(Error::MintLockNotFound { opid }) if opid == res.details.opid);
    let signed_psbt = party.wallet.sign_psbt(res.psbt.clone(), None).unwrap();
    let result = party.mint_end_result(party.online, signed_psbt.clone());
    assert_matches!(result, Err(Error::MintLockNotFound { opid }) if opid == res.details.opid);

    // the lock for the mint in its TX does (saving the asset needs its contract imported)
    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    rcv_party_2.import_contract(&consignment);
    rcv_party_2
        .wallet
        .save_new_asset(rcv_party_2.online, consignment, txid.clone())
        .unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn lock_before_block() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let rcv_party_2 = get_funded_party!();
    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // the lock is made, and finalized, but not before the block the mint commits to
    let receive_data = rcv_party.blind_receive();
    let before_block = evm_block_number() + SHORT_MINT_LOCK_BLOCKS;
    let res = party
        .mint_begin_before_result(
            party.online,
            &asset.asset_id,
            mint_recipient(&receive_data),
            before_block,
        )
        .unwrap();
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
    evm_mine(SHORT_MINT_LOCK_BLOCKS + 2);
    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    // the mint will never be confirmed, for anyone: its recipient refuses it, it's not valid and
    // it cannot be ended
    rcv_party.refresh_all();
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    let result = rcv_party_2
        .wallet
        .save_new_asset(rcv_party_2.online, consignment, txid.clone());
    assert_matches!(result, Err(Error::MintLockExpired { opid, before_block: b }) if opid == res.details.opid && b == before_block);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let result = party.mint_end_result(party.online, signed_psbt);
    assert_matches!(result, Err(Error::MintLockExpired { opid, before_block: b }) if opid == res.details.opid && b == before_block);
    // ending it fails it, releasing its inputs
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));

    // no lock is made and the blocks before the one the mint commits to are all finalized: the
    // mint is refused right away, without waiting for the invoice to expire. A mint has no
    // operation on the sender proxy, so none is NACKed: the refusal doesn't depend on the sender
    // transport endpoint, unreachable here
    let receive_data = rcv_party.blind_receive();
    let before_block = evm_block_number() + SHORT_MINT_LOCK_BLOCKS;
    let res = party
        .wallet
        .mint_begin(
            party.online,
            asset.asset_id.clone(),
            mint_recipient(&receive_data),
            before_block,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            s!("rpc://127.0.0.1:1/json-rpc"),
        )
        .unwrap();
    let txid = Psbt::from_str(&res.psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid()
        .to_string();
    rcv_party.refresh_all();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    evm_mine(SHORT_MINT_LOCK_BLOCKS + EVM_FINALITY_DEPTH);
    rcv_party.refresh_all();
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    // the sender's refresh leaves it pending, failing it fails it, releasing its inputs
    party.refresh_all();
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Initiated));
    assert!(party.fail_transfers_single(res.batch_transfer_idx));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let result = party.mint_end_result(party.online, signed_psbt);
    assert_matches!(result, Err(Error::CannotEndTransfer { txid: t }) if t == txid);

    // failing all expired transfers fails a sent mint once its lock has expired
    let receive_data = rcv_party.blind_receive();
    let res = party
        .mint_begin_before_result(
            party.online,
            &asset.asset_id,
            mint_recipient(&receive_data),
            evm_block_number() + SHORT_MINT_LOCK_BLOCKS,
        )
        .unwrap();
    let txid = Psbt::from_str(&res.psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid()
        .to_string();
    assert!(!party.fail_transfers_all());
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Initiated));
    evm_mine(SHORT_MINT_LOCK_BLOCKS + EVM_FINALITY_DEPTH);
    assert!(party.fail_transfers_all());
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn before_block_too_far() {
    initialize();

    let (_, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // the recipient accepts mints leaving fewer blocks for their lock than the sender does
    let max_mint_lock_blocks = 100;
    rcv_party.set_evm_chains(HashMap::from([(
        ANVIL_CHAIN_ID,
        EvmChain {
            rpc_url: EVM_RPC_URL.to_string(),
            min_mint_lock_blocks: MIN_MINT_LOCK_BLOCKS,
            max_mint_lock_blocks,
        },
    )]));

    // a mint committing to a block further than the recipient accepts is refused when checked
    let receive_data = rcv_party.blind_receive();
    party
        .mint_begin_before_result(
            party.online,
            &asset.asset_id,
            mint_recipient(&receive_data),
            evm_block_number() + max_mint_lock_blocks * 2,
        )
        .unwrap();
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

    // one committing to a block within it is checked
    let receive_data = rcv_party.blind_receive();
    let before_block = evm_block_number() + max_mint_lock_blocks / 2;
    party
        .mint_begin_before_result(
            party.online,
            &asset.asset_id,
            mint_recipient(&receive_data),
            before_block,
        )
        .unwrap();
    rcv_party.refresh_all();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    let received_mint = rcv_party
        .wallet
        .get_received_mint(receive_data.recipient_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(received_mint.before_block, before_block);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn checked_mint_reject_list_not_repeated() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();
    let reject_list_contract = deploy_reject_list();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(
        3,
        bridge_contract.address.clone(),
        Some(reject_list_contract.address.clone()),
    );
    let mint_right_opouts: Vec<_> = party
        .extract_opouts_by_utxo(&asset.asset_id)
        .into_iter()
        .flat_map(|(_, opouts)| opouts)
        .map(|o| (o, true))
        .collect();

    // the recipient checks the mint
    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    rcv_party.refresh_all();
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_some()
    );

    // the reject list is updated to reject the history of the mint (the mint right it spends)
    // after the check, when the lock may have been made already: the checked mint completes
    // whatever the recipient does, so it's not refused
    reject_list_append(&reject_list_contract.address, &mint_right_opouts);
    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn retry_looks_up_mint_lock() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);
    let validations = || RECEIVED_CONSIGNMENT_VALIDATIONS.with_borrow(|n| *n);

    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);

    // checking the mint validates its consignment and marks a backup as required, although the
    // transfer status doesn't change
    let backup_file_path =
        get_test_data_dir_path().join("test_retry_looks_up_mint_lock.rgb-lib_backup");
    let backup_file = backup_file_path.to_str().unwrap();
    let _ = std::fs::remove_file(backup_file);
    rcv_party.wallet.backup(backup_file, PASSWORD).unwrap();
    assert!(!rcv_party.wallet.backup_info().unwrap());
    let validations_before = validations();
    rcv_party.refresh_all();
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_some()
    );
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert_eq!(validations(), validations_before + 1);
    assert!(rcv_party.wallet.backup_info().unwrap());

    // until its lock is finalized, a refresh only looks the lock up
    rcv_party.refresh_all();
    lock_mint(&token_contract, &bridge_contract, &res);
    rcv_party.refresh_all();
    assert_eq!(validations(), validations_before + 1);
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // once it is, the consignment is validated again and accepted
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.refresh_all();
    assert_eq!(validations(), validations_before + 2);
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingBroadcast
    ));
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn locked_mint_not_refused() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();
    let reject_list_contract = deploy_reject_list();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(
        3,
        bridge_contract.address.clone(),
        Some(reject_list_contract.address.clone()),
    );

    // the recipient accepts mints leaving fewer blocks for their lock than the sender does
    rcv_party.set_evm_chains(HashMap::from([(
        ANVIL_CHAIN_ID,
        EvmChain {
            rpc_url: EVM_RPC_URL.to_string(),
            min_mint_lock_blocks: MIN_MINT_LOCK_BLOCKS,
            max_mint_lock_blocks: MINT_LOCK_BLOCKS / 2,
        },
    )]));

    // a mint committing to a block further than the recipient accepts, whose lock is made before
    // the recipient checks it (e.g. for a wallet restored from a backup taken before the check,
    // with a lower bound configured), isn't refused, as it completes anyway
    let receive_data = rcv_party.blind_receive();
    let (res, _) = mint_locked_before_check(
        &mut party,
        mint_recipient(&receive_data),
        &asset.asset_id,
        &token_contract.address,
        &bridge_contract.address,
    );
    rcv_party.refresh_all();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_some()
    );
    settle_locked_mint(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &receive_data.recipient_id,
        res,
    );

    // nor is one whose lock is finalized before the recipient first sees it, the reject list
    // meanwhile rejecting its history (the mint right it spends)
    let mint_right_opouts: Vec<_> = party
        .extract_opouts_by_utxo(&asset.asset_id)
        .into_iter()
        .flat_map(|(_, opouts)| opouts)
        .map(|o| (o, true))
        .collect();
    let receive_data = rcv_party.blind_receive();
    let (res, _) = mint_locked_before_check(
        &mut party,
        mint_recipient(&receive_data),
        &asset.asset_id,
        &token_contract.address,
        &bridge_contract.address,
    );
    reject_list_append(&reject_list_contract.address, &mint_right_opouts);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.refresh_all();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingBroadcast
    ));
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id).settled,
        AMOUNT * 2
    );
}

// a received mint whose lock is finalized, but whose TX the minter spends an input of instead of
// broadcasting it, is failed once the spending TX is confirmed, as the mint can never complete
#[cfg(feature = "electrum")]
fn mint_tx_inputs_spent(rcv_indexer_url: Option<String>) {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party(true, rcv_indexer_url);
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    rcv_party.refresh_all();
    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.refresh_all();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingBroadcast
    ));

    // while the mint TX can still be broadcast, the receive waits for it
    rcv_party.refresh_all();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingBroadcast
    ));
    assert_matches!(
        rcv_party.fail_transfers(Some(receive_data.batch_transfer_idx), false, false),
        Err(Error::CannotFailBatchTransfer)
    );

    // the minter spends an input of the mint TX elsewhere
    let outpoint = Psbt::from_str(&res.psbt).unwrap().unsigned_tx.input[0].previous_output;
    let address = BdkAddress::from_str(&party.get_address()).unwrap();
    let mut tx_builder = party.wallet.bdk_wallet_mut().build_tx();
    tx_builder
        .add_utxo(outpoint)
        .unwrap()
        .manually_selected_only()
        .drain_to(address.assume_checked().script_pubkey())
        .fee_rate(FeeRate::from_sat_per_vb_u32(FEE_RATE as u32));
    let psbt = tx_builder.finish().unwrap();
    let signed_psbt = party.wallet.sign_psbt(psbt.to_string(), None).unwrap();
    let tx = Psbt::from_str(&signed_psbt).unwrap().extract_tx().unwrap();
    party.wallet.broadcast_tx(tx).unwrap();
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Failed,
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn mint_tx_inputs_spent_electrum() {
    mint_tx_inputs_spent(None);
}

#[cfg(feature = "electrum")]
#[cfg(feature = "bitcoind")]
#[test]
#[parallel]
#[serial(evm)]
fn mint_tx_inputs_spent_bitcoind() {
    mint_tx_inputs_spent(Some(BITCOIND_URL.to_string()));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn out_of_band_mint_refused() {
    initialize();

    let (_, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    // a mint paying an out-of-band invoice, its consignment provided out-of-band, is refused
    // before it's checked (the invoice is reserved on the proxy for the minter to post the mint,
    // as rgb-lib only mints to proxy recipients)
    let receive_data = rcv_party.blind_receive_with_endpoints(None, vec![]);
    get_proxy_client(Some(PROXY_URL))
        .prepare_invoice(
            &invoice_id(&receive_data.recipient_id, &receive_data.public_key),
            &receive_data.expiration_timestamp.to_string(),
            &receive_data.public_key,
            None,
        )
        .unwrap();
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    let txid = Psbt::from_str(&res.psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid()
        .to_string();
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid)
        .to_string_lossy()
        .to_string();
    let refreshed = rcv_party
        .wallet
        .provide_out_of_band_consignment(rcv_party.online, consignment_path, vec![])
        .unwrap();
    assert_eq!(
        refreshed
            .get(&receive_data.batch_transfer_idx)
            .unwrap()
            .updated_status,
        Some(TransferStatus::Failed)
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
#[parallel]
#[serial(evm)]
fn checked_mint_utxo_allocations_checked() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    rcv_party.refresh_all();
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_some()
    );
    // other forms of the recipient ID, naming the same files, find no receive
    let other_forms = [
        receive_data.recipient_id.replace(':', "_"),
        format!("./{}", receive_data.recipient_id),
    ];
    for recipient_id in &other_forms {
        assert!(
            rcv_party
                .wallet
                .get_received_mint(recipient_id.clone())
                .unwrap()
                .is_none()
        );
    }

    // the receiving UTXO can no longer take the mint allocation once the lock is made: the
    // checked mint is refused, as accepting it would freeze the UTXO
    MOCK_MAX_UTXO_ASSET_ALLOCATIONS.replace(Some(0));
    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.refresh_all();
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    // there's nothing left to lock for, whatever the form of the recipient ID
    for recipient_id in other_forms
        .into_iter()
        .chain([receive_data.recipient_id.clone()])
    {
        assert!(
            rcv_party
                .wallet
                .get_received_mint(recipient_id)
                .unwrap()
                .is_none()
        );
    }
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn mint_paying_less_than_minted_refused() {
    initialize();

    let (_, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    // a mint keeping part of the amount minted (which its lock has to match) to another output
    // is refused when checked, as the recipient making the lock would pay for it too
    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    MOCK_MINT_KEPT_AMOUNT.replace(Some(AMOUNT));
    party.mint_begin(party.online, &asset.asset_id, recipient);
    assert!(MOCK_MINT_KEPT_AMOUNT.take().is_none());
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
#[parallel]
#[serial(evm)]
fn minted_asset_transfer_acked() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let (res, _) = mint_locked_before_check(
        &mut party,
        recipient,
        &asset.asset_id,
        &token_contract.address,
        &bridge_contract.address,
    );
    rcv_party.refresh_all();
    settle_locked_mint(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &receive_data.recipient_id,
        res,
    );

    // a transfer of the minted asset, which has the mint in its history, isn't a mint: its
    // recipient ACKs it right away, so that its sender broadcasts it at the next refresh
    let receive_data_2 = rcv_party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data_2.public_key.clone()),
            expiration_timestamp: Some(receive_data_2.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid = rcv_party.send_retry(&recipient_map);
    // a single refresh pass, as refresh_all makes another one for a receive waiting for the
    // broadcast, which would ACK it anyway
    rcv_party_2.refresh_result(None, &[]).unwrap();
    let batch_transfer = rcv_party_2
        .db_batch_transfers()
        .into_iter()
        .find(|bt| bt.idx == receive_data_2.batch_transfer_idx)
        .unwrap();
    assert!(batch_transfer.broadcast_deadline.is_some());
    rcv_party.refresh_all();
    assert!(
        rcv_party.check_test_transfer_status_sender(&txid, TransferStatus::WaitingConfirmations)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn concurrent_mints() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

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
            expiration_timestamp: None,
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
        lock_mint(&token_contract, &bridge_contract, res);
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
#[parallel]
#[serial(evm)]
fn mint_end_after_fail() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // begin a mint, lock its amount on the external chain and sign its PSBT
    let mint_begin = |party: &mut SinglesigParty, rcv_party: &mut SinglesigParty| {
        let receive_data = rcv_party.blind_receive();
        let recipient = mint_recipient(&receive_data);
        let res = party.mint_begin(party.online, &asset.asset_id, recipient);
        lock_mint(&token_contract, &bridge_contract, &res);
        evm_mine(EVM_FINALITY_DEPTH);
        let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
        (res.batch_transfer_idx, signed_psbt)
    };

    // a failed mint is not ended: one whose lock has expired, failed by the first mint_end
    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let res = party
        .mint_begin_before_result(
            party.online,
            &asset.asset_id,
            recipient,
            evm_block_number() + SHORT_MINT_LOCK_BLOCKS,
        )
        .unwrap();
    evm_mine(SHORT_MINT_LOCK_BLOCKS + EVM_FINALITY_DEPTH);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let txid = Psbt::from_str(&signed_psbt)
        .unwrap()
        .extract_tx()
        .unwrap()
        .compute_txid()
        .to_string();
    let result = party.mint_end_result(party.online, signed_psbt.clone());
    assert_matches!(result, Err(Error::MintLockExpired { .. }));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));
    let result = party.mint_end_result(party.online, signed_psbt);
    assert_matches!(result, Err(Error::CannotEndTransfer { txid: t }) if t == txid);
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));

    // a settled mint is not ended again
    let (_, signed_psbt) = mint_begin(&mut party, &mut rcv_party);
    let res = party.mint_end(party.online, signed_psbt.clone());
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_transfer_status_sender(None, &res.txid, TransferStatus::Settled);
    let result = party.mint_end_result(party.online, signed_psbt);
    assert_matches!(result, Err(Error::CannotEndTransfer { txid: t }) if t == res.txid);
    assert!(party.check_test_transfer_status_sender(&res.txid, TransferStatus::Settled));
}

// The BDK changes of a mint (the change address revealed by the begin, the TX applied by the end)
// must reach the DB with the operation, so a crash right after it cannot roll them back and lead
// to address reuse.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn bdk_changes_persisted() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

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
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    // the begin revealed the change address and wrote it to the DB
    let reloaded = check_persisted(&party);
    assert_ne!(
        reloaded
            .spk_index()
            .last_revealed_index(KeychainKind::External),
        revealed_before,
    );

    lock_mint(&token_contract, &bridge_contract, &res);
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
#[parallel]
#[serial(evm)]
fn received_mint_amount() {
    initialize();

    let (_, bridge_contract) = deploy_token_and_bridge();

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
            expiration_timestamp: None,
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
#[parallel]
#[serial(evm)]
fn invoice_expiration_ignored() {
    initialize();

    // allow invoices expiring within seconds
    MOCK_CLOCK_SKEW_MARGIN.replace(Some(1));

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // mint to an invoice expiring shortly and let the recipient check the mint
    let expiration = now().unix_timestamp() + 40;
    let receive_data = rcv_party.blind_receive_asset_expiry(None, Some(expiration as u64));
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    let txid = Psbt::from_str(&res.psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid()
        .to_string();
    rcv_party.refresh_all();
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_some()
    );

    // once the mint has been checked, the invoice expiration no longer applies: past it, neither
    // side fails the mint, its only expiration being the block its lock has to be included before
    let secs = std::cmp::max(expiration - now().unix_timestamp() + 2, 0) as u64;
    std::thread::sleep(Duration::from_secs(secs));
    assert!(!rcv_party.fail_transfers_all());
    assert!(!party.fail_transfers_all());
    rcv_party.refresh_all();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Initiated));

    // the lock is then made, and the mint completes past the invoice expiration
    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);
    party.wait_for_transfer_status_sender(Some(&asset.asset_id), &txid, TransferStatus::Settled);
}

// finalize the lock of the mint `res` paying `recipient_id`, end the mint and check it settles
#[cfg(feature = "electrum")]
fn settle_locked_mint(
    party: &mut SinglesigParty,
    rcv_party: &mut SinglesigParty,
    asset_id: &str,
    recipient_id: &str,
    res: MintBeginResult,
) {
    let txid = Psbt::from_str(&res.psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid()
        .to_string();
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(None, recipient_id, TransferStatus::Settled);
    assert_eq!(rcv_party.get_asset_balance(asset_id).settled, AMOUNT);
    party.wait_for_transfer_status_sender(Some(asset_id), &txid, TransferStatus::Settled);
}

// a mint whose lock is made before its recipient checks it (e.g. by a bridge locking right away),
// first checked by the refresh fail_transfers makes on the receive
#[cfg(feature = "electrum")]
fn mint_locked_before_check(
    party: &mut SinglesigParty,
    recipient: Recipient,
    asset_id: &str,
    token_contract: &str,
    bridge_contract: &str,
) -> (MintBeginResult, u64) {
    let res = party.mint_begin(party.online, asset_id, recipient);
    erc20_approve(token_contract, bridge_contract, AMOUNT);
    let lock_from = evm_block_number();
    bridge_funds_in(bridge_contract, AMOUNT, &mint_lock_id(&res));
    (res, lock_from)
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn mint_checked_by_fail_all() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    // a witness invoice (witness receives have no retention grace) expiring shortly
    let expiration = now().unix_timestamp() + 40;
    let receive_data = rcv_party
        .wallet
        .witness_receive(
            Some(rcv_party.online),
            None,
            Assignment::Any,
            expiration as u64,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: Some(WitnessData {
            amount_sat: 1000,
            blinding: None,
        }),
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: None,
        encryption: Encryption::Optional,
    };
    let (res, lock_from) = mint_locked_before_check(
        &mut party,
        recipient,
        &asset.asset_id,
        &token_contract.address,
        &bridge_contract.address,
    );

    // past the invoice expiration, failing the expired transfers checks the mint and doesn't fail it
    let secs = std::cmp::max(expiration - now().unix_timestamp() + 2, 0) as u64;
    std::thread::sleep(Duration::from_secs(secs));
    let failed = rcv_party.fail_transfers_all();
    // the lock is not final, so the refresh cannot settle the receive
    assert!(evm_block_number() < lock_from + EVM_FINALITY_DEPTH);
    assert!(!failed);
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_some()
    );

    // the mint then completes past the invoice expiration
    settle_locked_mint(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &receive_data.recipient_id,
        res,
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn mint_checked_by_fail_single() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    // a blind invoice, which can be failed before its expiration
    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let (res, _) = mint_locked_before_check(
        &mut party,
        recipient,
        &asset.asset_id,
        &token_contract.address,
        &bridge_contract.address,
    );

    // failing the receive checks the mint and is then denied
    let result = rcv_party.fail_transfers(Some(receive_data.batch_transfer_idx), false, false);
    assert_matches!(result, Err(Error::CannotFailBatchTransfer));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_some()
    );

    // the mint then completes
    settle_locked_mint(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &receive_data.recipient_id,
        res,
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
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
        0,
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
    let recipient = mint_recipient(&receive_data);

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
    // - check before_block: it has to leave a block for the lock after the latest one
    let result = party.mint_begin_before_result(
        party.online,
        &asset_bfa.asset_id,
        recipient.clone(),
        evm_block_number() + 1,
    );
    assert_matches!(result, Err(Error::InvalidBeforeBlock { .. }));
    // - check before_block: it has to leave at most the configured blocks for the lock (with a
    //   margin for the blocks mined meanwhile)
    let result = party.mint_begin_before_result(
        party.online,
        &asset_bfa.asset_id,
        recipient.clone(),
        evm_block_number() + MAX_MINT_LOCK_BLOCKS + 10,
    );
    assert_matches!(result, Err(Error::InvalidBeforeBlock { ref details }) if details.contains(&format!("more than {MAX_MINT_LOCK_BLOCKS} blocks")));
    // - check before_block: it has to leave at least the configured blocks for the lock (with a
    //   margin for the blocks mined meanwhile)
    let min_mint_lock_blocks = 20;
    party.set_evm_chains(HashMap::from([(
        ANVIL_CHAIN_ID,
        EvmChain {
            rpc_url: EVM_RPC_URL.to_string(),
            min_mint_lock_blocks,
            max_mint_lock_blocks: MAX_MINT_LOCK_BLOCKS,
        },
    )]));
    let result = party.mint_begin_before_result(
        party.online,
        &asset_bfa.asset_id,
        recipient.clone(),
        evm_block_number() + min_mint_lock_blocks - 10,
    );
    assert_matches!(result, Err(Error::InvalidBeforeBlock { ref details }) if details.contains(&format!("less than {min_mint_lock_blocks} blocks")));
    party.set_evm_rpc_urls(HashMap::from([(ANVIL_CHAIN_ID, EVM_RPC_URL.to_string())]));
    // - check fee_rate
    //   - low
    let result = party.wallet.mint_begin(
        party.online,
        asset_bfa.asset_id.clone(),
        recipient.clone(),
        default_before_block(),
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
        default_before_block(),
        u64::MAX,
        MIN_CONFIRMATIONS,
        PROXY_ENDPOINT.clone(),
    );
    assert_matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER);
    // - recipient with an expiration (a mint expires by the block its lock has to be included
    //   before only)
    let expiring_recipient = Recipient {
        expiration_timestamp: Some(receive_data.expiration_timestamp),
        ..recipient.clone()
    };
    let result = party.mint_begin_result(party.online, &asset_bfa.asset_id, expiring_recipient);
    assert_matches!(result, Err(Error::InvalidRecipientData { details: m }) if m == "a mint recipient cannot have an expiration");

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
            allow_reckless_schemas: false,
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
            allow_reckless_schemas: false,
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
            false,
            None,
            false,
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
    // - no lock on the external chain
    let res = party.mint_begin(party.online, &asset_bfa.asset_id, recipient);
    let expected_opid = res.details.opid.clone();
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    let result = party.mint_end_result(party.online, signed_psbt.clone());
    assert_matches!(result, Err(Error::MintLockNotFound { opid }) if opid == expected_opid);
    // - the pending mint is left as is, so the mint can be ended later, and it can't be failed
    //   until its lock has expired
    assert!(
        party.check_test_transfer_status_sender(
            &Psbt::from_str(&signed_psbt)
                .unwrap()
                .unsigned_tx
                .compute_txid()
                .to_string(),
            TransferStatus::Initiated
        )
    );
    let result =
        party
            .wallet
            .fail_transfers(party.online, Some(res.batch_transfer_idx), false, false);
    assert_matches!(result, Err(Error::CannotFailBatchTransfer));
    assert!(!party.fail_transfers_all());
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
#[parallel]
#[serial(evm)]
fn fail_received() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let asset = party.issue_asset_bfa(2, bridge_contract.address.clone(), None);

    // a receive whose mint hasn't been checked yet is a regular one, failed by fail_transfers
    let receive_data = rcv_party.blind_receive();
    assert!(rcv_party.fail_transfers_single(receive_data.batch_transfer_idx));

    // once checked, a received mint is never failed by fail_transfers, with its lock pending
    // or confirmed (waiting for the broadcast)
    let receive_data = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data);
    let res = party.mint_begin(party.online, &asset.asset_id, recipient);
    rcv_party.refresh_all();
    assert!(
        rcv_party
            .wallet
            .get_received_mint(receive_data.recipient_id.clone())
            .unwrap()
            .is_some()
    );
    let check_not_failed = |rcv_party: &mut SinglesigParty, status: TransferStatus| {
        let result = rcv_party.wallet.fail_transfers(
            rcv_party.online,
            Some(receive_data.batch_transfer_idx),
            false,
            false,
        );
        assert_matches!(result, Err(Error::CannotFailBatchTransfer));
        assert!(!rcv_party.fail_transfers_all());
        assert!(rcv_party.check_test_transfer_status_recipient(&receive_data.recipient_id, status));
    };
    check_not_failed(&mut rcv_party, TransferStatus::WaitingCounterparty);
    lock_mint(&token_contract, &bridge_contract, &res);
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.refresh_all();
    check_not_failed(&mut rcv_party, TransferStatus::WaitingBroadcast);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
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
        assert!(send_transfer.change_utxos.is_empty());
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

    // the received consignment can be inspected, the mint right moved by a propagate transition
    let inspection = inspect_consignment(
        rcv_transfer.consignment_path.clone().unwrap(),
        BundleFilter::All,
    )
    .unwrap();
    assert_eq!(inspection.asset_schema, AssetSchema::Bfa);
    assert_eq!(inspection.genesis.ticker.as_deref(), Some(TICKER));
    assert_eq!(inspection.genesis.name, NAME);
    assert_eq!(inspection.genesis.precision, PRECISION);
    assert_eq!(inspection.witnesses.len(), 1);
    let transitions = &inspection.witnesses[0].operations[0].transitions;
    assert!(!transitions.is_empty());
    assert!(
        transitions
            .iter()
            .all(|t| t.r#type == TypeOfTransition::Propagate)
    );

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
    let recipient = mint_recipient(&receive_data_2);
    let result = party.mint_begin_result(party.online, &asset.asset_id, recipient);
    assert_matches!(result, Err(Error::LastMintRight));

    // the receiver can mint with a received right
    rcv_party.create_utxos_default();
    let receive_data_3 = rcv_party.blind_receive();
    let recipient = mint_recipient(&receive_data_3);
    let res = rcv_party.mint_begin(rcv_party.online, &asset.asset_id, recipient);

    // lock ERC-20 tokens on the MinimalBridge contract via fundsIn
    lock_mint(&token_contract, &bridge_contract, &res);
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
#[parallel]
#[serial(evm)]
fn reject_list() {
    initialize();

    // EVM contracts: the bridged token, its bridge and the reject list
    let (token_contract, bridge_contract) = deploy_token_and_bridge();
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
        expiration_timestamp: None,
        encryption: Encryption::Optional,
    };
    let res = party_1.mint_begin(party_1.online, &asset.asset_id, recipient);
    let mint_opid = res.details.opid.clone();
    erc20_approve(&token_contract.address, &bridge_contract.address, amt);
    bridge_funds_in(&bridge_contract.address, amt, &mint_lock_id(&res));
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
    party_3.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    party_2.wait_for_refresh(None);
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
        expiration_timestamp: None,
        encryption: Encryption::Optional,
    };
    let res = party_1.mint_begin(party_1.online, &asset.asset_id, recipient);
    erc20_approve(&token_contract.address, &bridge_contract.address, amt);
    bridge_funds_in(&bridge_contract.address, amt, &mint_lock_id(&res));
    evm_mine(EVM_FINALITY_DEPTH);
    party_3.wait_for_refresh(None);
    let signed_psbt = party_1.wallet.sign_psbt(res.psbt, None).unwrap();
    party_1.mint_end(party_1.online, signed_psbt);
    mine(false);
    party_3.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    party_1.wait_for_refresh(None);

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
        expiration_timestamp: None,
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
#[parallel]
#[serial(evm)]
fn reject_list_moved_rights() {
    initialize();

    let (token_contract, bridge_contract) = deploy_token_and_bridge();
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
        let recipient = mint_recipient(&receive_data);
        let res = party.mint_begin(party.online, &asset.asset_id, recipient);
        lock_mint(&token_contract, &bridge_contract, &res);
        evm_mine(EVM_FINALITY_DEPTH);
        rcv_party.wait_for_refresh(None);
        let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
        party.mint_end(party.online, signed_psbt);
        mine(false);
        rcv_party.wait_for_transfer_status_recipient(
            None,
            &receive_data.recipient_id,
            TransferStatus::Settled,
        );
        party.wait_for_refresh(None);
    };

    // each mint right is on its own UTXO
    let rights = party.extract_opouts_by_utxo(&asset.asset_id);
    assert_eq!(rights.len(), 4);
    assert!(rights.iter().all(|(_, opouts)| opouts.len() == 1));

    // reject the first 2 mint rights to be selected: the mint retries with the next one and moves
    // the rejected rights, each to its own UTXO, as a UTXO holds at most one mint right
    let rejected_rights = [(rights[0].1[0], true), (rights[1].1[0], true)];
    reject_list_append(&reject_list_contract.address, &rejected_rights);
    evm_mine(EVM_FINALITY_DEPTH);
    mint(&mut party, &mut rcv_party);
    let rights = party.extract_opouts_by_utxo(&asset.asset_id);
    assert_eq!(rights.len(), 3);
    assert!(rights.iter().all(|(_, opouts)| opouts.len() == 1));

    // allow the moved rights again and reject another one: the mint retries with a clean one and
    // moves the rejected one, still leaving a single mint right on each UTXO
    let allowed_rights = rejected_rights.map(|(opout, _)| (opout, false));
    reject_list_append(&reject_list_contract.address, &allowed_rights);
    reject_list_append(&reject_list_contract.address, &[(rights[0].1[0], true)]);
    evm_mine(EVM_FINALITY_DEPTH);
    mint(&mut party, &mut rcv_party);
    let rights = party.extract_opouts_by_utxo(&asset.asset_id);
    assert_eq!(rights.len(), 2);
    assert!(rights.iter().all(|(_, opouts)| opouts.len() == 1));
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id).settled,
        AMOUNT * 2
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
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
    online_options.evm_chains =
        test_evm_chains(HashMap::from([(other_chain_id, EVM_RPC_URL.to_string())]));
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
#[parallel]
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

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn receive_mint_right_on_utxo_with_one() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_noutxo_party!();

    // prepare UTXOs
    rcv_party.create_utxos(false, Some(1), None, FEE_RATE, None);

    // issue
    let asset = party.issue_asset_bfa(4, FAKE_EVM_ADDRESS.to_string(), None);
    let asset_id = asset.asset_id.as_str();

    let mint_right_invoice = |rcv_party: &mut SinglesigParty| {
        rcv_party.wallet.blind_receive(
            Some(rcv_party.online),
            Some(asset_id.to_string()),
            Assignment::MintRight,
            default_rcv_expiration(),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
            None,
        )
    };

    // 2 invoices on the only UTXO of the receiver
    let receive_data_1 = rcv_party.blind_receive();
    let receive_data_2 = rcv_party.blind_receive();

    // a mint right paying the first one is accepted
    let recipient_map = blind_recipient_map(asset_id, &receive_data_1, Assignment::MintRight);
    let txid = party.send_retry(&recipient_map);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(asset_id));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // a mint right paying the second one is refused, as the UTXO is due to receive one
    send_to_blind_refused(
        &mut party,
        &mut rcv_party,
        asset_id,
        &receive_data_2,
        Assignment::MintRight,
    );

    // settle the first one
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data_1.recipient_id,
        TransferStatus::Settled,
    );
    party.wait_for_transfer_status_sender(Some(asset_id), &txid, TransferStatus::Settled);

    // no mint right invoice can be made on the UTXO holding one
    assert_matches!(
        mint_right_invoice(&mut rcv_party),
        Err(Error::InsufficientAllocationSlots)
    );

    // a mint right paying another invoice on it is refused, as the UTXO holds one
    let receive_data_3 = rcv_party.blind_receive();
    send_to_blind_refused(
        &mut party,
        &mut rcv_party,
        asset_id,
        &receive_data_3,
        Assignment::MintRight,
    );
    assert_eq!(
        single_colored_utxo_allocations(&mut rcv_party)
            .into_iter()
            .map(|a| a.assignment)
            .collect::<Vec<_>>(),
        vec![Assignment::MintRight]
    );

    // a mint right invoice goes to a new UTXO, which then cannot get another one
    rcv_party.create_utxos(false, Some(1), None, FEE_RATE, None);
    let receive_data_4 = mint_right_invoice(&mut rcv_party).unwrap();
    assert_matches!(
        mint_right_invoice(&mut rcv_party),
        Err(Error::InsufficientAllocationSlots)
    );
    send_to_blind_and_settle(
        &mut party,
        &mut rcv_party,
        asset_id,
        &receive_data_4,
        Assignment::MintRight,
    );

    // each UTXO of the receiver holds a single mint right
    let unspents = rcv_party.list_unspents(false);
    let mint_right_counts: Vec<usize> = unspents
        .iter()
        .map(|u| {
            u.rgb_allocations
                .iter()
                .filter(|a| a.assignment == Assignment::MintRight)
                .count()
        })
        .filter(|c| *c > 0)
        .collect();
    assert_eq!(mint_right_counts, vec![1, 1]);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn moved_mint_rights_on_separate_utxos() {
    initialize();

    let amount: u64 = 100;

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut nia_party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // prepare UTXOs
    party.create_utxos(false, Some(2), None, FEE_RATE, None);

    // issue a mint right on each UTXO
    let bfa_asset = party.issue_asset_bfa(2, FAKE_EVM_ADDRESS.to_string(), None);
    let bfa_asset_id = bfa_asset.asset_id.as_str();
    let mint_rights_by_utxo = |party: &mut SinglesigParty| -> Vec<usize> {
        party
            .list_unspents(false)
            .iter()
            .map(|u| {
                u.rgb_allocations
                    .iter()
                    .filter(|a| {
                        a.asset_id.as_deref() == Some(bfa_asset_id)
                            && a.assignment == Assignment::MintRight
                    })
                    .count()
            })
            .filter(|c| *c > 0)
            .collect()
    };
    assert_eq!(mint_rights_by_utxo(&mut party), vec![1, 1]);

    // receive an asset on both UTXOs
    let nia_asset = nia_party.issue_asset_nia(Some(&[AMOUNT]));
    let nia_asset_id = nia_asset.asset_id.as_str();
    for _ in 0..2 {
        send_and_settle(
            &mut nia_party,
            &mut party,
            nia_asset_id,
            Assignment::Fungible(amount),
        );
    }
    let unspents = party.list_unspents(false);
    assert!(
        unspents
            .iter()
            .filter(|u| !u.rgb_allocations.is_empty())
            .all(|u| u.rgb_allocations.len() == 2)
    );

    // spending both UTXOs moves both mint rights: one goes to the RGB change, the other to a UTXO
    // without one
    party.create_utxos(false, Some(2), None, FEE_RATE, None);
    send_and_settle(
        &mut party,
        &mut rcv_party,
        nia_asset_id,
        Assignment::Fungible(amount * 2 - AMOUNT_SMALL),
    );
    assert_eq!(mint_rights_by_utxo(&mut party), vec![1, 1]);
    assert_eq!(party.get_asset_balance(nia_asset_id).settled, AMOUNT_SMALL);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn moved_mint_right_no_btc_change() {
    initialize();

    let amount: u64 = 100;
    // tiny UTXOs, so that sending leaves no BTC change and the RGB change goes to an existing UTXO
    let utxo_size: u32 = 500;

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut nia_party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // prepare UTXOs
    party.create_utxos(false, Some(2), Some(utxo_size), FEE_RATE, None);

    // issue a mint right on each UTXO
    let bfa_asset = party.issue_asset_bfa(2, FAKE_EVM_ADDRESS.to_string(), None);
    let bfa_asset_id = bfa_asset.asset_id.as_str();
    let utxo_of = |party: &SinglesigParty, receive_data: &ReceiveData| {
        let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
        party
            .get_test_transfer_data(&transfer)
            .0
            .receive_utxo
            .unwrap()
    };
    let allocations_on = |party: &mut SinglesigParty, utxo: &Outpoint| {
        party
            .list_unspents(false)
            .into_iter()
            .find(|u| u.utxo.outpoint == *utxo)
            .map(|u| u.rgb_allocations)
            .unwrap_or_default()
    };
    let mint_rights_on = |party: &mut SinglesigParty, utxo: &Outpoint| {
        allocations_on(party, utxo)
            .iter()
            .filter(|a| {
                a.asset_id.as_deref() == Some(bfa_asset_id) && a.assignment == Assignment::MintRight
            })
            .count()
    };

    // an asset is received on one of them (S), the other one (M) holding just its mint right
    let nia_asset = nia_party.issue_asset_nia(Some(&[AMOUNT]));
    let nia_asset_id = nia_asset.asset_id.as_str();
    let receive_data_s = party.blind_receive();
    send_to_blind_and_settle(
        &mut nia_party,
        &mut party,
        nia_asset_id,
        &receive_data_s,
        Assignment::Fungible(amount),
    );
    let utxo_s = utxo_of(&party, &receive_data_s);
    let utxo_m = party
        .list_unspents(false)
        .into_iter()
        .find(|u| !u.rgb_allocations.is_empty() && u.utxo.outpoint != utxo_s)
        .unwrap()
        .utxo
        .outpoint;
    assert_eq!(allocations_on(&mut party, &utxo_s).len(), 2);
    assert_eq!(mint_rights_on(&mut party, &utxo_s), 1);
    assert_eq!(allocations_on(&mut party, &utxo_m).len(), 1);
    assert_eq!(mint_rights_on(&mut party, &utxo_m), 1);

    // sending spends S, moving its mint right along, which can't go to the RGB change, on M (the
    // only other UTXO) as it already holds one, and there's no other UTXO for it
    let receive_data = rcv_party.blind_receive();
    let recipient_map = blind_recipient_map(
        nia_asset_id,
        &receive_data,
        Assignment::Fungible(amount / 2),
    );
    let result = party.send_result(&recipient_map);
    assert_matches!(result, Err(Error::InsufficientAllocationSlots));

    // with another UTXO (D), holding more allocations than M (of another asset, so that sending
    // doesn't spend it), the RGB change still goes to M and the mint right goes to D
    party.create_utxos(false, Some(1), Some(utxo_size), FEE_RATE, None);
    let nia_asset_2 = nia_party.issue_asset_nia(Some(&[AMOUNT]));
    let receive_data_d = party.blind_receive();
    MOCK_ACCEPT_MULTIPLE_ALLOCATIONS.replace(true);
    MOCK_SPAM_RECIPIENT_ASSIGNMENTS.replace(Some((1, 2)));
    send_to_blind_and_settle(
        &mut nia_party,
        &mut party,
        &nia_asset_2.asset_id,
        &receive_data_d,
        Assignment::Fungible(amount),
    );
    assert!(MOCK_SPAM_RECIPIENT_ASSIGNMENTS.take().is_none());
    MOCK_ACCEPT_MULTIPLE_ALLOCATIONS.replace(false);
    let utxo_d = utxo_of(&party, &receive_data_d);
    assert_eq!(allocations_on(&mut party, &utxo_d).len(), 2);
    assert_eq!(mint_rights_on(&mut party, &utxo_d), 0);
    let txid = party.send_result(&recipient_map).unwrap().txid;
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(transfer_data.change_utxos, vec![utxo_m.clone()]);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    party.wait_for_transfer_status_sender(None, &txid, TransferStatus::Settled);

    // M got the RGB change, D the mint right, each UTXO holding a single mint right
    assert_eq!(allocations_on(&mut party, &utxo_m).len(), 2);
    assert_eq!(mint_rights_on(&mut party, &utxo_m), 1);
    assert_eq!(allocations_on(&mut party, &utxo_d).len(), 3);
    assert_eq!(mint_rights_on(&mut party, &utxo_d), 1);
    assert!(allocations_on(&mut party, &utxo_s).is_empty());
    assert_eq!(
        party.get_asset_balance(nia_asset_id).settled,
        amount - amount / 2
    );
}

// mint `amount` to a blind receive of `rcv_party`, locking it on the bridge, and settle the mint
#[cfg(feature = "electrum")]
fn mint_and_settle(
    party: &mut SinglesigParty,
    rcv_party: &mut SinglesigParty,
    asset_id: &str,
    token_contract: &DeployedContract,
    bridge_contract: &DeployedContract,
    amount: u64,
) {
    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(amount),
        ..mint_recipient(&receive_data)
    };
    let res = party.mint_begin(party.online, asset_id, recipient);
    let txid = Psbt::from_str(&res.psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid()
        .to_string();
    rcv_party.refresh_all();
    erc20_approve(&token_contract.address, &bridge_contract.address, amount);
    bridge_funds_in(&bridge_contract.address, amount, &mint_lock_id(&res));
    evm_mine(EVM_FINALITY_DEPTH);
    rcv_party.wait_for_refresh(None);
    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();
    party.mint_end(party.online, signed_psbt);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    party.wait_for_transfer_status_sender(Some(asset_id), &txid, TransferStatus::Settled);
}

// a bridge releasing locked tokens without the RGB allocations being burned lets the RGB supply
// exceed u64::MAX: a UTXO receiving a mint that takes it past u64::MAX of the asset would hold
// more than a transition can spend, so the mint is refused
#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn mint_past_u64_on_utxo_refused() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, u64::MAX);
    let bridge_contract = deploy_bridge(&token_contract.address);

    // wallets, the recipient with a single UTXO
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_noutxo_party!();
    rcv_party.create_utxos(false, Some(1), None, FEE_RATE, None);
    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // mint AMOUNT, then release it from the bridge, keeping the RGB allocation
    mint_and_settle(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &token_contract,
        &bridge_contract,
        AMOUNT,
    );
    bridge_funds_out(&bridge_contract.address, &token_contract.deployer, AMOUNT);

    // a mint of u64::MAX to the same UTXO is refused before the lock is made
    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(u64::MAX),
        ..mint_recipient(&receive_data)
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
    assert_eq!(rcv_party.get_asset_balance(&asset.asset_id).settled, AMOUNT);
}

// a bridge releasing locked tokens without the RGB allocations being burned lets the RGB supply
// exceed u64::MAX: the balance is capped and input selection doesn't collect more than a
// transition can spend
#[cfg(feature = "electrum")]
#[test]
#[parallel]
#[serial(evm)]
fn amounts_past_u64() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, u64::MAX);
    let bridge_contract = deploy_bridge(&token_contract.address);

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();
    let asset = party.issue_asset_bfa(3, bridge_contract.address.clone(), None);

    // mint AMOUNT, release it from the bridge keeping the RGB allocation, then mint u64::MAX
    mint_and_settle(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &token_contract,
        &bridge_contract,
        AMOUNT,
    );
    bridge_funds_out(&bridge_contract.address, &token_contract.deployer, AMOUNT);
    mint_and_settle(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &token_contract,
        &bridge_contract,
        u64::MAX,
    );

    // the allocations are on separate UTXOs, their total exceeding u64::MAX
    let amounts: Vec<u64> = rcv_party
        .list_unspents(false)
        .iter()
        .flat_map(|u| u.rgb_allocations.iter())
        .filter(|a| a.asset_id.as_deref() == Some(&asset.asset_id))
        .map(|a| a.assignment.main_amount())
        .collect();
    assert_eq!(amounts.len(), 2);
    assert!(amounts.contains(&AMOUNT) && amounts.contains(&u64::MAX));

    // the balance is capped
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id),
        Balance {
            settled: u64::MAX,
            future: u64::MAX,
            spendable: u64::MAX,
        }
    );

    // sending almost u64::MAX selects only the UTXO holding u64::MAX, as adding the other one
    // would take the inputs past what a transition can spend
    let receive_data = rcv_party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(u64::MAX - 1),
            recipient_id: receive_data.recipient_id.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = rcv_party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    rcv_party_2.wait_for_refresh(None);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party_2.wait_for_refresh(Some(&asset.asset_id));
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    assert_eq!(
        rcv_party_2.get_asset_balance(&asset.asset_id).settled,
        u64::MAX - 1
    );
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id).settled,
        AMOUNT + 1
    );
}
