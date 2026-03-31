#[cfg(feature = "electrum")]
use std::ffi::OsString;
#[cfg(all(feature = "electrum", feature = "rgb_fs_store_migration"))]
use std::fs::File;
#[cfg(feature = "electrum")]
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
#[cfg(feature = "_online")]
use std::{
    process::{Command, Stdio},
    sync::{Once, RwLock},
    time::Instant,
};

use std::cell::RefCell;
#[cfg(feature = "_online")]
use std::path::MAIN_SEPARATOR_STR;

#[cfg(feature = "electrum")]
use amplify::set;
#[cfg(feature = "_online")]
use bdk_wallet::bitcoin::Denomination;
use bdk_wallet::descriptor::ExtendedDescriptor;
#[cfg(feature = "electrum")]
use biscuit_auth::{KeyPair, builder::date, macros::*};
#[cfg(feature = "electrum")]
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
#[cfg(feature = "electrum")]
use rgbstd::stl::{EmbeddedMedia as RgbEmbeddedMedia, ProofOfReserves as RgbProofOfReserves};
use serde_json::Value;
use serial_test::parallel;
#[cfg(feature = "_online")]
use serial_test::serial;
#[cfg(feature = "_online")]
use time::OffsetDateTime;

use super::*;

#[cfg(feature = "electrum")]
use crate::encryption::ENC_VERSION;
#[cfg(feature = "electrum")]
use crate::keys::Keys;
#[cfg(all(feature = "electrum", feature = "rgb_fs_store_migration"))]
use crate::keys::restore_keys;
#[cfg(all(feature = "electrum", feature = "rgb_fs_store_migration"))]
use crate::utils::{RGB_RUNTIME_DELETING_DIR, RGB_RUNTIME_DIR};
#[cfg(feature = "_online")]
use crate::wallet::online::*;
#[cfg(feature = "electrum")]
use crate::wallet::rust_only::check_proxy_url;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::wallet::utils::build_indexer;
use crate::{
    keys::generate_keys,
    utils::{
        KEYCHAIN_BTC, KEYCHAIN_RGB, get_account_derivation_children, get_coin_type,
        get_extended_derivation_path,
    },
    wallet::{backup::*, core::*, offline::*, singlesig::*},
};
#[cfg(feature = "electrum")]
use crate::{
    utils::{get_account_data, recipient_id_from_script_buf, script_buf_from_recipient_id},
    wallet::{multisig::*, rust_only::*},
};

const PROXY_HOST: &str = "127.0.0.1:3000/json-rpc";
static PROXY_ENDPOINT: Lazy<String> = Lazy::new(|| format!("rpc://{PROXY_HOST}"));
#[cfg(feature = "_online")]
const EVM_RPC_URL: &str = "http://127.0.0.1:8545";
/// EIP-155 chain ID of the anvil node (its default one, see tests/compose.yaml)
#[cfg(feature = "_online")]
const ANVIL_CHAIN_ID: u64 = 31337;
/// Blocks to mine on anvil for the latest block to become finalized: two epochs of its default
/// 32 slots
#[cfg(feature = "electrum")]
const EVM_FINALITY_DEPTH: u64 = 64;
#[cfg(feature = "_online")]
const TEST_DATA_DIR_PARTS: [&str; 2] = ["tests", "tmp_srv"];
const NOSRV_DATA_DIR_PARTS: [&str; 2] = ["tests", "tmp_nosrv"];
const PASSWORD: &str = "password";
#[cfg(feature = "electrum")]
const RESTORE_DIR_PARTS: [&str; 3] = ["tests", "tmp_srv", "restored"];
const MAX_ALLOCATIONS_PER_UTXO: u32 = 5;

