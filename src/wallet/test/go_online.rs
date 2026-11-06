use super::*;

#[cfg(all(feature = "electrum", feature = "esplora"))]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = offline_party!(get_test_wallet(true, None));

    // go online
    let bak_info_before = party.db_backup_info_opt();
    assert!(bak_info_before.is_none());
    let result_1 = party.go_online_result(false, None);
    let bak_info_after = party.db_backup_info_opt();
    assert!(bak_info_after.is_none());
    assert!(result_1.is_ok());

    // can go online again with the same electrum URL
    let result_2 = party.go_online_result(false, None);
    assert!(result_2.is_ok());
    let online = result_2.unwrap();
    assert_eq!(result_1.unwrap(), online);

    // going online again with the same electrum URL updates the other options
    let online_2 = party
        .wallet
        .go_online(OnlineOptions {
            vanilla_sync_lookback: INDEXER_SYNC_LOOKBACK as u32 + 1,
            ..test_go_online_options(None)
        })
        .unwrap();
    assert_eq!(online_2, online);
    assert_eq!(
        party.wallet.vanilla_sync_lookback(),
        INDEXER_SYNC_LOOKBACK as u32 + 1
    );

    // can go online again with a different electrum URL, which also updates the other options
    let result_3 = party.wallet.go_online(OnlineOptions {
        skip_consistency_check: false,
        vanilla_sync_lookback: INDEXER_SYNC_LOOKBACK as u32 + 2,
        ..test_go_online_options(Some(ELECTRUM_2_URL))
    });
    assert!(result_3.is_ok());
    assert_eq!(
        party.wallet.vanilla_sync_lookback(),
        INDEXER_SYNC_LOOKBACK as u32 + 2
    );

    // can go online again with esplora URL
    let result_4 = party.go_online_result(false, Some(ESPLORA_URL));
    assert!(result_4.is_ok());

    // can go online again with bitcoind URL
    #[cfg(feature = "bitcoind")]
    {
        let result_5 = party.go_online_result(false, Some(BITCOIND_URL));
        assert!(result_5.is_ok());
    }
}

#[cfg(feature = "_online")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let mut party = offline_party!(get_test_wallet(true, None));
    let mut party_testnet = offline_party!(get_test_wallet_with_net(
        true,
        None,
        BitcoinNetwork::Testnet
    ));

    // cannot go online with an invalid indexer URL
    let result = party.go_online_result(false, Some("other:50001"));
    let details = INVALID_INDEXER_MSG;
    assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains(details) ));

    // cannot go online again with an invalid indexer URL
    #[cfg(feature = "electrum")]
    let indexer_url = ELECTRUM_URL;
    #[cfg(all(feature = "esplora", not(feature = "electrum")))]
    let indexer_url = ESPLORA_URL;
    #[cfg(all(
        feature = "bitcoind",
        not(any(feature = "electrum", feature = "esplora"))
    ))]
    let indexer_url = BITCOIND_URL;
    party.go_online(false, Some(indexer_url));
    let result = party.go_online_result(false, Some("other:50001"));
    assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains(details) ));

    let details_another_chain = "resolver is for another chain-network pair";

    #[cfg(feature = "bitcoind")]
    {
        // bitcoind wrong network
        let result = party_testnet.go_online_result(false, Some(BITCOIND_URL));
        assert!(
            matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details_another_chain )
        );
    }

    #[cfg(feature = "electrum")]
    {
        // electrs wrong network
        let result = party_testnet.go_online_result(false, None);
        assert!(
            matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details_another_chain )
        );

        // unsupported electrs variant
        let result = party.go_online_result(false, Some(ELECTRUM_BLOCKSTREAM_URL));
        let details = "verbose transactions are unsupported by the provided electrum service";
        assert!(
            matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains(details) )
        );
    }

    #[cfg(feature = "esplora")]
    {
        // esplora wrong network
        let result = party_testnet.go_online_result(false, Some(ESPLORA_URL));
        assert!(
            matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details_another_chain )
        );
    }

    // bad online object
    let wrong_online = Online { id: 1 };
    let result = party.wallet.check_online(wrong_online);
    assert!(matches!(result, Err(Error::CannotChangeOnline)));
}

