pub(crate) mod evm;
pub(crate) mod multisig_hub;
pub(crate) mod proxy;
pub(crate) mod reject_list;

use super::*;

const JSON: &str = "application/json";
const OCTET_STREAM: &str = "application/octet-stream";
const CONNECT_TIMEOUT: u64 = 10;
const READ_WRITE_TIMEOUT: u64 = 120;
/// Maximum number of entries of a reject list, whether kept by an EVM contract or served at a URL
///
/// The source a reject list is read from is chosen by the contract issuer, so it is not trusted: a
/// contract reporting a huge length and serving every page, or a server streaming a huge body,
/// would otherwise keep a client reading it (and holding it in memory) without bound, at every
/// check against the list.
const MAX_REJECT_LIST_ENTRIES: u64 = 100_000;