#[cfg(feature = "electrum")]
const PROXY_HOST_MOD_API: &str = "127.0.0.1:3002/json-rpc";
#[cfg(feature = "electrum")]
const PROXY_HOST_MOD_PROTO: &str = "127.0.0.1:3001/json-rpc";
#[cfg(feature = "_online")]
const PROXY_URL: &str = "http://127.0.0.1:3000/json-rpc";
#[cfg(feature = "electrum")]
const PROXY_URL_MOD_API: &str = "http://127.0.0.1:3002/json-rpc";
#[cfg(feature = "electrum")]
const PROXY_URL_MOD_PROTO: &str = "http://127.0.0.1:3001/json-rpc";
#[cfg(feature = "electrum")]
const PROXY_HOST_AUTH: &str = "127.0.0.1:3003/json-rpc";
#[cfg(feature = "electrum")]
const PROXY_URL_AUTH: &str = "http://127.0.0.1:3003/json-rpc";
#[cfg(feature = "electrum")]
static PROXY_ENDPOINT_AUTH: Lazy<String> = Lazy::new(|| format!("rpc://{PROXY_HOST_AUTH}"));
// Ed25519 key pair authenticating calls to the proxy-auth service (see tests/compose.yaml)
#[cfg(feature = "electrum")]
const PROXY_AUTH_SEED: &str = "c01959890b0f215ff37637c58762a4eab0a2ec8be09c6a8436034c8f34b1c07e";
#[cfg(feature = "electrum")]
const PROXY_AUTH_PUBKEY: &str = "0738ae88b5bfc7f915b809937573aa51d3bf00cce2edb5795b99f682d4065f07";
static TRANSPORT_ENDPOINTS: Lazy<Vec<String>> = Lazy::new(|| vec![PROXY_ENDPOINT.clone()]);
#[cfg(feature = "bitcoind")]
const BITCOIND_URL: &str = "http://127.0.0.1:18443";
#[cfg(feature = "bitcoind")]
const BITCOIND_NO_FILTERS_URL: &str = "http://127.0.0.1:18444";
#[cfg(feature = "bitcoind")]
const BITCOIND_NO_TXINDEX_URL: &str = "http://127.0.0.1:18445";
#[cfg(feature = "bitcoind")]
const BITCOIND_USER: &str = "user";
#[cfg(feature = "bitcoind")]
const BITCOIND_PASSWORD: &str = "default_password";
#[cfg(feature = "electrum")]
const ELECTRUM_URL: &str = "127.0.0.1:50001";
#[cfg(feature = "electrum")]
const ELECTRUM_2_URL: &str = "127.0.0.1:50002";
#[cfg(feature = "electrum")]
const ELECTRUM_BLOCKSTREAM_URL: &str = "127.0.0.1:50003";
#[cfg(feature = "electrum")]
const ELECTRUM_SIGNET_CUSTOM_URL: &str = "127.0.0.1:50005";
#[cfg(any(feature = "electrum", feature = "esplora"))]
const ESPLORA_URL: &str = "http://127.0.0.1:8094/regtest/api";
#[cfg(feature = "electrum")]
const MULTISIG_HUB_URL: &str = "http://127.0.0.1:8141";
#[cfg(feature = "electrum")]
const LISTS_DIR_PARTS: [&str; 2] = ["tests", "lists"];
#[cfg(feature = "electrum")]
const HUB_DIR_PARTS: [&str; 2] = ["tests", "hub"];
#[cfg(feature = "_online")]
const TICKER: &str = "TICKER";
#[cfg(feature = "_online")]
const NAME: &str = "asset name";
#[cfg(feature = "electrum")]
const DETAILS: &str = "details with ℧nicode characters";
#[cfg(feature = "_online")]
const PRECISION: u8 = 7;
#[cfg(feature = "_online")]
const AMOUNT: u64 = 666;
#[cfg(feature = "electrum")]
const AMOUNT_INFLATION: u64 = 400;
#[cfg(feature = "_online")]
const AMOUNT_SMALL: u64 = 66;
#[cfg(feature = "electrum")]
const BURN_REASON: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
#[cfg(feature = "_online")]
const FEE_RATE: u64 = 2;
#[cfg(feature = "electrum")]
const FILE_STR: &str = "README.md";
#[cfg(feature = "electrum")]
const FEE_MSG_LOW: &str = "value under minimum 1";
#[cfg(feature = "electrum")]
const FEE_MSG_OVER: &str = "value overflows";
#[cfg(feature = "electrum")]
const EMPTY_MSG: &str = "must contain at least one character.";
#[cfg(feature = "electrum")]
const IDENT_EMPTY_MSG: &str = "ident must contain at least one character";
#[cfg(feature = "electrum")]
const IDENT_TOO_LONG_MSG: &str = "string has invalid length.";
#[cfg(feature = "electrum")]
const IDENT_NOT_ASCII_MSG: &str = "string '{0}' contains invalid character '{1}' at position {2}.";
#[cfg(feature = "electrum")]
const IDENT_NOT_START_MSG: &str = "string '{0}' must not start with character '{1}'.";
const MIN_CONFIRMATIONS: u8 = 1;
const FAKE_TXID: &str = "e5a3e577309df31bd606f48049049d2e1e02b048206ba232944fcc053a176ccb";
#[cfg(feature = "electrum")]
const FAKE_OUTPOINT: &str = "e5a3e577309df31bd606f48049049d2e1e02b048206ba232944fcc053a176ccb:0";
#[cfg(feature = "electrum")]
const FAKE_EVM_ADDRESS: &str = "0x5FbDB2315678afecb367f032d93F642f64180aa3";
#[cfg(feature = "electrum")]
const FAKE_PUBKEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
#[cfg(feature = "electrum")]
const FAKE_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
#[cfg(feature = "electrum")]
const UNKNOWN_IDX: i32 = 9999;
#[cfg(feature = "electrum")]
const TINY_BTC_AMOUNT: u32 = 330;
#[cfg(feature = "_online")]
const QUEUE_DEPTH_EXCEEDED: &str = "Work queue depth exceeded";
#[cfg(feature = "_online")]
const DURATION_RCV_TRANSFER: u32 = 86400;
#[cfg(feature = "_online")]
const DURATION_SEND_TRANSFER: u32 = 3600;
#[cfg(feature = "_online")]
pub(crate) const INDEXER_SYNC_LOOKBACK: usize = 20;
#[cfg(feature = "_online")]
pub(crate) const FAILED_WITNESS_RECEIVE_GRACE_SECS: u32 = 604800;