#[cfg(feature = "bitcoind")]
#[test]
#[parallel]
fn bitcoind() {
    initialize();

    // wallets synced via bitcoind, funded by the electrum-backed helpers; the regtest node has a
    // block filter index
    let mut party = get_funded_party!(BITCOIND_URL.to_string());
    let mut rcv_party = get_funded_party!(BITCOIND_URL.to_string());
    match party.wallet.indexer() {
        Indexer::Bitcoind(client) => assert!(client.block_filters),
        #[cfg(any(feature = "electrum", feature = "esplora"))]
        _ => panic!("expected a bitcoind indexer"),
    }

    // same options: same online object
    let online = party
        .wallet
        .go_online(test_go_online_options(Some(BITCOIND_URL)));
    assert_eq!(online.unwrap(), party.online);

    // a transfer is validated and settled via bitcoind
    let asset = party.issue_asset_nia(None);
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
    let txid = party
        .wallet
        .send(
            party.online,
            recipient_map,
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            default_send_expiration(),
            Some(PROXY_ENDPOINT.clone()),
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());
    let transfer_statuses = |party: &SinglesigParty, rcv_party: &SinglesigParty| {
        let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
        let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
        let (transfer, _, _) = party.get_test_transfer_sender(&txid);
        let (transfer_data, _) = party.get_test_transfer_data(&transfer);
        (rcv_transfer_data.status, transfer_data.status)
    };
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    assert_eq!(
        transfer_statuses(&party, &rcv_party),
        (
            TransferStatus::WaitingConfirmations,
            TransferStatus::WaitingConfirmations
        )
    );
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    assert_eq!(
        transfer_statuses(&party, &rcv_party),
        (TransferStatus::Settled, TransferStatus::Settled)
    );

    // can switch to electrum and back
    #[cfg(feature = "electrum")]
    {
        let online_electrum = party.go_online(false, Some(ELECTRUM_URL));
        assert_ne!(online_electrum, party.online);
        party.online = party.go_online(false, Some(BITCOIND_URL));
        assert_ne!(online_electrum, party.online);
    }

    let mut online_options = test_go_online_options(Some(BITCOIND_URL));

    // wrong credentials
    online_options.indexer_config = IndexerConfig::Bitcoind {
        url: BITCOIND_URL.to_string(),
        auth: BitcoindAuth::UserPass {
            username: BITCOIND_USER.to_string(),
            password: s!("wrong"),
        },
    };
    let result = party.wallet.go_online(online_options.clone());
    assert!(
        matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains("cannot connect"))
    );

    // unreachable node
    online_options = test_go_online_options(Some(BITCOIND_URL));
    online_options.indexer_config = IndexerConfig::Bitcoind {
        url: s!("http://127.0.0.1:1"),
        auth: bitcoind_pass_auth(),
    };
    let result = party.wallet.go_online(online_options.clone());
    assert!(
        matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains("cannot connect"))
    );

    // missing cookie file
    online_options = test_go_online_options(Some(BITCOIND_URL));
    online_options.indexer_config = IndexerConfig::Bitcoind {
        url: BITCOIND_URL.to_string(),
        auth: BitcoindAuth::CookieFile {
            path: s!("/nonexistent/.cookie"),
        },
    };
    let result = party.wallet.go_online(online_options.clone());
    assert!(matches!(result, Err(Error::InvalidIndexer { .. })));

    // wrong network
    let mut party_testnet = offline_party!(get_test_wallet_with_net(
        true,
        None,
        BitcoinNetwork::Testnet
    ));
    let result = party_testnet.go_online_result(false, Some(BITCOIND_URL));
    let details = "resolver is for another chain-network pair";
    assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details));

    // check_indexer_url reports the protocol
    let protocol = check_indexer_url(
        BITCOIND_URL,
        Some(&bitcoind_pass_auth()),
        BitcoinNetwork::Regtest,
    )
    .unwrap();
    assert!(matches!(protocol, IndexerProtocol::Bitcoind));
}

