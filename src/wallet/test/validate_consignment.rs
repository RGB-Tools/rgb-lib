use super::*;

fn validate(
    consignment_path: &str,
    offchain_txid: Option<&str>,
    min_confirmations: Option<u8>,
) -> Result<ConsignmentValidation, Error> {
    validate_consignment(
        consignment_path,
        &IndexerConfig::Electrum {
            url: ELECTRUM_URL.to_string(),
        },
        &HashMap::new(),
        min_confirmations,
        &validation_options(offchain_txid),
    )
}

fn validation_options(offchain_txid: Option<&str>) -> ValidationOptions {
    ValidationOptions {
        bitcoin_network: BitcoinNetwork::Regtest,
        offchain_txid: offchain_txid.map(str::to_string),
    }
}

#[test]
#[parallel]
fn success() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid = party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid)
        .to_string_lossy()
        .to_string();
    let consignment = RgbTransfer::load_file(&consignment_path).unwrap();

    // the witness hasn't been broadcast yet, so it needs to be taken from the consignment
    let result = validate(&consignment_path, None, None);
    assert_matches!(result, Err(Error::ConsignmentValidationFailed { .. }));
    let validation = validate(&consignment_path, Some(&txid), None).unwrap();
    assert_eq!(
        validation.consignment_id,
        consignment.consignment_id().to_string()
    );
    assert!(validation.is_transfer);
    assert_eq!(validation.asset_id, asset.asset_id);
    assert_eq!(validation.asset_schema, AssetSchema::Nia);
    // without a confirmation policy the unconfirmed witness isn't reported
    assert_eq!(validation.safe_height, None);
    assert_eq!(
        validation.witnesses,
        vec![ValidatedWitness {
            txid: txid.clone(),
            status: WitnessValidationStatus::Tentative,
            from_consignment: true,
        }]
    );
    assert!(validation.warnings.is_empty());

    // an unconfirmed witness is reported when confirmations are requested
    let validation = validate(&consignment_path, Some(&txid), Some(1)).unwrap();
    assert!(validation.safe_height.is_some());
    assert_eq!(
        validation.warnings,
        vec![ValidationWarning::UnsafeHistory {
            txids: vec![txid.clone()]
        }]
    );

    // unconfirmed witnesses are reported whatever the requested confirmations
    let validation = validate(&consignment_path, Some(&txid), Some(0)).unwrap();
    assert_eq!(
        validation.warnings,
        vec![ValidationWarning::UnsafeHistory {
            txids: vec![txid.clone()]
        }]
    );

    // the contract consignment carries the genesis only
    let contract_file = tempfile::NamedTempFile::with_prefix("validate_consignment::").unwrap();
    consignment
        .clone()
        .into_contract()
        .save_file(contract_file.path())
        .unwrap();
    let validation = validate(&contract_file.path().to_string_lossy(), None, Some(1)).unwrap();
    assert!(!validation.is_transfer);
    assert_eq!(validation.asset_id, asset.asset_id);
    assert!(validation.witnesses.is_empty());
    assert!(validation.warnings.is_empty());
    // it has no witness, so no offchain TXID can match one
    let contract_path = contract_file.path().to_string_lossy().to_string();
    let result = validate(&contract_path, Some(&txid), None);
    assert_matches!(result, Err(Error::OffchainTxidNotFound { txid: t }) if t == txid);

    // a contract consignment carrying bundles is invalid, so no offchain TXID can be of use
    let mut contract = consignment.clone().into_contract();
    contract.bundles = consignment.bundles.clone();
    contract.save_file(contract_file.path()).unwrap();
    let result = validate(&contract_path, None, None);
    assert_matches!(result, Err(Error::ConsignmentValidationFailed { .. }));

    // settle the transfer, the witness then being resolved by the indexer
    rcv_party.wait_for_refresh(None);

    // the validated consignment kept by the recipient is not a consignment to be validated
    let rcv_consignment_path = rcv_party
        .wallet
        .get_receive_consignment_path(&receive_data.recipient_id);
    let valid_consignment_path = rcv_party
        .wallet
        .get_receive_valid_consignment_path(&rcv_consignment_path);
    assert!(valid_consignment_path.exists());
    let result = validate(&valid_consignment_path.to_string_lossy(), None, None);
    assert_matches!(result, Err(Error::InvalidConsignment));

    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    let validation = validate(&consignment_path, None, Some(1)).unwrap();
    let safe_height = validation.safe_height.unwrap();
    assert_eq!(validation.witnesses.len(), 1);
    let witness = &validation.witnesses[0];
    assert_eq!(witness.txid, txid);
    assert!(!witness.from_consignment);
    assert_matches!(witness.status, WitnessValidationStatus::Mined { height, .. } if height <= safe_height);
    assert!(validation.warnings.is_empty());

    // a mined witness is safe with 0 confirmations requested
    let validation = validate(&consignment_path, None, Some(0)).unwrap();
    assert!(validation.warnings.is_empty());

    // the other indexers resolve the witness the same way
    let indexer_configs: Vec<IndexerConfig> = vec![
        #[cfg(feature = "esplora")]
        IndexerConfig::Esplora {
            url: ESPLORA_URL.to_string(),
        },
        #[cfg(feature = "bitcoind")]
        IndexerConfig::Bitcoind {
            url: BITCOIND_URL.to_string(),
            auth: bitcoind_pass_auth(),
        },
    ];
    for indexer_config in indexer_configs {
        let other_validation = validate_consignment(
            &consignment_path,
            &indexer_config,
            &HashMap::new(),
            None,
            &validation_options(None),
        )
        .unwrap();
        assert_eq!(other_validation.witnesses, validation.witnesses);
        assert!(other_validation.warnings.is_empty());
    }
}