#[cfg(all(feature = "esplora", not(feature = "electrum")))]
const DEFAULT_INDEXER_URL: &str = ESPLORA_URL;
#[cfg(all(
    feature = "bitcoind",
    not(any(feature = "electrum", feature = "esplora"))
))]
const DEFAULT_INDEXER_URL: &str = BITCOIND_URL;
#[cfg(feature = "electrum")]
const DEFAULT_INDEXER_URL: &str = ELECTRUM_URL;

#[cfg(feature = "electrum")]
const INVALID_INDEXER_MSG: &str = "not a valid electrum server";
#[cfg(all(feature = "esplora", not(feature = "electrum")))]
const INVALID_INDEXER_MSG: &str = "not a valid esplora server";
#[cfg(all(
    feature = "bitcoind",
    not(any(feature = "electrum", feature = "esplora"))
))]
const INVALID_INDEXER_MSG: &str = "cannot connect to bitcoind";

#[cfg(feature = "_online")]
static INIT: Once = Once::new();

thread_local! {
    pub(crate) static MOCK_CHAIN_NET: RefCell<Option<ChainNet>> = const { RefCell::new(None) };
    pub(crate) static MOCK_CONTRACT_DATA: RefCell<Vec<Attachment>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_CONTRACT_DETAILS: RefCell<Option<String>> = const { RefCell::new(None) };
    pub(crate) static MOCK_TOKEN_DATA: RefCell<Vec<TokenData>> = const { RefCell::new(vec![]) };
}

