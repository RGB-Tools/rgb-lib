use std::{
    ffi::OsString,
    io::Write,
    path::MAIN_SEPARATOR_STR,
    process::{Command, Stdio},
    sync::{Once, RwLock},
};

use bdk_wallet::descriptor::ExtendedDescriptor;
use once_cell::sync::Lazy;
use regex::RegexSet;
use rgbstd::stl::{EmbeddedMedia as RgbEmbeddedMedia, ProofOfReserves as RgbProofOfReserves};
use serde_json::Value;
use serial_test::{parallel, serial};
use std::{cell::RefCell, time::Instant};
use time::OffsetDateTime;

use super::*;

#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::wallet::{
    rust_only::{check_indexer_url, check_proxy_url},
    test::utils::chain::*,
    utils::build_indexer,
};
use crate::{
    database::entities::transfer_transport_endpoint,
    utils::{
        KEYCHAIN_BTC, KEYCHAIN_RGB, RGB_RUNTIME_DIR, get_account_data,
        get_account_derivation_children, get_coin_type, get_extended_derivation_path,
        recipient_id_from_script_buf, script_buf_from_recipient_id,
    },
    wallet::{
        backup::{BackupPubData, ScryptParams, get_backup_paths, unzip, zip_dir},
        rust_only::{AssetColoringInfo, ColoringInfo, IndexerProtocol},
        test::utils::{api::*, helpers::*},
    },
};

const PROXY_HOST: &str = "127.0.0.1:3000/json-rpc";
const PROXY_HOST_MOD_API: &str = "127.0.0.1:3002/json-rpc";
const PROXY_HOST_MOD_PROTO: &str = "127.0.0.1:3001/json-rpc";
const PROXY_URL: &str = "http://127.0.0.1:3000/json-rpc";
const PROXY_URL_MOD_API: &str = "http://127.0.0.1:3002/json-rpc";
const PROXY_URL_MOD_PROTO: &str = "http://127.0.0.1:3001/json-rpc";
static PROXY_ENDPOINT: Lazy<String> = Lazy::new(|| format!("rpc://{PROXY_HOST}"));
static TRANSPORT_ENDPOINTS: Lazy<Vec<String>> = Lazy::new(|| vec![PROXY_ENDPOINT.clone()]);
const ELECTRUM_URL: &str = "127.0.0.1:50001";
const ELECTRUM_2_URL: &str = "127.0.0.1:50002";
const ELECTRUM_BLOCKSTREAM_URL: &str = "127.0.0.1:50003";
const ELECTRUM_SIGNET_CUSTOM_URL: &str = "127.0.0.1:50005";
const ESPLORA_URL: &str = "http://127.0.0.1:8094/regtest/api";
const TEST_DATA_DIR_PARTS: [&str; 2] = ["tests", "tmp"];
const LISTS_DIR_PARTS: [&str; 2] = ["tests", "lists"];
const TICKER: &str = "TICKER";
const NAME: &str = "asset name";
const DETAILS: &str = "details with ℧nicode characters";
const PRECISION: u8 = 7;
const AMOUNT: u64 = 666;
const AMOUNT_INFLATION: u64 = 400;
const AMOUNT_SMALL: u64 = 66;
const FEE_RATE: u64 = 2;
const FEE_MSG_LOW: &str = "value under minimum 1";
const FEE_MSG_OVER: &str = "value overflows";
const EMPTY_MSG: &str = "must contain at least one character.";
const IDENT_EMPTY_MSG: &str = "ident must contain at least one character";
const IDENT_TOO_LONG_MSG: &str = "string has invalid length.";
const IDENT_NOT_ASCII_MSG: &str = "string '{0}' contains invalid character '{1}' at position {2}.";
const IDENT_NOT_START_MSG: &str = "string '{0}' must not start with character '{1}'.";
const RESTORE_DIR_PARTS: [&str; 3] = ["tests", "tmp", "restored"];
const MAX_ALLOCATIONS_PER_UTXO: u32 = 5;
const MIN_CONFIRMATIONS: u8 = 1;
const FAKE_TXID: &str = "e5a3e577309df31bd606f48049049d2e1e02b048206ba232944fcc053a176ccb:0";
const UNKNOWN_IDX: i32 = 9999;
#[cfg(feature = "electrum")]
const TINY_BTC_AMOUNT: u32 = 330;
const QUEUE_DEPTH_EXCEEDED: &str = "Work queue depth exceeded";

