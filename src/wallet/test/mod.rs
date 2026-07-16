#[cfg(feature = "electrum")]
use std::ffi::OsString;
#[cfg(feature = "electrum")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use std::{
    process::{Command, Stdio},
    sync::{Once, RwLock},
    time::Instant,
};

use std::{cell::RefCell, path::MAIN_SEPARATOR_STR};

#[cfg(feature = "electrum")]
use amplify::set;
#[cfg(any(feature = "electrum", feature = "esplora"))]
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
#[cfg(any(feature = "electrum", feature = "esplora"))]
use serial_test::serial;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use time::OffsetDateTime;

use super::*;

#[cfg(feature = "electrum")]
use crate::keys::Keys;
#[cfg(feature = "electrum")]
use crate::wallet::rust_only::check_proxy_url;
#[cfg(all(feature = "esplora", not(feature = "electrum")))]
use crate::wallet::rust_only::*;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::wallet::{online::*, rust_only::check_indexer_url, utils::build_indexer};
use crate::{
    keys::generate_keys,
    utils::{
        KEYCHAIN_BTC, KEYCHAIN_RGB, get_account_derivation_children, get_coin_type,
        get_extended_derivation_path,
    },
    wallet::{core::*, offline::*, singlesig::*},
};
#[cfg(feature = "electrum")]
use crate::{
    utils::{
        RGB_RUNTIME_DIR, get_account_data, recipient_id_from_script_buf,
        script_buf_from_recipient_id,
    },
    wallet::{backup::*, multisig::*, rust_only::*},
};

const PROXY_HOST: &str = "127.0.0.1:3000/json-rpc";
static PROXY_ENDPOINT: Lazy<String> = Lazy::new(|| format!("rpc://{PROXY_HOST}"));
const TEST_DATA_DIR_PARTS: [&str; 2] = ["tests", "tmp"];
const PASSWORD: &str = "password";
const RESTORE_DIR_PARTS: [&str; 3] = ["tests", "tmp", "restored"];
const MAX_ALLOCATIONS_PER_UTXO: u32 = 5;

#[cfg(feature = "electrum")]
const PROXY_HOST_MOD_API: &str = "127.0.0.1:3002/json-rpc";
#[cfg(feature = "electrum")]
const PROXY_HOST_MOD_PROTO: &str = "127.0.0.1:3001/json-rpc";
#[cfg(any(feature = "electrum", feature = "esplora"))]
const PROXY_URL: &str = "http://127.0.0.1:3000/json-rpc";
#[cfg(feature = "electrum")]
const PROXY_URL_MOD_API: &str = "http://127.0.0.1:3002/json-rpc";
#[cfg(feature = "electrum")]
const PROXY_URL_MOD_PROTO: &str = "http://127.0.0.1:3001/json-rpc";
#[cfg(any(feature = "electrum", feature = "esplora"))]
static TRANSPORT_ENDPOINTS: Lazy<Vec<String>> = Lazy::new(|| vec![PROXY_ENDPOINT.clone()]);
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
#[cfg(any(feature = "electrum", feature = "esplora"))]
const TICKER: &str = "TICKER";
#[cfg(any(feature = "electrum", feature = "esplora"))]
const NAME: &str = "asset name";
#[cfg(feature = "electrum")]
const DETAILS: &str = "details with ℧nicode characters";
#[cfg(any(feature = "electrum", feature = "esplora"))]
const PRECISION: u8 = 7;
#[cfg(any(feature = "electrum", feature = "esplora"))]
const AMOUNT: u64 = 666;
#[cfg(feature = "electrum")]
const AMOUNT_INFLATION: u64 = 400;
#[cfg(any(feature = "electrum", feature = "esplora"))]
const AMOUNT_SMALL: u64 = 66;
#[cfg(any(feature = "electrum", feature = "esplora"))]
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
#[cfg(any(feature = "electrum", feature = "esplora"))]
const MIN_CONFIRMATIONS: u8 = 1;
#[cfg(feature = "electrum")]
const FAKE_TXID: &str = "e5a3e577309df31bd606f48049049d2e1e02b048206ba232944fcc053a176ccb";
#[cfg(feature = "electrum")]
const FAKE_OUTPOINT: &str = "e5a3e577309df31bd606f48049049d2e1e02b048206ba232944fcc053a176ccb:0";
#[cfg(feature = "electrum")]
const UNKNOWN_IDX: i32 = 9999;
#[cfg(feature = "electrum")]
const TINY_BTC_AMOUNT: u32 = 330;
#[cfg(any(feature = "electrum", feature = "esplora"))]
const QUEUE_DEPTH_EXCEEDED: &str = "Work queue depth exceeded";
#[cfg(any(feature = "electrum", feature = "esplora"))]
const DURATION_RCV_TRANSFER: u32 = 86400;
#[cfg(any(feature = "electrum", feature = "esplora"))]
const DURATION_SEND_TRANSFER: u32 = 3600;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) const INDEXER_SYNC_LOOKBACK: usize = 20;