#[test]
#[parallel]
fn fail() {
    initialize();

    // inexistent file
    let result = validate("inexistent", None, None);
    assert_matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == "inexistent");

    // not a consignment
    let invalid_file = tempfile::NamedTempFile::with_prefix("validate_consignment::").unwrap();
    let invalid_path = invalid_file.path().to_string_lossy().to_string();
    fs::write(&invalid_file, b"not a consignment").unwrap();
    let result = validate(&invalid_path, None, None);
    assert_matches!(result, Err(Error::InvalidConsignment));

    // a schema definition
    AssetSchema::Nia
        .schema_definition()
        .save_file(invalid_file.path())
        .unwrap();
    let result = validate(&invalid_path, None, None);
    assert_matches!(result, Err(Error::InvalidConsignment));

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid = party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid)
        .to_string_lossy()
        .to_string();

    // invalid TXID
    let result = validate(&consignment_path, Some("invalid"), None);
    assert_matches!(result, Err(Error::InvalidTxid));

    // TXID not of a witness in the consignment
    let unknown_txid = "0".repeat(64);
    let result = validate(&consignment_path, Some(&unknown_txid), None);
    assert_matches!(result, Err(Error::OffchainTxidNotFound { txid: t }) if t == unknown_txid);

    // indexer for another network
    let result = validate_consignment(
        &consignment_path,
        &IndexerConfig::Electrum {
            url: ELECTRUM_URL.to_string(),
        },
        &HashMap::new(),
        None,
        &ValidationOptions {
            bitcoin_network: BitcoinNetwork::Testnet,
            ..validation_options(Some(&txid))
        },
    );
    assert_matches!(result, Err(Error::InvalidIndexer { .. }));

    // a transition not committed by its bundle
    let mut consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    let witness_id = consignment.bundled_witnesses().next().unwrap().witness_id();
    assert!(consignment.modify_bundle(witness_id, |witness_bundle| {
        let bundle = witness_bundle.bundle_mut();
        let mut known_transitions = bundle.known_transitions.to_unconfined();
        known_transitions[0].transition.nonce =
            known_transitions[0].transition.nonce.wrapping_add(1);
        bundle.known_transitions = Confined::from_iter_checked(known_transitions);
    }));
    consignment.save_file(invalid_file.path()).unwrap();
    let result = validate(&invalid_path, Some(&txid), None);
    assert_matches!(result, Err(Error::ConsignmentValidationFailed { .. }));
}