#[cfg(feature = "_online")]
thread_local! {
    pub(crate) static MOCK_CHECK_FEE_RATE: RefCell<Vec<bool>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_INPUT_UNSPENTS: RefCell<Vec<LocalUnspent>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_SKIP_BUILD_DAG: RefCell<Option<()>> = const { RefCell::new(None) };
    pub(crate) static MOCK_SPV_ONLY: RefCell<bool> = const { RefCell::new(false) };
    pub(crate) static MOCK_CHAIN_TXS: RefCell<HashMap<String, (BdkTransaction, u32)>> = RefCell::new(HashMap::new());
    pub(crate) static MOCK_LOCAL_VERSION: RefCell<Option<String>> = const { RefCell::new(None) };
    pub(crate) static MOCK_SEND_END_CRASH: RefCell<Option<()>> = const { RefCell::new(None) };
    pub(crate) static MOCK_STRIP_TERMINALS: RefCell<Option<()>> = const { RefCell::new(None) };
    pub(crate) static MOCK_CLOCK_SKEW_MARGIN: RefCell<Option<u32>> = const { RefCell::new(None) };
    pub(crate) static MOCK_RETENTION_GRACE: RefCell<Option<u32>> = const { RefCell::new(None) };
    pub(crate) static MOCK_MIN_RETENTION_GRACE: RefCell<Option<u32>> = const { RefCell::new(None) };
    pub(crate) static MOCK_CONFIRMATION_MARGIN: RefCell<Option<u32>> = const { RefCell::new(None) };
    pub(crate) static MOCK_MINT_LOCK_WINDOW: RefCell<Option<u32>> = const { RefCell::new(None) };
    pub(crate) static MOCK_MINT_AFTER_BLOCK_GRACE: RefCell<Option<i64>> = const { RefCell::new(None) };
    pub(crate) static MOCK_MAX_MINT_SCAN_BLOCKS: RefCell<Option<u64>> = const { RefCell::new(None) };
}

#[cfg(feature = "_online")]
pub fn initialize() {
    INIT.call_once(|| {
        if std::env::var("SKIP_INIT").is_ok() {
            println!("skipping services initialization");
            return;
        }
        let regtest_script = ["tests", "regtest.sh"].join(MAIN_SEPARATOR_STR);
        println!("starting test services...");
        let output = Command::new(regtest_script)
            .arg("prepare_tests_environment")
            .output()
            .expect("failed to start test services");
        if !output.status.success() {
            println!("{output:?}");
            panic!("failed to start test services");
        }
        wait_indexers_sync()
    });
}

/// Recreate a test service from scratch, dropping its data: its container and, if any, the named
/// volumes it persists to.
#[cfg(feature = "electrum")]
fn recreate_service(service_name: &str, volumes: &[&str]) {
    let cmd_base = vec![s!("-f"), ["tests", "compose.yaml"].join(MAIN_SEPARATOR_STR)];
    let mut cmd = cmd_base.clone();
    cmd.extend([
        s!("rm"),
        s!("-f"),
        s!("-s"),
        s!("-v"),
        service_name.to_string(),
    ]);
    Command::new("docker")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .arg("compose")
        .args(&cmd)
        .output()
        .unwrap_or_else(|_| panic!("failed to remove {service_name} service"));
    for volume in volumes {
        Command::new("docker")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .arg("volume")
            .arg("rm")
            .arg(volume)
            .output()
            .unwrap_or_else(|_| panic!("failed to remove {volume} volume"));
    }
    let mut cmd = cmd_base.clone();
    cmd.extend([s!("up"), s!("-d"), service_name.to_string()]);
    Command::new("docker")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .arg("compose")
        .args(&cmd)
        .output()
        .unwrap_or_else(|_| panic!("failed to start {service_name} service"));
}

#[cfg(feature = "electrum")]
pub fn restart_multisig_hub() {
    recreate_service("rgb-multisig-hub", &["tests_hub"]);
}