// only covers what the block-filters node cannot: detection of the missing filter index and the
// sync path fetching every block. Everything else bitcoind-related is covered by `bitcoind`.
#[cfg(feature = "bitcoind")]
#[test]
#[parallel]
fn bitcoind_no_block_filters() {
    initialize();

    // the node has no -blockfilterindex and lives on its own isolated chain, so funding and
    // mining go through the node itself instead of the shared helpers
    let mut party = get_empty_party!(BITCOIND_NO_FILTERS_URL.to_string());
    match party.wallet.indexer() {
        Indexer::Bitcoind(client) => assert!(!client.block_filters),
        #[cfg(any(feature = "electrum", feature = "esplora"))]
        _ => panic!("expected a bitcoind indexer"),
    }

    // an incoming TX is first seen unconfirmed (via the mempool), then confirmed (applied from a
    // fetched block)
    fund_wallet_no_filters(&party.get_address());
    let balance = party
        .wallet
        .get_btc_balance(Some(party.online), false)
        .unwrap();
    assert_eq!(balance.vanilla.settled, 0);
    assert!(balance.vanilla.future > 0);
    mine_no_filters(1);
    let balance = party
        .wallet
        .get_btc_balance(Some(party.online), false)
        .unwrap();
    assert!(balance.vanilla.settled > 0);
}