static INIT: Once = Once::new();

thread_local! {
    pub(crate) static MOCK_CHAIN_NET: RefCell<Option<ChainNet>> = const { RefCell::new(None) };
    pub(crate) static MOCK_CHECK_FEE_RATE: RefCell<Vec<bool>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_CONTRACT_DATA: RefCell<Vec<Attachment>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_CONTRACT_DETAILS: RefCell<Option<String>> = const { RefCell::new(None) };
    pub(crate) static MOCK_INPUT_UNSPENTS: RefCell<Vec<LocalUnspent>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_SKIP_BUILD_DAG: RefCell<Option<()>> = const { RefCell::new(None) };
    pub(crate) static MOCK_TOKEN_DATA: RefCell<Vec<TokenData>> = const { RefCell::new(vec![]) };
    pub(crate) static MOCK_VOUT: RefCell<Option<u32>> = const { RefCell::new(None) };
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

// the get_*_wallet! macros can be called with no arguments to use defaults
#[cfg(any(feature = "electrum", feature = "esplora"))]
macro_rules! get_empty_wallet {
    ($i: expr) => {
        get_empty_wallet(true, Some($i))
    };
    () => {
        get_empty_wallet(true, None)
    };
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
macro_rules! get_funded_noutxo_wallet {
    ($i: expr) => {
        get_funded_noutxo_wallet(true, Some($i))
    };
    () => {
        get_funded_noutxo_wallet(true, None)
    };
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
macro_rules! get_funded_wallet {
    ($i: expr) => {
        get_funded_wallet(true, Some($i))
    };
    () => {
        get_funded_wallet(true, None)
    };
}

pub fn mock_asset_terms(
    wallet: &Wallet,
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

pub fn mock_token_data(
    wallet: &Wallet,
    index: TokenIndex,
    media_data: &Option<(Attachment, Media)>,
    attachments: BTreeMap<u8, Attachment>,
) -> TokenData {
    MOCK_TOKEN_DATA.with_borrow_mut(|v| {
        if v.is_empty() {
            wallet.new_token_data(index, media_data, attachments)
        } else {
            println!("mocking token data");
            v.pop().unwrap()
        }
    })
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn mock_input_unspents(wallet: &Wallet, unspents: &[LocalUnspent]) -> Vec<LocalUnspent> {
    MOCK_INPUT_UNSPENTS.with_borrow_mut(|v| {
        if v.is_empty() {
            wallet.get_input_unspents(unspents).unwrap()
        } else {
            println!("mocking input unspents");
            std::mem::take(v)
        }
    })
}

pub fn mock_contract_details(wallet: &Wallet) -> Option<Details> {
    let mock = MOCK_CONTRACT_DETAILS.take();
    if let Some(details) = mock {
        println!("mocking contract details");
        Some(wallet.check_details(details.to_string()).unwrap())
    } else {
        None
    }
}

pub fn mock_chain_net(wallet: &Wallet) -> ChainNet {
    match MOCK_CHAIN_NET.take() {
        Some(chain_net) => {
            println!("mocking chain net");
            chain_net
        }
        None => wallet.bitcoin_network().into(),
    }
}

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

pub fn skip_build_dag() -> bool {
    if MOCK_SKIP_BUILD_DAG.take().is_none() {
        false
    } else {
        println!("skipping check dag (mock)");
        true
    }
}

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
mod utils;

// API tests
mod backup;
mod blind_receive;
mod create_utxos;
mod delete_transfers;
mod drain_to;
mod fail_transfers;
mod get_address;
mod get_asset_balance;
mod get_asset_metadata;
mod get_btc_balance;
mod get_fee_estimation;
mod get_wallet_data;
mod get_wallet_dir;
mod go_online;
mod inflate;
mod issue_asset_cfa;
mod issue_asset_ifa;
mod issue_asset_nia;
mod issue_asset_uda;
mod list_assets;
mod list_transactions;
mod list_transfers;
mod list_unspents;
mod new;
mod refresh;
mod rust_only;
mod send;
mod send_btc;
mod sign_psbt;
mod witness_receive;
