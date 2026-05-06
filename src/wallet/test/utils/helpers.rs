use super::*;

/// Panic if the given expression doesn't match the provided pattern, logging the unexpected result
#[macro_export]
macro_rules! assert_matches {
    ($expression:expr, $pattern:pat $(if $guard:expr)? $(,)?) => {
        match $expression {
            $pattern $(if $guard)? => {},
            _ => {
                panic!("received unexpected result: {}", format!("{:?}", $expression));
            }
        }
    };
}

pub(crate) fn join_with_sep(parts: &[&str]) -> String {
    parts.join(MAIN_SEPARATOR_STR)
}

pub(crate) fn get_current_time() -> u128 {
    let now = std::time::SystemTime::now();
    now.duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

pub(crate) fn get_restore_dir_string() -> String {
    join_with_sep(&RESTORE_DIR_PARTS)
}

pub(crate) fn get_test_data_dir_string() -> String {
    join_with_sep(&TEST_DATA_DIR_PARTS)
}

pub(crate) fn get_restore_dir_path<P: AsRef<Path>>(last: Option<P>) -> PathBuf {
    let mut path = PathBuf::from(get_restore_dir_string());
    if let Some(l) = last {
        path = path.join(l);
    }
    path
}

pub(crate) fn get_test_data_dir_path() -> PathBuf {
    PathBuf::from(get_test_data_dir_string())
}

pub(crate) fn create_test_data_dir() -> PathBuf {
    let test_data_dir = get_test_data_dir_path();
    if !test_data_dir.exists() {
        fs::create_dir_all(&test_data_dir).unwrap();
    }
    test_data_dir
}

pub(crate) fn get_test_wallet_data(data_dir: &str) -> WalletData {
    WalletData {
        data_dir: data_dir.to_string(),
        bitcoin_network: BitcoinNetwork::Regtest,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        supported_schemas: AssetSchema::VALUES.to_vec(),
    }
}

pub(crate) fn get_test_wallet_with_keys(keys: &Keys) -> Wallet {
    let wallet_keys = SinglesigKeys::from_keys(keys, None);
    get_test_wallet_raw(&wallet_keys, None, BitcoinNetwork::Regtest)
}

// return a wallet for testing
pub(crate) fn get_test_wallet_with_net(
    private_keys: bool,
    max_allocations_per_utxo: Option<u32>,
    bitcoin_network: BitcoinNetwork,
) -> Wallet {
    let keys = generate_keys(bitcoin_network, WitnessVersion::Taproot);
    let wallet_keys = if private_keys {
        SinglesigKeys::from_keys(&keys, None)
    } else {
        SinglesigKeys::from_keys_no_mnemonic(&keys, None)
    };
    get_test_wallet_raw(&wallet_keys, max_allocations_per_utxo, bitcoin_network)
}

// return a wallet for testing
pub(crate) fn get_test_wallet_raw(
    wallet_keys: &SinglesigKeys,
    max_allocations_per_utxo: Option<u32>,
    bitcoin_network: BitcoinNetwork,
) -> Wallet {
    create_test_data_dir();

    let wallet = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: max_allocations_per_utxo.unwrap_or(MAX_ALLOCATIONS_PER_UTXO),
            supported_schemas: AssetSchema::VALUES.to_vec(),
        },
        wallet_keys.clone(),
    )
    .unwrap();
    println!("wallet directory: {:?}", wallet.get_wallet_dir());
    wallet
}