#[test]
#[parallel]
fn with_resolver() {
    initialize();

    struct MockResolver {
        witness: Option<(BdkTransaction, WitnessOrd)>,
        chain_net_ok: bool,
    }
    impl ResolveWitness for MockResolver {
        fn resolve_witness(&self, _: RgbTxid) -> Result<WitnessStatus, WitnessResolverError> {
            Ok(match &self.witness {
                Some((tx, ord)) => WitnessStatus::Resolved(tx.clone(), *ord),
                None => WitnessStatus::Unresolved,
            })
        }
        fn check_chain_net(&self, _: ChainNet) -> Result<(), WitnessResolverError> {
            if self.chain_net_ok {
                Ok(())
            } else {
                Err(WitnessResolverError::WrongChainNet)
            }
        }
    }

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid = party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid)
        .to_string_lossy()
        .to_string();
    let consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    let witness_tx = consignment.bundled_witnesses().next().unwrap().tx.clone();
    let validate_with =
        |resolver: &MockResolver, offchain_txid: Option<&str>, safe_height: Option<u32>| {
            validate_consignment_with_resolver(
                &consignment_path,
                resolver,
                None,
                None,
                safe_height.and_then(NonZeroU32::new),
                &validation_options(offchain_txid),
            )
        };

    // a witness unknown to the resolver needs to be taken from the consignment
    let resolver = MockResolver {
        witness: None,
        chain_net_ok: true,
    };
    let result = validate_with(&resolver, None, None);
    assert_matches!(result, Err(Error::ConsignmentValidationFailed { .. }));
    let validation = validate_with(&resolver, Some(&txid), None).unwrap();
    assert!(validation.is_transfer);
    assert_eq!(validation.asset_id, asset.asset_id);
    assert_eq!(validation.safe_height, None);
    assert_eq!(
        validation.witnesses,
        vec![ValidatedWitness {
            txid: txid.clone(),
            status: WitnessValidationStatus::Tentative,
            from_consignment: true,
        }]
    );
    assert!(validation.warnings.is_empty());

    // a witness mined above the safe height is reported
    let pos = WitnessPos::bitcoin(NonZeroU32::new(100).unwrap(), now().unix_timestamp()).unwrap();
    let resolver = MockResolver {
        witness: Some((witness_tx, WitnessOrd::Mined(pos))),
        chain_net_ok: true,
    };
    let validation = validate_with(&resolver, None, Some(200)).unwrap();
    assert_eq!(validation.safe_height, Some(200));
    assert_eq!(
        validation.witnesses,
        vec![ValidatedWitness {
            txid: txid.clone(),
            status: WitnessValidationStatus::Mined {
                height: 100,
                timestamp: pos.timestamp(),
            },
            from_consignment: false,
        }]
    );
    assert!(validation.warnings.is_empty());
    let validation = validate_with(&resolver, None, Some(100)).unwrap();
    assert!(validation.warnings.is_empty());
    let validation = validate_with(&resolver, None, Some(99)).unwrap();
    assert_eq!(
        validation.warnings,
        vec![ValidationWarning::UnsafeHistory {
            txids: vec![txid.clone()]
        }]
    );

    // a consignment for another network than the provided one, even with a resolver for it
    let resolver = MockResolver {
        witness: None,
        chain_net_ok: true,
    };
    let result = validate_consignment_with_resolver(
        &consignment_path,
        &resolver,
        None,
        None,
        None,
        &ValidationOptions {
            bitcoin_network: BitcoinNetwork::Testnet,
            ..validation_options(Some(&txid))
        },
    );
    assert_matches!(result, Err(Error::ConsignmentValidationFailed { .. }));

    // a resolver for another network
    let resolver = MockResolver {
        witness: None,
        chain_net_ok: false,
    };
    let result = validate_with(&resolver, Some(&txid), None);
    assert_matches!(result, Err(Error::Network { .. }));
}