/// Reset the proxy, dropping all its data (reservations, consignments and operations), and wait
/// for it to serve requests again.
#[cfg(feature = "electrum")]
pub fn reset_proxy() {
    recreate_service("proxy", &[]);
    let start = Instant::now();
    while check_proxy_url(PROXY_URL).is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "timeout waiting for the proxy to restart"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

// the get_*_wallet! macros can be called with no arguments to use defaults
#[cfg(feature = "_online")]
macro_rules! get_empty_party {
    ($i: expr) => {
        get_empty_party(true, Some($i))
    };
    () => {
        get_empty_party(true, None)
    };
}

#[cfg(feature = "_online")]
macro_rules! get_funded_noutxo_party {
    ($i: expr) => {
        get_funded_noutxo_party(true, Some($i))
    };
    () => {
        get_funded_noutxo_party(true, None)
    };
}

#[cfg(feature = "_online")]
macro_rules! get_funded_party {
    ($i: expr) => {
        get_funded_party(true, Some($i))
    };
    () => {
        get_funded_party(true, None)
    };
}

pub fn mock_asset_terms<W: WalletOffline + ?Sized>(
    wallet: &W,
    text: RicardianContract,
    media: Option<Attachment>,
) -> ContractTerms {
    MOCK_CONTRACT_DATA.with_borrow_mut(|mock_reqs| {
        if mock_reqs.is_empty() {
            wallet.new_asset_terms(text, media)
        } else {
            println!("mocking contract data");
            let mocked_media = mock_reqs.pop();
            wallet.new_asset_terms(text, mocked_media)
        }
    })
}

pub fn mock_token_data(token_data: TokenData) -> TokenData {
    MOCK_TOKEN_DATA.with_borrow_mut(|v| {
        if v.is_empty() {
            token_data
        } else {
            println!("mocking token data");
            v.pop().unwrap()
        }
    })
}

#[cfg(feature = "_online")]
pub fn mock_input_unspents<W: WalletOnline + ?Sized>(
    wallet: &W,
    unspents: &[LocalUnspent],
) -> Vec<LocalUnspent> {
    MOCK_INPUT_UNSPENTS.with_borrow_mut(|v| {
        if v.is_empty() {
            wallet.get_input_unspents(unspents).unwrap()
        } else {
            println!("mocking input unspents");
            std::mem::take(v)
        }
    })
}

pub fn mock_contract_details<W: WalletOffline + ?Sized>(wallet: &W) -> Option<Details> {
    let mock = MOCK_CONTRACT_DETAILS.take();
    if let Some(details) = mock {
        println!("mocking contract details");
        Some(wallet.check_details(details.to_string()).unwrap())
    } else {
        None
    }
}

pub fn mock_chain_net<W: WalletOffline + ?Sized>(wallet: &W) -> ChainNet {
    match MOCK_CHAIN_NET.take() {
        Some(chain_net) => {
            println!("mocking chain net");
            chain_net
        }
        None => wallet.bitcoin_network().into(),
    }
}

#[cfg(feature = "_online")]
pub fn mock_local_version(version: &str) -> String {
    let mock = MOCK_LOCAL_VERSION.take();
    if let Some(mock) = mock {
        println!("mocking local version");
        mock
    } else {
        version.to_string()
    }
}

#[cfg(feature = "_online")]
pub fn skip_check_fee_rate() -> bool {
    MOCK_CHECK_FEE_RATE.with_borrow_mut(|mock| {
        if mock.is_empty() {
            false
        } else {
            println!("mocking check fee rate");
            mock.pop().unwrap()
        }
    })
}

#[cfg(feature = "_online")]
pub fn skip_build_dag() -> bool {
    if MOCK_SKIP_BUILD_DAG.take().is_none() {
        false
    } else {
        println!("skipping check dag (mock)");
        true
    }
}

#[cfg(feature = "_online")]
pub fn mock_chain_tx(txid: &str) -> Option<(BdkTransaction, u32)> {
    MOCK_CHAIN_TXS.with_borrow(|m| m.get(txid).cloned())
}

#[cfg(feature = "_online")]
pub fn spv_only() -> bool {
    let spv_only = MOCK_SPV_ONLY.with_borrow(|s| *s);
    if spv_only {
        println!("refusing witness TX lookup (mock)");
    }
    spv_only
}

#[cfg(feature = "_online")]
pub fn strip_terminals() -> bool {
    if MOCK_STRIP_TERMINALS.take().is_none() {
        false
    } else {
        println!("stripping consignment terminals (mock)");
        true
    }
}

/// Clock skew margin (seconds) to use instead of `PROXY_CLOCK_SKEW_MARGIN_SECS`, if set.
#[cfg(feature = "_online")]
pub fn mock_clock_skew_margin() -> Option<u32> {
    MOCK_CLOCK_SKEW_MARGIN.with_borrow(|m| *m)
}

/// Minimum time (seconds) left on the invoice of a mint to use instead of
/// `MIN_MINT_LOCK_WINDOW_SECS`, if set.
#[cfg(feature = "_online")]
pub fn mock_mint_lock_window() -> Option<u32> {
    MOCK_MINT_LOCK_WINDOW.with_borrow(|m| *m)
}

/// Time (seconds) the block a received mint commits to may precede the invoice to use instead of
/// `MINT_AFTER_BLOCK_GRACE_SECS`, if set.
#[cfg(feature = "_online")]
pub fn mock_mint_after_block_grace() -> Option<i64> {
    MOCK_MINT_AFTER_BLOCK_GRACE.with_borrow(|m| *m)
}

/// Maximum number of blocks scanned for the event of a received mint to use instead of
/// `MAX_MINT_SCAN_BLOCKS`, if set.
#[cfg(feature = "_online")]
pub fn mock_max_mint_scan_blocks() -> Option<u64> {
    MOCK_MAX_MINT_SCAN_BLOCKS.with_borrow(|m| *m)
}

/// Sender proxy retention grace (seconds) to use instead of the one advertised by the proxy, if
/// set.
#[cfg(feature = "_online")]
pub fn mock_retention_grace() -> Option<u32> {
    MOCK_RETENTION_GRACE.with_borrow(|m| *m)
}

/// Minimum sender proxy retention grace (seconds) to use instead of
/// `MIN_SENDER_RETENTION_GRACE_SECS`, if set.
#[cfg(feature = "_online")]
pub fn mock_min_retention_grace() -> Option<u32> {
    MOCK_MIN_RETENTION_GRACE.with_borrow(|m| *m)
}

/// Confirmation margin (seconds) to use instead of `CONFIRMATION_MARGIN_SECS`, if set.
#[cfg(feature = "_online")]
pub fn mock_confirmation_margin() -> Option<u32> {
    MOCK_CONFIRMATION_MARGIN.with_borrow(|m| *m)
}

#[cfg(feature = "_online")]
pub fn mock_send_end_crash() -> bool {
    if MOCK_SEND_END_CRASH.take().is_none() {
        false
    } else {
        println!("simulating send_end crash (mock)");
        true
    }
}

// test utilities
#[macro_use]
mod utils;
#[cfg(feature = "electrum")]
pub(crate) use utils::anvil::*;
#[cfg(feature = "_online")]
pub(crate) use utils::chain::*;
pub(crate) use utils::{api::*, helpers::*};

// API tests
#[cfg(feature = "electrum")]
mod abort_pending_vanilla_tx;
mod backup;
mod blind_receive;
#[cfg(feature = "electrum")]
mod burn;
#[cfg(feature = "_online")]
mod check_indexer_url;
#[cfg(feature = "electrum")]
mod create_utxos;
#[cfg(feature = "electrum")]
mod delete_transfers;
#[cfg(feature = "electrum")]
mod drain_to;
#[cfg(feature = "electrum")]
mod fail_transfers;
#[cfg(feature = "electrum")]
mod finalize_psbt;
mod get_address;
mod get_asset_balance;
#[cfg(feature = "electrum")]
mod get_asset_metadata;
#[cfg(feature = "electrum")]
mod get_btc_balance;
#[cfg(feature = "_online")]
mod get_fee_estimation;
mod get_wallet_data;
mod get_wallet_dir;
#[cfg(feature = "_online")]
mod go_online;
#[cfg(feature = "electrum")]
mod inflate;
#[cfg(feature = "electrum")]
mod issue_asset_bfa;
#[cfg(feature = "electrum")]
mod issue_asset_cfa;
#[cfg(feature = "electrum")]
mod issue_asset_ifa;
#[cfg(feature = "electrum")]
mod issue_asset_nia;
#[cfg(feature = "electrum")]
mod issue_asset_uda;
#[cfg(feature = "electrum")]
mod list_assets;
#[cfg(feature = "electrum")]
mod list_pending_vanilla_txs;
#[cfg(feature = "electrum")]
mod list_transactions;
mod list_transfers;
#[cfg(feature = "electrum")]
mod list_unspents;
mod load;
#[cfg(all(feature = "electrum", feature = "rgb_fs_store_migration"))]
mod migrate;
mod mint;
#[cfg(feature = "electrum")]
mod multisig;
mod new;
#[cfg(feature = "electrum")]
mod refresh;
#[cfg(feature = "electrum")]
mod rust_only;
#[cfg(feature = "_online")]
mod send;
#[cfg(feature = "electrum")]
mod send_btc;
mod sign_psbt;
#[cfg(feature = "electrum")]
mod sync;
mod witness_receive;