#[cfg(all(feature = "esplora", not(feature = "electrum")))]
const DEFAULT_INDEXER_URL: &str = ESPLORA_URL;
#[cfg(feature = "electrum")]
const DEFAULT_INDEXER_URL: &str = ELECTRUM_URL;

#[cfg(any(feature = "electrum", feature = "esplora"))]
static INIT: Once = Once::new();

thread_local! {
    pub(crate) static MOCK_CHAIN_NET: RefCell<Option<ChainNet>> = const { RefCell::new(None) };
    pub(crate) static MOCK_CONTRACT_DATA: RefCell<Vec<Attachment>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_CONTRACT_DETAILS: RefCell<Option<String>> = const { RefCell::new(None) };
    pub(crate) static MOCK_TOKEN_DATA: RefCell<Vec<TokenData>> = const { RefCell::new(vec![]) };
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
thread_local! {
    pub(crate) static MOCK_CHECK_FEE_RATE: RefCell<Vec<bool>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_INPUT_UNSPENTS: RefCell<Vec<LocalUnspent>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_SKIP_BUILD_DAG: RefCell<Option<()>> = const { RefCell::new(None) };
    pub(crate) static MOCK_VOUT: RefCell<Option<u32>> = const { RefCell::new(None) };
    pub(crate) static MOCK_LOCAL_VERSION: RefCell<Option<String>> = const { RefCell::new(None) };
    pub(crate) static MOCK_SEND_END_CRASH: RefCell<Option<()>> = const { RefCell::new(None) };
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
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

#[cfg(feature = "electrum")]
pub fn restart_multisig_hub() {
    let service_name = "rgb-multisig-hub";
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
        .expect("failed to remove hub service");
    Command::new("docker")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .arg("volume")
        .arg("rm")
        .arg("tests_hub")
        .output()
        .expect("failed to remove hub volume");
    let mut cmd = cmd_base.clone();
    cmd.extend([s!("up"), s!("-d"), service_name.to_string()]);
    Command::new("docker")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .arg("compose")
        .args(&cmd)
        .output()
        .expect("failed to start hub service");
}

// the get_*_wallet! macros can be called with no arguments to use defaults
#[cfg(any(feature = "electrum", feature = "esplora"))]
macro_rules! get_empty_party {
    ($i: expr) => {
        get_empty_party(true, Some($i))
    };
    () => {
        get_empty_party(true, None)
    };
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
macro_rules! get_funded_noutxo_party {
    ($i: expr) => {
        get_funded_noutxo_party(true, Some($i))
    };
    () => {
        get_funded_noutxo_party(true, None)
    };
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
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

#[cfg(any(feature = "electrum", feature = "esplora"))]
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

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn mock_local_version(version: &str) -> String {
    let mock = MOCK_LOCAL_VERSION.take();
    if let Some(mock) = mock {
        println!("mocking local version");
        mock
    } else {
        version.to_string()
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
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

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn skip_build_dag() -> bool {
    if MOCK_SKIP_BUILD_DAG.take().is_none() {
        false
    } else {
        println!("skipping check dag (mock)");
        true
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn mock_send_end_crash() -> bool {
    if MOCK_SEND_END_CRASH.take().is_none() {
        false
    } else {
        println!("simulating send_end crash (mock)");
        true
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn mock_vout(vout: Option<u32>) -> Option<u32> {
    let mock = MOCK_VOUT.take();
    if mock.is_some() {
        println!("mocking vout");
        assert_ne!(mock, vout);
        mock
    } else {
        vout
    }
}

// test utilities
#[macro_use]
mod utils;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) use utils::chain::*;
pub(crate) use utils::{api::*, helpers::*};

// API tests
#[cfg(feature = "electrum")]
mod abort_pending_vanilla_tx;
#[cfg(feature = "electrum")]
mod backup;
mod blind_receive;
#[cfg(feature = "electrum")]
mod burn;
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
#[cfg(any(feature = "electrum", feature = "esplora"))]
mod get_fee_estimation;
mod get_wallet_data;
mod get_wallet_dir;
#[cfg(any(feature = "electrum", feature = "esplora"))]
mod go_online;
#[cfg(feature = "electrum")]
mod inflate;
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
#[cfg(feature = "electrum")]
mod multisig;
mod new;
#[cfg(feature = "electrum")]
mod refresh;
#[cfg(any(feature = "electrum", feature = "esplora"))]
mod rust_only;
#[cfg(any(feature = "electrum", feature = "esplora"))]
mod send;
#[cfg(feature = "electrum")]
mod send_btc;
#[cfg(feature = "electrum")]
mod sign_psbt;
#[cfg(feature = "electrum")]
mod sync;
#[cfg(feature = "electrum")]
mod witness_receive;