// return a regtest wallet for testing
pub(crate) fn get_test_wallet(private_keys: bool, max_allocations_per_utxo: Option<u32>) -> Wallet {
    get_test_wallet_with_net(
        private_keys,
        max_allocations_per_utxo,
        BitcoinNetwork::Regtest,
    )
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_funded_party_p2wpkh() -> SinglesigParty {
    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::SegWitV0);
    let wallet_keys = SinglesigKeys::from_keys(&keys, None);
    let mut wallet = get_test_wallet_raw(&wallet_keys, None, BitcoinNetwork::Regtest);
    let online = wallet.go_online(test_go_online_options(None)).unwrap();
    let mut party = party!(wallet, online);
    fund_wallet(party.get_address());
    party.create_utxos_default();
    party
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_empty_wallet(
    private_keys: bool,
    indexer_url: Option<String>,
) -> (Wallet, Online) {
    let mut wallet = get_test_wallet(private_keys, None);
    let online = wallet
        .go_online(test_go_online_options(indexer_url.as_deref()))
        .unwrap();
    (wallet, online)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_funded_noutxo_wallet(
    private_keys: bool,
    indexer_url: Option<String>,
) -> (Wallet, Online) {
    let (mut wallet, online) = get_empty_wallet(private_keys, indexer_url);
    fund_wallet(wallet.get_address().unwrap());
    (wallet, online)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_funded_party(private_keys: bool, indexer_url: Option<String>) -> SinglesigParty {
    let (wallet, online) = get_funded_noutxo_wallet(private_keys, indexer_url);
    let mut party = party!(wallet, online);
    party.create_utxos_default();
    party
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_empty_party(private_keys: bool, indexer_url: Option<String>) -> SinglesigParty {
    let (wallet, online) = get_empty_wallet(private_keys, indexer_url);
    party!(wallet, online)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_funded_noutxo_party(
    private_keys: bool,
    indexer_url: Option<String>,
) -> SinglesigParty {
    let (wallet, online) = get_funded_noutxo_wallet(private_keys, indexer_url);
    party!(wallet, online)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn send_to_address(address: String) {
    send_sats_to_address(address, None);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn send_sats_to_address(address: String, sats: Option<u64>) {
    let amt = BdkAmount::from_sat(sats.unwrap_or(100_000_000));
    let btc_str = amt.to_string_in(Denomination::Bitcoin);
    let t_0 = OffsetDateTime::now_utc();
    let bitcoin_cli = bitcoin_cli();
    loop {
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
            panic!("could not send to address ({QUEUE_DEPTH_EXCEEDED})");
        }
        let output = Command::new("docker")
            .stdin(Stdio::null())
            .arg("compose")
            .args(&bitcoin_cli)
            .arg("-rpcwallet=miner")
            .arg("sendtoaddress")
            .arg(&address)
            .arg(&btc_str)
            .output()
            .expect("failed to fund wallet");
        if !output.status.success()
            && String::from_utf8(output.stderr)
                .unwrap()
                .contains(QUEUE_DEPTH_EXCEEDED)
        {
            eprintln!("work queue depth exceeded");
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        }
        assert!(output.status.success());
        break;
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn fund_wallet(address: String) {
    send_to_address(address);
    mine(false);
}

pub(crate) fn compare_test_directories(src: &Path, dst: &Path, skip: &[&str]) {
    let ignores = RegexSet::new(skip).unwrap();
    let cmp = dircmp::Comparison::new(ignores);
    let diff = cmp.compare(src, dst).unwrap();
    assert!(diff.is_empty());
}

pub(crate) fn print_unspents(unspents: &[Unspent], msg: &str) {
    println!("\n{msg} ({} unspents)", unspents.len());
    for u in unspents {
        println!(
            "> {} {} {}",
            u.utxo.outpoint,
            u.utxo.btc_amount,
            if u.utxo.colorable {
                "colorable"
            } else {
                "vanilla"
            }
        );
        for a in &u.rgb_allocations {
            println!(
                "\t- {} {:?} {}",
                a.asset_id.as_ref().unwrap(),
                a.assignment,
                if a.settled { "settled" } else { "pending" }
            )
        }
    }
}

pub(crate) fn wait_for_function<F>(mut func: F, timeout_secs: u8, interval_ms: u16) -> bool
where
    F: FnMut() -> bool,
{
    let start = Instant::now();
    let timeout = Duration::from_secs(timeout_secs as u64);
    while start.elapsed() < timeout {
        if func() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(interval_ms as u64));
    }
    false
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn write_opouts_to_reject_list(filename: &str, opouts: &[String]) {
    let lists_dir = PathBuf::from(join_with_sep(&LISTS_DIR_PARTS));
    if !lists_dir.exists() {
        fs::create_dir_all(&lists_dir).unwrap();
    }
    let file_path = lists_dir.join(filename);
    let mut file = std::fs::File::create(&file_path).unwrap();
    for opout in opouts {
        writeln!(file, "{}", opout).unwrap()
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_proxy_client(proxy_url: Option<&str>) -> ProxyClient {
    ProxyClient::new(proxy_url.unwrap_or(PROXY_URL)).unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_go_online_options(indexer_url: Option<&str>) -> OnlineOptions {
    OnlineOptions {
        indexer_url: indexer_url.unwrap_or(ELECTRUM_URL).to_string(),
        skip_consistency_check: true,
        vanilla_sync_lookback: INDEXER_SYNC_LOOKBACK as u32,
    }
}