// a node without -txindex cannot resolve RGB witnesses by TX ID, so going online is refused
#[cfg(feature = "bitcoind")]
#[test]
#[parallel]
fn bitcoind_no_txindex() {
    initialize();

    let mut party = offline_party!(get_test_wallet(true, None));
    let result = party.go_online_result(false, Some(BITCOIND_NO_TXINDEX_URL));
    let details = "bitcoind has no synced transaction index (-txindex)";
    assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn invalid_chain_net() {
    initialize();

    // URL for custom signet but wallet for default signet
    let mut party_signet =
        offline_party!(get_test_wallet_with_net(true, None, BitcoinNetwork::Signet));
    let result = party_signet.go_online_result(false, Some(ELECTRUM_SIGNET_CUSTOM_URL));
    let details = "resolver is for another chain-network pair";
    assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains(details) ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn consistency_check_fail_bitcoins() {
    initialize();

    // prepare test wallet with UTXOs + an asset
    let mut orig_party = get_funded_party!();
    orig_party.issue_asset_nia(None);

    // get wallet data
    let wallet_dir_orig = orig_party.wallet.get_wallet_dir();
    let keys = orig_party.get_keys();
    let fingerprint = keys.master_fingerprint.clone();
    // prepare directories
    let data_dir_prefill_1 = get_test_data_dir_path().join("test_consistency.bitcoin.prefill_1");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.bitcoin.prefill_2");
    let data_dir_prefill_3 = get_test_data_dir_path().join("test_consistency.bitcoin.prefill_3");
    let wallet_dir_prefill = PathBuf::from(&data_dir_prefill_2).join(&fingerprint);
    let wallet_dir_prefill_2 = PathBuf::from(&data_dir_prefill_3).join(&fingerprint);
    for dir in [
        &data_dir_prefill_1,
        &data_dir_prefill_2,
        &wallet_dir_prefill,
        &wallet_dir_prefill_2,
    ] {
        if PathBuf::from(dir).is_dir() {
            fs::remove_dir_all(dir.clone()).unwrap();
        }
        fs::create_dir_all(dir).unwrap();
    }
    // prepare wallet data objects
    let wallet_data_empty = get_test_wallet_data(data_dir_prefill_1.to_str().unwrap());
    let wallet_data_prefill = get_test_wallet_data(data_dir_prefill_2.to_str().unwrap());
    let wallet_data_prefill_2 = get_test_wallet_data(data_dir_prefill_3.to_str().unwrap());
    // copy original wallet's db data to prefilled wallet data dir
    let db_files: Vec<OsString> = fs::read_dir(&wallet_dir_orig)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .into_string()
                .unwrap()
                .starts_with(RGB_LIB_DB_NAME)
        })
        .map(|e| e.as_ref().unwrap().file_name())
        .collect();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_orig).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill).join(file);
        fs::copy(src, dst).unwrap();
    }

    // introduce asset inconsistency by spending UTXOs from other instance of the same wallet,
    // simulating a wallet used on multiple devices (which needs to be avoided to prevent asset
    // loss)
    let mut party_empty = offline_party!(Wallet::new(wallet_data_empty, keys.clone()).unwrap());
    let online_empty = party_empty.go_online(false, None);
    let mut party_empty = party!(party_empty.wallet, online_empty);
    let request = party_empty.wallet.bdk_wallet().start_full_scan();
    let update = party_empty.wallet.indexer().full_scan(request).unwrap();
    party_empty
        .wallet
        .bdk_wallet_mut()
        .apply_update(update)
        .unwrap();
    let txn = party_empty.wallet.database().begin_transaction().unwrap();
    party_empty.wallet.persist_bdk(&txn).unwrap();
    txn.commit().unwrap();
    let mut rcv_party = get_funded_party!();
    party_empty.drain_to(&rcv_party.get_address());

    // detect asset inconsistency
    let err = "spent bitcoins with another wallet";
    let mut party_prefill = offline_party!(Wallet::new(wallet_data_prefill, keys.clone()).unwrap());
    let result = party_prefill.go_online_result(false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e.contains(err)));

    // make sure detection works multiple times (doesn't get reset on first failed check)
    let mut party_prefill_2 =
        offline_party!(Wallet::new(wallet_data_prefill_2, keys.clone()).unwrap());
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_prefill).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill_2).join(file);
        fs::copy(src, dst).unwrap();
    }
    let result = party_prefill_2.go_online_result(false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e.contains(err)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn consistency_check_fail_utxos() {
    initialize();

    // prepare test wallet with UTXOs + an asset
    let mut orig_party = get_funded_party!();
    orig_party.issue_asset_nia(None);

    // get wallet data
    let wallet_dir_orig = orig_party.wallet.get_wallet_dir();
    let keys = orig_party.wallet.get_keys();
    let fingerprint = keys.master_fingerprint.clone();
    // prepare directories
    let data_dir_prefill_1 = get_test_data_dir_path().join("test_consistency.utxos.prefill_1");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.utxos.prefill_2");
    let data_dir_prefill_3 = get_test_data_dir_path().join("test_consistency.utxos.prefill_3");
    let wallet_dir_prefill = PathBuf::from(&data_dir_prefill_2).join(&fingerprint);
    let wallet_dir_prefill_2 = PathBuf::from(&data_dir_prefill_3).join(&fingerprint);
    for dir in [
        &data_dir_prefill_1,
        &data_dir_prefill_2,
        &wallet_dir_prefill,
        &wallet_dir_prefill_2,
    ] {
        if PathBuf::from(dir).is_dir() {
            fs::remove_dir_all(dir.clone()).unwrap();
        }
        fs::create_dir_all(dir).unwrap();
    }
    // prepare wallet data objects
    let wallet_data_empty = get_test_wallet_data(data_dir_prefill_1.to_str().unwrap());
    let wallet_data_prefill = get_test_wallet_data(data_dir_prefill_2.to_str().unwrap());
    let wallet_data_prefill_2 = get_test_wallet_data(data_dir_prefill_3.to_str().unwrap());
    // copy original wallet's db data to prefilled wallet data dir
    let db_files: Vec<OsString> = fs::read_dir(&wallet_dir_orig)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .into_string()
                .unwrap()
                .starts_with(RGB_LIB_DB_NAME)
        })
        .map(|e| e.as_ref().unwrap().file_name())
        .collect();
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_orig).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill).join(file);
        fs::copy(src, dst).unwrap();
    }

    // introduce asset inconsistency by spending UTXOs from other instance of the same wallet,
    // simulating a wallet used on multiple devices (which needs to be avoided to prevent asset
    // loss)
    let mut party_empty = offline_party!(Wallet::new(wallet_data_empty, keys.clone()).unwrap());
    let online_empty = party_empty.go_online(false, None);
    let mut party_empty = party!(party_empty.wallet, online_empty);
    let request = party_empty.wallet.bdk_wallet().start_full_scan();
    let update = party_empty.wallet.indexer().full_scan(request).unwrap();
    party_empty
        .wallet
        .bdk_wallet_mut()
        .apply_update(update)
        .unwrap();
    let txn = party_empty.wallet.database().begin_transaction().unwrap();
    party_empty.wallet.persist_bdk(&txn).unwrap();
    txn.commit().unwrap();
    let mut rcv_party = get_funded_party!();
    party_empty.drain_to(&rcv_party.get_address());

    // detect asset inconsistency
    let err = "spent bitcoins with another wallet";
    let mut party_prefill = offline_party!(Wallet::new(wallet_data_prefill, keys.clone()).unwrap());
    let result = party_prefill.go_online_result(false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e.contains(err)));

    // make sure detection works multiple times (doesn't get reset on first failed check)
    let mut party_prefill_2 =
        offline_party!(Wallet::new(wallet_data_prefill_2, keys.clone()).unwrap());
    for file in &db_files {
        let src = PathBuf::from(&wallet_dir_prefill).join(file);
        let dst = PathBuf::from(&wallet_dir_prefill_2).join(file);
        fs::copy(src, dst).unwrap();
    }
    let result = party_prefill_2.go_online_result(false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e.contains(err)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn consistency_check_fail_media() {
    initialize();

    // prepare test wallet with UTXOs + an asset
    let mut orig_party = get_funded_party!();
    orig_party.issue_asset_cfa(None, Some(FILE_STR.to_string()));

    // get wallet data
    let wallet_dir_orig = orig_party.wallet.get_wallet_dir();
    let keys = orig_party.wallet.get_keys();
    let fingerprint = keys.master_fingerprint.clone();
    // prepare directories
    let data_dir_prefill_1 = get_test_data_dir_path().join("test_consistency.media.prefill_1");
    let data_dir_prefill_2 = get_test_data_dir_path().join("test_consistency.media.prefill_2");
    let data_dir_prefill_3 = get_test_data_dir_path().join("test_consistency.media.prefill_3");
    let wallet_dir_prefill_1 = PathBuf::from(&data_dir_prefill_1).join(&fingerprint);
    let wallet_dir_prefill_2 = PathBuf::from(&data_dir_prefill_2).join(&fingerprint);
    let wallet_dir_prefill_3 = PathBuf::from(&data_dir_prefill_3).join(&fingerprint);
    for dir in [
        &data_dir_prefill_1,
        &data_dir_prefill_2,
        &data_dir_prefill_3,
    ] {
        if PathBuf::from(dir).is_dir() {
            fs::remove_dir_all(dir.clone()).unwrap();
        }
        fs::create_dir_all(dir).unwrap();
    }
    // prepare wallet data objects
    let wallet_data_prefill_1 = get_test_wallet_data(data_dir_prefill_1.to_str().unwrap());
    let wallet_data_prefill_2 = get_test_wallet_data(data_dir_prefill_2.to_str().unwrap());
    let wallet_data_prefill_3 = get_test_wallet_data(data_dir_prefill_3.to_str().unwrap());
    // copy original wallet's data to prefilled wallets 1 + 2 data dir
    for destination in [&wallet_dir_prefill_1, &wallet_dir_prefill_2] {
        let result = copy_dir::copy_dir(&wallet_dir_orig, destination);
        assert!(result.unwrap().is_empty());
    }

    // check the first wallet copy works ok
    let mut party_prefill_1 =
        offline_party!(Wallet::new(wallet_data_prefill_1, keys.clone()).unwrap());
    let result = party_prefill_1.go_online_result(false, None);
    assert!(result.is_ok());

    // introduce media inconsistency by removing media dir
    fs::remove_dir_all(wallet_dir_prefill_2.join(MEDIA_DIR)).unwrap();

    // detect inconsistency
    let err = "DB media do not match with the ones stored in media directory";
    let mut party_prefill_2 =
        offline_party!(Wallet::new(wallet_data_prefill_2.clone(), keys.clone()).unwrap());
    let result = party_prefill_2.go_online_result(false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e == err));

    // make sure detection works multiple times
    let result = copy_dir::copy_dir(wallet_dir_prefill_2, wallet_dir_prefill_3);
    assert!(result.unwrap().is_empty());
    let mut party_prefill_3 =
        offline_party!(Wallet::new(wallet_data_prefill_3, keys.clone()).unwrap());
    let result = party_prefill_3.go_online_result(false, None);
    assert!(matches!(result, Err(Error::Inconsistency { details: e }) if e == err));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn consistency_check_after_recipient_broadcast() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // send (not a donation, so the sender doesn't broadcast)
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send(recipient_map, FEE_RATE, None).txid;
    assert!(!txid.is_empty());

    // the recipient ACKs and broadcasts the TX
    rcv_party.wait_for_refresh(None);
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );

    // the sender can still go online after the recipient has spent its inputs
    let result = party.go_online_result(false, None);
    assert!(result.is_ok(), "{result:?}");

    // the sender refresh progresses the transfer, marking its inputs as spent
    party.wait_for_refresh(Some(&asset.asset_id));
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // the inputs are not excused anymore, as the transfer isn't waiting for the counterparty
    let result = party.go_online_result(false, None);
    assert!(result.is_ok(), "{result:?}");

    // transfers settle
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // the sender can still go online after the transfer has settled
    let result = party.go_online_result(false, None);
    assert!(result.is_ok(), "{result:?}");
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn on_off_online() {
    initialize();

    // create wallet and go online
    let mut party = offline_party!(get_test_wallet(true, None));
    let wallet_data = party.wallet.get_wallet_data();
    let keys = party.wallet.get_keys();
    let _online = party.go_online(false, None);

    // go offline and close wallet
    drop(party);

    // re-instantiate wallet and go back online
    let mut party = offline_party!(Wallet::new(wallet_data, keys.clone()).unwrap());
    party.go_online(false, None);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn offline() {
    initialize();

    // don't go online and manually craft the Online object
    let wallet = get_test_wallet(true, Some(MAX_ALLOCATIONS_PER_UTXO));
    let online = Online { id: 0 };
    let mut party = party!(wallet, online);

    // the online check should report that the wallet is offline
    let result = party.create_utxos_begin_result(true, None, None, FEE_RATE);
    assert!(matches!(result, Err(Error::Offline)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn proxy_auth_keys() {
    initialize();

    let mut party = offline_party!(get_test_wallet(true, None));

    // the test seed matches the pubkey the proxy-auth service is started with
    let seed: [u8; 32] = hex::decode(PROXY_AUTH_SEED).unwrap().try_into().unwrap();
    let sk = ed25519_from_seed(&seed);
    assert_eq!(
        hex::encode(sk.verifying_key().to_bytes()),
        PROXY_AUTH_PUBKEY
    );

    let options_with_keys = |proxy_auth_keys: HashMap<String, String>| {
        let mut online_options = test_go_online_options(None);
        online_options.proxy_auth_keys = proxy_auth_keys;
        online_options
    };

    // invalid transport endpoint
    let result = party.wallet.go_online(options_with_keys(HashMap::from([(
        PROXY_HOST.to_string(),
        PROXY_AUTH_SEED.to_string(),
    )])));
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // unsupported transport endpoint
    let result = party.wallet.go_online(options_with_keys(HashMap::from([(
        format!("ws://{PROXY_HOST}"),
        PROXY_AUTH_SEED.to_string(),
    )])));
    assert!(matches!(result, Err(Error::UnsupportedTransportType)));

    // invalid seeds: not hex, wrong length
    for seed in [
        "not hex",
        &PROXY_AUTH_SEED[..62],
        &format!("{PROXY_AUTH_SEED}00"),
    ] {
        let result = party.wallet.go_online(options_with_keys(HashMap::from([(
            PROXY_ENDPOINT.clone(),
            seed.to_string(),
        )])));
        assert!(matches!(
            result,
            Err(Error::InvalidProxyAuthKey { details: _ })
        ));
    }
    assert!(party.wallet.online_data().is_none());

    // keys are stored by proxy URL
    let online = party
        .wallet
        .go_online(options_with_keys(HashMap::from([(
            PROXY_ENDPOINT.clone(),
            PROXY_AUTH_SEED.to_string(),
        )])))
        .unwrap();
    let online_data = party.wallet.online_data().as_ref().unwrap();
    assert_eq!(online_data.proxy_auth_keys.len(), 1);
    assert_eq!(online_data.proxy_auth_keys[PROXY_URL].to_bytes(), seed);

    // signatures are only produced for configured proxies and verify against the pubkey
    let message = "message";
    let digest = sha256_digest(message.as_bytes());
    let signature = online_data
        .proxy_auth_signature(PROXY_URL, message)
        .unwrap();
    assert!(verify_ed25519(
        &sk.verifying_key(),
        &digest,
        &hex::decode(signature).unwrap()
    ));
    assert!(
        online_data
            .proxy_auth_signature("http://other/json-rpc", message)
            .is_none()
    );

    // going online again with the same indexer keeps the online object and replaces the keys
    let online_2 = party
        .wallet
        .go_online(options_with_keys(HashMap::new()))
        .unwrap();
    assert_eq!(online_2, online);
    let online_data = party.wallet.online_data().as_ref().unwrap();
    assert!(online_data.proxy_auth_keys.is_empty());
}
