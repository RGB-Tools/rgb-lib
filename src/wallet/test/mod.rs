use amplify::s;
use lazy_static::lazy_static;
use once_cell::sync::Lazy;
use regex::RegexSet;
use std::process::{Command, Stdio};
use std::sync::{Mutex, Once, RwLock};
use time::OffsetDateTime;

use crate::generate_keys;

use super::*;
use utils::api::*;
use utils::chain::*;
use utils::helpers::*;

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
const TEST_DATA_DIR: &str = "./tests/tmp";
const TICKER: &str = "TICKER";
const NAME: &str = "asset name";
const DESCRIPTION: &str = "description with ℧nicode characters";
const PRECISION: u8 = 7;
const AMOUNT: u64 = 666;
const FEE_RATE: f32 = 1.5;
const FEE_MSG_LOW: &str = "value under minimum 1";
const FEE_MSG_HIGH: &str = "value above maximum 1000";
const IDENT_EMPTY_MSG: &str = "ident must contain at least one character";
const IDENT_TOO_LONG_MSG: &str = "identifier name has invalid length";
const IDENT_NOT_ASCII_MSG: &str = "identifier name contains non-ASCII character(s)";
const RESTORE_DIR: &str = "./tests/tmp/restored";
const MAX_ALLOCATIONS_PER_UTXO: u32 = 5;
const MIN_CONFIRMATIONS: u8 = 1;

static INIT: Once = Once::new();

pub fn initialize() {
    INIT.call_once(|| {
        println!("starting test services...");
        let status = Command::new("./tests/start_services.sh")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("failed to start test services");
        assert!(status.success());
    });
}

// the get_*_wallet! macros can be called with no arguments to use defaults
macro_rules! get_empty_wallet {
    ($p: expr, $k: expr) => {
        get_empty_wallet($p, $k)
    };
    () => {
        get_empty_wallet(false, true)
    };
}
macro_rules! get_funded_noutxo_wallet {
    ($p: expr, $k: expr) => {
        get_funded_noutxo_wallet($p, $k)
    };
    () => {
        get_funded_noutxo_wallet(false, true)
    };
}
macro_rules! get_funded_wallet {
    ($p: expr, $k: expr) => {
        get_funded_wallet($p, $k)
    };
    () => {
        get_funded_wallet(false, true)
    };
}

lazy_static! {
    static ref MOCK_CONTRACT_DATA: Mutex<Vec<Attachment>> = Mutex::new(vec![]);
}

pub fn mock_contract_data(terms: RicardianContract, media: Option<Attachment>) -> ContractData {
    let mut mock_reqs = MOCK_CONTRACT_DATA.lock().unwrap();
    if mock_reqs.is_empty() {
        ContractData { terms, media }
    } else {
        let mocked_media = mock_reqs.pop();
        ContractData {
            terms,
            media: mocked_media,
        }
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
mod go_online;
mod issue_asset_cfa;
mod issue_asset_nia;
mod list_assets;
mod list_transactions;
mod list_transfers;
mod list_unspents;
mod new;
mod refresh;
mod send;
mod send_btc;
mod witness_receive;