#[test]
#[parallel]
#[serial(evm)]
fn mint() {
    initialize();

    let token_contract = deploy_test_erc20("Bridged Token", "BRG", 18, 1_000_000);
    let bridge_contract = deploy_bridge(&token_contract.address);
    let reject_list_contract = deploy_reject_list();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_bfa(
        2,
        bridge_contract.address.clone(),
        Some(reject_list_contract.address.clone()),
    );
    let receive_data = rcv_party.blind_receive();
    let recipient = Recipient {
        assignment: Assignment::Fungible(AMOUNT),
        recipient_id: receive_data.recipient_id.clone(),
        witness_data: None,
        transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        public_key: Some(receive_data.public_key.clone()),
        expiration_timestamp: None,
        encryption: Encryption::Optional,
    };
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
    let validate_mint = |evm_rpc_urls: HashMap<u64, String>| {
        validate_consignment(
            &consignment_path,
            &IndexerConfig::Electrum {
                url: ELECTRUM_URL.to_string(),
            },
            &evm_rpc_urls,
            None,
            &validation_options(Some(&txid)),
        )
    };
    let evm_rpc_urls = HashMap::from([(ANVIL_CHAIN_ID, EVM_RPC_URL.to_string())]);

    // no EVM RPC URL for the bridge's chain
    let result = validate_mint(HashMap::new());
    assert_matches!(result, Err(Error::NoEvmRpcUrl { chain_id }) if chain_id == ANVIL_CHAIN_ID);

    // the funds haven't been locked yet
    let result = validate_mint(evm_rpc_urls.clone());
    assert_matches!(result, Err(Error::MintLockNotFound { opid }) if opid == res.details.opid);

    // a custom claim resolver is trusted in place of the EVM RPC endpoint
    struct MockClaimResolver {
        chain_id: u64,
        confirmed: bool,
    }
    impl ResolveClaim for MockClaimResolver {
        fn evm_chain_id(&self) -> Result<u64, ClaimResolverError> {
            Ok(self.chain_id)
        }
        fn is_confirmed(&self, _: &BridgeLocation, _: &Claim) -> Result<bool, ClaimResolverError> {
            Ok(self.confirmed)
        }
    }
    let indexer_resolver = get_resolver(
        &IndexerConfig::Electrum {
            url: ELECTRUM_URL.to_string(),
        },
        BitcoinNetwork::Regtest,
    )
    .unwrap();
    let reject_list_resolver = RemoteRejectListResolver {
        evm_rpc_urls: &evm_rpc_urls,
        evm_chains_checked: false,
    };
    let validate_with_resolvers =
        |claim_resolver: Option<&MockClaimResolver>,
         reject_list_resolver: Option<&RemoteRejectListResolver>| {
            validate_consignment_with_resolver(
                &consignment_path,
                &ChainResolver {
                    inner: &indexer_resolver,
                },
                claim_resolver.map(|r| r as &dyn ResolveClaim),
                reject_list_resolver.map(|r| r as &dyn ResolveRejectList),
                None,
                &validation_options(Some(&txid)),
            )
        };
    let validate_with_claim_resolver = |claim_resolver: &MockClaimResolver| {
        validate_with_resolvers(Some(claim_resolver), Some(&reject_list_resolver))
    };
    let validation = validate_with_claim_resolver(&MockClaimResolver {
        chain_id: ANVIL_CHAIN_ID,
        confirmed: true,
    })
    .unwrap();
    assert_eq!(validation.asset_schema, AssetSchema::Bfa);
    let result = validate_with_claim_resolver(&MockClaimResolver {
        chain_id: ANVIL_CHAIN_ID,
        confirmed: false,
    });
    assert_matches!(result, Err(Error::MintLockNotFound { opid }) if opid == res.details.opid);
    let result = validate_with_claim_resolver(&MockClaimResolver {
        chain_id: ANVIL_CHAIN_ID + 1,
        confirmed: true,
    });
    assert_matches!(result, Err(Error::EvmChainIdMismatch { expected, actual }) if expected == ANVIL_CHAIN_ID && actual == ANVIL_CHAIN_ID + 1);
    // the resolvers are needed for a consignment with claims and a reject list
    let result = validate_with_resolvers(None, Some(&reject_list_resolver));
    assert_matches!(result, Err(Error::NoClaimResolver));
    let confirming_resolver = MockClaimResolver {
        chain_id: ANVIL_CHAIN_ID,
        confirmed: true,
    };
    let result = validate_with_resolvers(Some(&confirming_resolver), None);
    assert_matches!(result, Err(Error::NoRejectListResolver));

    // the lock isn't finalized yet
    erc20_approve(&token_contract.address, &bridge_contract.address, AMOUNT);
    bridge_funds_in(&bridge_contract.address, AMOUNT, &mint_lock_id(&res));
    let result = validate_mint(evm_rpc_urls.clone());
    assert_matches!(result, Err(Error::MintLockNotFound { .. }));

    // the lock is finalized
    evm_mine(EVM_FINALITY_DEPTH);
    let validation = validate_mint(evm_rpc_urls.clone()).unwrap();
    assert!(validation.is_transfer);
    assert_eq!(validation.asset_id, asset.asset_id);
    assert_eq!(validation.asset_schema, AssetSchema::Bfa);
    assert_eq!(
        validation.witnesses,
        vec![ValidatedWitness {
            txid: txid.clone(),
            status: WitnessValidationStatus::Tentative,
            from_consignment: true,
        }]
    );
    assert!(validation.warnings.is_empty());

    // the minted allocation rejected by the reject list contract, once finalized
    let consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    let terminal_ops = consignment.terminal_ops();
    assert_eq!(terminal_ops.len(), 1);
    let terminal = Opout::new(*terminal_ops.iter().next().unwrap(), OS_ASSET, 0);
    reject_list_append(&reject_list_contract.address, &[(terminal, true)]);
    evm_mine(EVM_FINALITY_DEPTH);
    let result = validate_mint(evm_rpc_urls);
    assert_matches!(result, Err(Error::ConsignmentRejected { opouts }) if opouts == vec![terminal.to_string()]);
}

