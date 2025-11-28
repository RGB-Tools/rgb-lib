pub(crate) mod multisig_hub;
pub(crate) mod proxy;
pub(crate) mod reject_list;

use super::*;

const JSON: &str = "application/json";
const OCTET_STREAM: &str = "application/octet-stream";
const CONNECT_TIMEOUT: u64 = 10;
const READ_WRITE_TIMEOUT: u64 = 120;