#[test]
#[parallel]
fn reject_list() {
    initialize();

    let list_name = "validate_consignment.list";
    write_opouts_to_reject_list(list_name, &[]);

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_ifa(
        Some(&[AMOUNT]),
        None,
        Some(format!("http://localhost:8140/lists/{list_name}")),
    );
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid = party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid)
        .to_string_lossy()
        .to_string();
    let consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    // the transfer is the terminal operation: its inputs (the issuance) and its outputs (the
    // blinded terminal and the revealed change) are checked against the reject list
    let transfer = consignment
        .bundles
        .iter()
        .next()
        .unwrap()
        .bundle
        .known_transitions
        .iter()
        .next()
        .unwrap();
    assert_eq!(consignment.terminal_ops(), BTreeSet::from([transfer.opid]));
    let issuance = transfer
        .transition
        .inputs()
        .into_iter()
        .next()
        .unwrap()
        .to_string();
    let asset_assigns = transfer.transition.assignments.get(&OS_ASSET).unwrap();
    assert_eq!(asset_assigns.len_u16(), 2);
    let (concealed, revealed): (Vec<u16>, Vec<u16>) = (0..2)
        .partition(|no| matches!(asset_assigns.seal_at(*no), Some(BuilderSeal::Concealed(_))));
    let terminal = Opout::new(transfer.opid, OS_ASSET, concealed[0]).to_string();
    let change = Opout::new(transfer.opid, OS_ASSET, revealed[0]).to_string();
    let mut all_checked = vec![terminal.clone(), change.clone(), issuance.clone()];
    all_checked.sort();
    let mut inputs_and_change = vec![change.clone(), issuance.clone()];
    inputs_and_change.sort();

    // nothing rejected
    validate(&consignment_path, Some(&txid), None).unwrap();

    // a rejected terminal
    write_opouts_to_reject_list(list_name, std::slice::from_ref(&terminal));
    let result = validate(&consignment_path, Some(&txid), None);
    assert_matches!(result, Err(Error::ConsignmentRejected { opouts }) if opouts == vec![terminal.clone()]);

    // a terminal rejected then allowed
    write_opouts_to_reject_list(list_name, &[terminal.clone(), format!("!{terminal}")]);
    validate(&consignment_path, Some(&txid), None).unwrap();

    // a rejected ancestor: the issuance itself, the terminal and the change are all tainted
    write_opouts_to_reject_list(list_name, std::slice::from_ref(&issuance));
    let result = validate(&consignment_path, Some(&txid), None);
    assert_matches!(result, Err(Error::ConsignmentRejected { opouts }) if opouts == all_checked);

    // an allowed terminal shields itself only: the operation's input and change stay tainted, so
    // the consignment is refused (a wallet receiving the terminal would accept it, as it checks
    // the assignments it receives)
    write_opouts_to_reject_list(list_name, &[issuance.clone(), format!("!{terminal}")]);
    let result = validate(&consignment_path, Some(&txid), None);
    assert_matches!(result, Err(Error::ConsignmentRejected { opouts }) if opouts == inputs_and_change);

    // a custom reject list resolver is trusted in place of the reject list server
    struct MockRejectListResolver(Vec<(Opout, bool)>);
    impl ResolveRejectList for MockRejectListResolver {
        fn get_reject_list(&self, source: &RejectListSource) -> Result<RejectList, Error> {
            assert_matches!(source, RejectListSource::Url(url) if url.ends_with("validate_consignment.list"));
            Ok(RejectList::from_decisions(self.0.clone()))
        }
    }
    let indexer_resolver = get_resolver(
        &IndexerConfig::Electrum {
            url: ELECTRUM_URL.to_string(),
        },
        BitcoinNetwork::Regtest,
    )
    .unwrap();
    let validate_with_resolver = |reject_list_resolver: Option<&MockRejectListResolver>| {
        validate_consignment_with_resolver(
            &consignment_path,
            &ChainResolver {
                inner: &indexer_resolver,
            },
            None,
            reject_list_resolver.map(|r| r as &dyn ResolveRejectList),
            None,
            &validation_options(Some(&txid)),
        )
    };
    let terminal = Opout::from_str(&terminal).unwrap();
    let issuance = Opout::from_str(&issuance).unwrap();
    validate_with_resolver(Some(&MockRejectListResolver(vec![]))).unwrap();
    let result = validate_with_resolver(Some(&MockRejectListResolver(vec![(issuance, false)])));
    assert_matches!(result, Err(Error::ConsignmentRejected { opouts }) if opouts == all_checked);
    let result = validate_with_resolver(Some(&MockRejectListResolver(vec![
        (issuance, false),
        (terminal, true),
    ])));
    assert_matches!(result, Err(Error::ConsignmentRejected { opouts }) if opouts == inputs_and_change);
    let result = validate_with_resolver(Some(&MockRejectListResolver(vec![
        (terminal, true),
        (terminal, false),
    ])));
    assert_matches!(result, Err(Error::ConsignmentRejected { .. }));
    // the reject list resolver is needed for an asset with a reject list
    let result = validate_with_resolver(None);
    assert_matches!(result, Err(Error::NoRejectListResolver));
}
