use super::*;

/// Selector of `getFundsInBlock(uint256,uint256)` on a bridge contract (`MinimalBridge`)
const FUNDS_IN_BLOCK_SELECTOR: &str = "0x2ce06e2d";
/// Selector of `circulatingSupply()` on a bridge contract (`MinimalBridge`)
#[cfg(feature = "_online")]
const CIRCULATING_SUPPLY_SELECTOR: &str = "0x9358928b";

/// Selector of `length()` on a reject list contract (`IRgbRejectList`)
const REJECT_LIST_LENGTH_SELECTOR: &str = "0x1f7b6d32";
/// Selector of `entries(uint256,uint256)` on a reject list contract (`IRgbRejectList`)
const REJECT_LIST_ENTRIES_SELECTOR: &str = "0x17b937e6";
/// Maximum number of reject list entries requested with a single `entries` call
///
/// The contract doesn't bound pages, the `eth_call` gas cap and timeout of the node do.
const REJECT_LIST_PAGE_SIZE: u64 = 1000;
/// Number of times a JSON-RPC request is sent again when it fails for a temporary reason (see
/// [`EvmClient::post`])
const RPC_RETRIES: u32 = 2;
/// Pause before sending a JSON-RPC request again, multiplied by the number of the retry
#[cfg(not(test))]
const RPC_RETRY_PAUSE: Duration = Duration::from_secs(1);
#[cfg(test)]
const RPC_RETRY_PAUSE: Duration = Duration::from_millis(10);

/// The status of the lock confirming a mint, which has to be included on the external chain
/// before the block the mint commits to (`before_block`), as recorded by the bridge contract as of
/// the latest finalized block (see [`EvmClient::mint_lock_status`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MintLockStatus {
    /// The lock is recorded, included before `before_block`
    Confirmed,
    /// No lock is recorded yet, but one may still be included before `before_block`
    Pending,
    /// The mint will never be confirmed: the block at height `before_block - 1` is finalized and
    /// no lock is recorded in it or in a previous block
    Expired,
}

/// The block an `eth_call` reads the state as of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallBlock {
    /// The block with this number
    Number(u64),
    /// The latest block (`latest` tag), whichever it is when the node answers the call
    #[cfg(feature = "_online")]
    Latest,
}

impl CallBlock {
    /// The block parameter of the call: the hex-encoded block number or the tag.
    fn param(self) -> String {
        match self {
            Self::Number(block) => format!("0x{block:x}"),
            #[cfg(feature = "_online")]
            Self::Latest => s!("latest"),
        }
    }
}

impl From<u64> for CallBlock {
    fn from(block: u64) -> Self {
        Self::Number(block)
    }
}

/// JSON-RPC client for an EVM node.
pub struct EvmClient {
    client: RestClient,
    rpc_url: String,
}

/// Strip the `0x` prefix the JSON-RPC API puts on every hex-encoded value, refusing values
/// without it.
fn strip_hex_prefix<'a>(value: &'a str, what: &str) -> Result<&'a str, Error> {
    value.strip_prefix("0x").ok_or_else(|| Error::Network {
        details: format!("invalid EVM {what} {value}: missing 0x prefix"),
    })
}

/// Parse a hex-encoded (`0x`-prefixed) quantity.
fn parse_quantity(value: &str, what: &str) -> Result<u64, Error> {
    u64::from_str_radix(strip_hex_prefix(value, what)?, 16).map_err(|e| Error::Network {
        details: format!("invalid EVM {what} {value}: {e}"),
    })
}

/// Parse a hex-encoded (`0x`-prefixed) block number.
fn parse_block_number(hex_block: &str) -> Result<u64, Error> {
    parse_quantity(hex_block, "block number")
}

/// The parameters of a method taking none, serialized as an empty array: JSON-RPC 2.0 allows
/// `params` to be omitted or to be an array or an object, not `null` (as a unit struct would be).
const NO_PARAMS: [(); 0] = [];

/// Decode a hex-encoded ABI word (32 bytes) at the given word index from `data`.
/// `data` must start with "0x".
fn abi_word(data: &str, index: usize) -> Result<[u8; 32], Error> {
    // work on bytes: slicing the str could split a (bogus) multi-byte char and panic
    let hex = strip_hex_prefix(data, "ABI data")?.as_bytes();
    // the index can come from the data itself (e.g. an array offset)
    let end = index
        .checked_add(1)
        .and_then(|words| words.checked_mul(64))
        .ok_or_else(|| abi_error("word index out of range"))?;
    let start = end - 64;
    if hex.len() < end {
        return Err(Error::Network {
            details: format!("ABI data too short: expected at least {end} hex chars"),
        });
    }
    let mut buf = [0u8; 32];
    hex::decode_to_slice(&hex[start..end], &mut buf).map_err(|e| Error::Network {
        details: format!("ABI hex decode error: {e}"),
    })?;
    Ok(buf)
}

/// Decode a hex-encoded ABI word as a `u64` at the given word index from `data`.
/// Returns `None` if the uint256 value doesn't fit in a `u64`.
fn abi_word_u64(data: &str, index: usize) -> Result<Option<u64>, Error> {
    let word = abi_word(data, index)?;
    if word[..24].iter().any(|b| *b != 0) {
        return Ok(None);
    }
    Ok(Some(u64::from_be_bytes(word[24..32].try_into().unwrap())))
}

/// An entry of a reject list kept by an EVM contract (`IRgbRejectList.Entry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RejectListEntry {
    /// RGB operation output the decision applies to.
    pub opout: Opout,
    /// Whether the opout is rejected (`true`) or allowed (`false`).
    pub reject: bool,
}

/// The entries of a reject list kept by an EVM contract, in append order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RejectListLog {
    /// All the entries, as of the latest block.
    pub entries: Vec<RejectListEntry>,
    /// How many of the leading entries were appended in finalized blocks.
    pub finalized: usize,
}

impl RejectListLog {
    /// The opouts currently rejected and allowed, as `(reject, allow)`.
    ///
    /// The latest entry for an opout wins, but entries in non-finalized blocks could still be
    /// dropped or reordered by a reorg. So, erring on the safe side, a reject counts as soon as
    /// it's seen, while an allow counts only once finalized: an opout is rejected if its finalized
    /// decision is a reject or if any non-finalized entry rejects it, and allowed if its finalized
    /// decision is an allow and no non-finalized entry rejects it.
    pub(crate) fn decisions(&self) -> (HashSet<Opout>, HashSet<Opout>) {
        let (finalized, pending) = self.entries.split_at(self.finalized);
        let finalized = finalized
            .iter()
            .map(|e| (e.opout, e.reject))
            .collect::<HashMap<_, _>>();
        let pending_rejects = pending
            .iter()
            .filter(|e| e.reject)
            .map(|e| e.opout)
            .collect::<HashSet<_>>();
        let (mut reject, mut allow) = (pending_rejects, HashSet::new());
        for (opout, rejected) in finalized {
            if rejected {
                reject.insert(opout);
            } else if !reject.contains(&opout) {
                allow.insert(opout);
            }
        }
        (reject, allow)
    }
}

fn abi_error(details: &str) -> Error {
    Error::Network {
        details: format!("invalid ABI data: {details}"),
    }
}

/// Decode a hex-encoded ABI word as a `uint16` at the given word index from `data`.
fn abi_word_u16(data: &str, index: usize) -> Result<u16, Error> {
    abi_word_u64(data, index)?
        .and_then(|v| u16::try_from(v).ok())
        .ok_or_else(|| abi_error("bad uint16"))
}

/// Decode the result of `entries(uint256,uint256)`, an ABI-encoded
/// `(bytes32, uint16, uint16, bool)[]`: the offset of the array, its length, then each entry
/// inline as four words.
fn decode_reject_list_entries(data: &str) -> Result<Vec<RejectListEntry>, Error> {
    // the offset and the length come from the (untrusted) contract: both are checked against the
    // data size before being used, so that they can't make the indexes overflow
    let words = strip_hex_prefix(data, "ABI data")?.len() / 64;
    let len_index = abi_word_u64(data, 0)?
        .filter(|o| o % 32 == 0)
        .and_then(|o| usize::try_from(o / 32).ok())
        .filter(|i| *i < words)
        .ok_or_else(|| abi_error("bad array offset"))?;
    let len = abi_word_u64(data, len_index)?
        .and_then(|l| usize::try_from(l).ok())
        .filter(|l| {
            l.checked_mul(4)
                .and_then(|entry_words| entry_words.checked_add(len_index + 1))
                .is_some_and(|end| end <= words)
        })
        .ok_or_else(|| abi_error("bad array length"))?;
    let mut entries = vec![];
    for i in 0..len {
        let base = len_index + 1 + 4 * i;
        let opout = Opout {
            op: OpId::from(abi_word(data, base)?),
            ty: AssignmentType::with(abi_word_u16(data, base + 1)?),
            no: abi_word_u16(data, base + 2)?,
        };
        let reject = match abi_word_u64(data, base + 3)? {
            Some(0) => false,
            Some(1) => true,
            _ => return Err(abi_error("bad bool")),
        };
        entries.push(RejectListEntry { opout, reject });
    }
    Ok(entries)
}

/// ABI-encode a `uint256` argument as a hex word.
fn abi_encode_u64(value: u64) -> String {
    format!("{value:064x}")
}

/// JSON-RPC request envelope.
#[derive(Debug, Serialize)]
struct RpcRequest<P: Serialize> {
    jsonrpc: &'static str,
    method: &'static str,
    params: P,
    id: u64,
}

/// JSON-RPC response envelope, carrying either the result or the error of the request.
#[derive(Debug, Deserialize)]
struct RpcResponse<R> {
    result: Option<R>,
    error: Option<RpcError>,
}

/// Error object of a JSON-RPC response.
#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

/// Transaction call object for `eth_call`.
#[derive(Debug, Serialize)]
struct CallObject {
    /// Contract address to call.
    to: String,
    /// ABI-encoded call data (hex).
    data: String,
}

/// The part of a block header returned by `eth_getBlockByNumber` that is needed here.
#[derive(Debug, Deserialize)]
struct BlockHeader {
    /// Block number (hex).
    number: String,
}

impl EvmClient {
    pub(crate) fn new(rpc_url: &str) -> Result<Self, Error> {
        let client = RestClient::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT))
            .timeout(Duration::from_secs(READ_WRITE_TIMEOUT))
            .build()?;
        Ok(Self {
            client,
            rpc_url: rpc_url.to_string(),
        })
    }

    /// The endpoint of this client, as scheme and host (with the port, if any): hosted endpoints
    /// carry the API key in the path, the query or the user info, so the full URL must not end up
    /// in logs or errors.
    #[cfg(feature = "_online")]
    pub(crate) fn endpoint(&self) -> String {
        match reqwest::Url::parse(&self.rpc_url) {
            Ok(url) => {
                let host = url.host_str().unwrap_or_default();
                match url.port() {
                    Some(port) => format!("{}://{host}:{port}", url.scheme()),
                    None => format!("{}://{host}", url.scheme()),
                }
            }
            Err(_) => s!("<invalid URL>"),
        }
    }

    /// Report a failure of the HTTP client, without the URL it carries (see [`Self::endpoint`]).
    fn req_err(e: reqwest::Error) -> Error {
        Error::Network {
            details: format!("EVM RPC: {}", e.without_url()),
        }
    }

    /// Post a JSON-RPC request, sending it again (up to [`RPC_RETRIES`] times, after a pause)
    /// when it fails for a temporary reason: it cannot be sent (e.g. for a dropped connection or a
    /// failed DNS lookup) or the node answering it is temporarily unavailable (HTTP 502, 503 or
    /// 504). The requests of this client only read the chain, so they can be repeated.
    ///
    /// A timed out request is not sent again, as it already took long.
    fn post(&self, body: &impl Serialize) -> Result<Response, Error> {
        let mut retries = 0;
        loop {
            let res = self
                .client
                .post(&self.rpc_url)
                .header(CONTENT_TYPE, JSON)
                .json(body)
                .send();
            let temporary = match &res {
                Ok(resp) => matches!(
                    resp.status(),
                    StatusCode::BAD_GATEWAY
                        | StatusCode::SERVICE_UNAVAILABLE
                        | StatusCode::GATEWAY_TIMEOUT
                ),
                Err(e) => !e.is_timeout() && !e.is_builder(),
            };
            if !temporary || retries == RPC_RETRIES {
                return res.map_err(Self::req_err);
            }
            retries += 1;
            std::thread::sleep(RPC_RETRY_PAUSE * retries);
        }
    }

    /// The block the lock of `amount` with ID `lock_id` was included in, as recorded by the bridge
    /// contract at `contract` as of `block`, if any.
    ///
    /// The contract keeps a permanent record of each lock, keyed by the lock ID, which commits to
    /// the mint operation and its witness TX (see [`LockId`]), and the amount (`getFundsInBlock`),
    /// so a single call tells whether, and where, the mint was locked.
    /// An address with no code (calling it returns no data) or a contract without the function
    /// (calling it reverts) cannot have recorded any lock.
    fn funds_in_block(
        &self,
        contract: &EvmAddress,
        lock_id: &LockId,
        amount: u64,
        block: impl Into<CallBlock>,
    ) -> Result<Option<u64>, Error> {
        let data = format!(
            "{FUNDS_IN_BLOCK_SELECTOR}{}{}",
            hex::encode(lock_id.to_byte_array()),
            abi_encode_u64(amount)
        );
        let Some(result) = self.eth_call_unless_reverted(contract, data, block)? else {
            return Ok(None);
        };
        if result == "0x" {
            return Ok(None);
        }
        // compare the hex length exactly: a length rounded to whole words would let trailing
        // data through
        if strip_hex_prefix(&result, "ABI data")?.len() != 64 {
            return Err(abi_error("unexpected getFundsInBlock result size"));
        }
        // the record is the block plus one, zero meaning there's none
        let block_plus_one = abi_word_u64(&result, 0)?
            .ok_or_else(|| abi_error("lock block doesn't fit in a u64"))?;
        Ok(block_plus_one.checked_sub(1))
    }

    /// The status of the lock of `amount` with ID `lock_id`, which has to be included before
    /// `before_block`, as recorded by the bridge contract at `contract` as of `finalized_block`
    /// (see [`Self::finalized_block_number`]).
    ///
    /// The block is taken as an argument so that checking many locks doesn't read it again for
    /// each of them, which also keeps the checks consistent with each other.
    pub(crate) fn mint_lock_status(
        &self,
        contract: &EvmAddress,
        lock_id: &LockId,
        amount: u64,
        before_block: u64,
        finalized_block: u64,
    ) -> Result<MintLockStatus, Error> {
        Ok(
            match self.funds_in_block(contract, lock_id, amount, finalized_block)? {
                Some(block) if block < before_block => MintLockStatus::Confirmed,
                Some(_) => MintLockStatus::Expired,
                // a lock not recorded as of the finalized block can only be included after it
                None if finalized_block.saturating_add(1) >= before_block => {
                    MintLockStatus::Expired
                }
                None => MintLockStatus::Pending,
            },
        )
    }

    /// Whether the bridge contract at `contract` records, as of the latest block, a lock of
    /// `amount` with ID `lock_id` included before `before_block`, finalized or not
    /// (unlike [`Self::mint_lock_status`]).
    pub(crate) fn mint_lock_recorded(
        &self,
        contract: &EvmAddress,
        lock_id: &LockId,
        amount: u64,
        before_block: u64,
    ) -> Result<bool, Error> {
        Ok(matches!(
            self.funds_in_block(contract, lock_id, amount, CallBlock::Latest)?,
            Some(block) if block < before_block
        ))
    }

    /// The circulating supply accounted by the bridge contract at `contract` as of the latest
    /// block (`circulatingSupply`): the amount locked and not released yet.
    ///
    /// The supply is read as of the latest block, not the latest finalized one, so it reports what
    /// the bridge accounts right now, including the locks of blocks a reorg can still drop: it's
    /// an upper bound, which is the safer side for a supply.
    ///
    /// Returns `None` if the contract doesn't account it: an address with no code (calling it
    /// returns no data) or a contract without the function (calling it reverts).
    #[cfg(feature = "_online")]
    pub(crate) fn circulating_supply(&self, contract: &EvmAddress) -> Result<Option<u64>, Error> {
        let Some(result) = self.eth_call_unless_reverted(
            contract,
            CIRCULATING_SUPPLY_SELECTOR.to_string(),
            CallBlock::Latest,
        )?
        else {
            return Ok(None);
        };
        if result == "0x" {
            return Ok(None);
        }
        // compare the hex length exactly: a length rounded to whole words would let trailing
        // data through
        if strip_hex_prefix(&result, "ABI data")?.len() != 64 {
            return Err(abi_error("unexpected circulatingSupply result size"));
        }
        abi_word_u64(&result, 0)?
            .map(Some)
            .ok_or_else(|| abi_error("circulating supply doesn't fit in a u64"))
    }

    fn rpc_error(method: &str, err: RpcError) -> Error {
        Error::Network {
            details: format!("{method} error {}: {}", err.code, err.message),
        }
    }

    fn null_result(method: &str) -> Error {
        Error::Network {
            details: format!("{method} returned null result"),
        }
    }

    /// Call a JSON-RPC method, failing if it reports an error or no result.
    fn rpc<P: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<R, Error> {
        self.try_rpc(method, params)?
            .map_err(|err| Self::rpc_error(method, err))?
            .ok_or_else(|| Self::null_result(method))
    }

    /// Call a JSON-RPC method, returning the error it reports apart from the other failures, and
    /// its result, which can be null.
    fn try_rpc<P: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<Result<Option<R>, RpcError>, Error> {
        let body = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id: 1,
        };
        let resp: RpcResponse<R> = self.post(&body)?.json().map_err(Self::req_err)?;
        Ok(match resp.error {
            Some(err) => Err(err),
            None => Ok(resp.result),
        })
    }

    /// Number of the latest block (`eth_blockNumber`).
    pub(crate) fn block_number(&self) -> Result<u64, Error> {
        parse_block_number(&self.rpc::<_, String>("eth_blockNumber", NO_PARAMS)?)
    }

    /// The parameters of an `eth_call` of `contract` with `data` as of `block`.
    fn call_params(
        contract: &EvmAddress,
        data: String,
        block: impl Into<CallBlock>,
    ) -> (CallObject, String) {
        let call = CallObject {
            to: contract.to_string(),
            data,
        };
        (call, block.into().param())
    }

    /// Call `contract` with `data` (`eth_call`) as of `block`, returning the raw result.
    fn eth_call(
        &self,
        contract: &EvmAddress,
        data: String,
        block: impl Into<CallBlock>,
    ) -> Result<String, Error> {
        self.rpc("eth_call", Self::call_params(contract, data, block))
    }

    /// Call `contract` with `data` (`eth_call`) as of `block`, returning the raw result, or `None`
    /// if the call reverts.
    fn eth_call_unless_reverted(
        &self,
        contract: &EvmAddress,
        data: String,
        block: impl Into<CallBlock>,
    ) -> Result<Option<String>, Error> {
        let method = "eth_call";
        match self.try_rpc(method, Self::call_params(contract, data, block))? {
            Ok(result) => result.map(Some).ok_or_else(|| Self::null_result(method)),
            // nodes report a revert with this message, followed by the reason if there's one
            Err(err) if err.message.starts_with("execution reverted") => Ok(None),
            Err(err) => Err(Self::rpc_error(method, err)),
        }
    }

    /// All the entries of the reject list kept by the `IRgbRejectList` contract at `contract`,
    /// in append order, telling which ones are finalized.
    ///
    /// The list is append-only, so the entries as of the latest finalized block are the leading
    /// ones as of the latest block: only the length is read at the former, while the length and
    /// every page are read as of the latter, so that entries appended in the meantime cannot make
    /// the result inconsistent.
    pub(crate) fn reject_list_entries(
        &self,
        contract: &EvmAddress,
    ) -> Result<RejectListLog, Error> {
        self.reject_list_entries_paged(contract, REJECT_LIST_PAGE_SIZE)
    }

    /// Length of the reject list kept by the `IRgbRejectList` contract at `contract` as of `block`.
    ///
    /// Returns `None` if there is no contract at `contract` as of `block`: calling an address with
    /// no code returns no data.
    fn reject_list_length(&self, contract: &EvmAddress, block: u64) -> Result<Option<u64>, Error> {
        let len_data = self.eth_call(contract, REJECT_LIST_LENGTH_SELECTOR.to_string(), block)?;
        if len_data == "0x" {
            return Ok(None);
        }
        abi_word_u64(&len_data, 0)?
            .map(Some)
            .ok_or_else(|| abi_error("reject list length doesn't fit in a u64"))
    }

    fn reject_list_entries_paged(
        &self,
        contract: &EvmAddress,
        page_size: u64,
    ) -> Result<RejectListLog, Error> {
        let block = self.block_number()?;
        // a node behind a load balancer could report a finalized block past the latest one read
        let finalized_block = std::cmp::min(self.finalized_block_number()?, block);
        // a contract deployed in a non-finalized block has no finalized entries
        let finalized_len = self
            .reject_list_length(contract, finalized_block)?
            .unwrap_or(0);
        let len = self
            .reject_list_length(contract, block)?
            .ok_or_else(|| Error::Network {
                details: format!("no contract at the reject list address {contract}"),
            })?;
        if finalized_len > len {
            return Err(Error::Network {
                details: s!("reject list shorter at the latest block than at the finalized one"),
            });
        }
        if len > MAX_REJECT_LIST_ENTRIES {
            return Err(Error::Network {
                details: format!(
                    "reject list too long: {len} entries, at most {MAX_REJECT_LIST_ENTRIES} supported"
                ),
            });
        }
        let mut entries = vec![];
        let mut start = 0;
        while start < len {
            let end = std::cmp::min(start.saturating_add(page_size), len);
            let data = format!(
                "{REJECT_LIST_ENTRIES_SELECTOR}{}{}",
                abi_encode_u64(start),
                abi_encode_u64(end)
            );
            let page = decode_reject_list_entries(&self.eth_call(contract, data, block)?)?;
            if page.len() as u64 != end - start {
                return Err(abi_error("unexpected reject list page size"));
            }
            entries.extend(page);
            start = end;
        }
        Ok(RejectListLog {
            entries,
            finalized: finalized_len as usize,
        })
    }

    /// Number of the latest finalized block (`eth_getBlockByNumber` with the `finalized` tag).
    ///
    /// Finality is the chain's own notion of irreversibility, whatever its consensus: two
    /// justified epochs on Ethereum, a milestone on Polygon, a batch finalized on L1 for a
    /// rollup, and so on. A reorged-out mint lock would permanently invalidate the RGB-side
    /// history for every future receiver, so locks are only accepted once finalized.
    pub(crate) fn finalized_block_number(&self) -> Result<u64, Error> {
        let method = "eth_getBlockByNumber";
        let header: Option<BlockHeader> = self
            .try_rpc(method, ("finalized", false))?
            .map_err(|err| Self::rpc_error(method, err))?;
        let header = header.ok_or_else(|| Error::Network {
            details: s!("the node reports no finalized block"),
        })?;
        parse_block_number(&header.number)
    }

    /// Check that the node serves what confirming mints needs, failing with an
    /// [`Error::InvalidEvmRpcUrl`] if it doesn't: the latest finalized block and the contract
    /// state as of it, since locks are only accepted once finalized and are read from the records
    /// of the bridge contract.
    ///
    /// The finalized block can be older than the blocks whose state some nodes keep (e.g. pruned
    /// ones), which would make the confirmation of a mint fail later, when receiving an asset.
    pub(crate) fn check_mint_support(&self) -> Result<(), Error> {
        let finalized_block =
            self.finalized_block_number()
                .map_err(|e| Error::InvalidEvmRpcUrl {
                    details: format!("cannot get the latest finalized block: {e}"),
                })?;
        self.eth_call(&EvmAddress::default(), s!("0x"), finalized_block)
            .map_err(|e| Error::InvalidEvmRpcUrl {
                details: format!(
                    "cannot read the state as of the latest finalized block, an endpoint keeping \
                     it is needed: {e}"
                ),
            })?;
        Ok(())
    }

    /// EIP-155 chain ID of the chain the node is connected to (`eth_chainId`).
    pub(crate) fn chain_id(&self) -> Result<u64, Error> {
        parse_quantity(
            &self.rpc::<_, String>("eth_chainId", NO_PARAMS)?,
            "chain ID",
        )
    }
}

/// Report a JSON-RPC client failure as the claim resolver's issue.
///
/// The client only fails with [`Error::Network`]s: their details are reported as they are, to
/// avoid wrapping them in a second network error once mapped back to an [`Error`].
fn resolver_issue(e: Error) -> ClaimResolverError {
    match e {
        Error::Network { details } => ClaimResolverError::ResolverIssue(details),
        e => ClaimResolverError::ResolverIssue(e.to_string()),
    }
}

/// A claim resolver over an [`EvmClient`] recording the mints that will never be confirmed,
/// their lock not included before the block they commit to (see [`MintLockStatus::Expired`]).
pub(crate) struct MintClaimResolver<'a> {
    client: &'a EvmClient,
    /// Operation IDs of the mints that will never be confirmed
    expired: RefCell<HashSet<OpId>>,
    /// The finalized block the locks are read as of, read once for all the claims of a validation
    finalized_block: Cell<Option<u64>>,
}

impl<'a> MintClaimResolver<'a> {
    pub(crate) fn new(client: &'a EvmClient) -> Self {
        Self {
            client,
            expired: RefCell::new(HashSet::new()),
            finalized_block: Cell::new(None),
        }
    }

    /// The finalized block the locks are read as of, read from the node the first time only: a
    /// validation can carry many claims, and they are all read as of the same block.
    fn finalized_block(&self) -> Result<u64, Error> {
        if let Some(block) = self.finalized_block.get() {
            return Ok(block);
        }
        let block = self.client.finalized_block_number()?;
        self.finalized_block.set(Some(block));
        Ok(block)
    }

    /// Whether the mint with the given operation ID was found to never be confirmed.
    ///
    /// A mint that is not confirmed but not expired may still be, once its lock is made or
    /// finalized.
    pub(crate) fn is_expired(&self, opid: &OpId) -> bool {
        self.expired.borrow().contains(opid)
    }
}

impl ResolveClaim for MintClaimResolver<'_> {
    fn evm_chain_id(&self) -> Result<u64, ClaimResolverError> {
        self.client.chain_id().map_err(resolver_issue)
    }

    /// Whether `claim` is confirmed, as [`EvmClient`] resolves it, recording it if it expired.
    fn is_confirmed(
        &self,
        location: &BridgeLocation,
        claim: &Claim,
    ) -> Result<bool, ClaimResolverError> {
        let BridgeLocation::Evm(EvmContract { address, .. }) = location;
        let Claim::Lock {
            opid,
            amount,
            before_block,
            ..
        } = claim;
        let finalized_block = self.finalized_block().map_err(resolver_issue)?;
        let status = self
            .client
            .mint_lock_status(
                address,
                &LockId::from(claim),
                *amount,
                *before_block,
                finalized_block,
            )
            .map_err(resolver_issue)?;
        if status == MintLockStatus::Expired {
            self.expired.borrow_mut().insert(*opid);
        }
        Ok(status == MintLockStatus::Confirmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An opout whose operation ID is filled with `b`, with an assignment type and number derived
    /// from `b` too, all different, so that decoding them in the wrong order is caught.
    fn test_opout(b: u8) -> Opout {
        Opout {
            op: OpId::from([b; 32]),
            ty: AssignmentType::with(4000 + b as u16),
            no: 100 + b as u16,
        }
    }

    fn test_entry(b: u8, reject: bool) -> RejectListEntry {
        RejectListEntry {
            opout: test_opout(b),
            reject,
        }
    }

    #[test]
    fn reject_list_entries() {
        // abi.encode of [(0x01..01, 4001, 101, true), (0x02..02, 4002, 102, false)]
        let page = format!(
            "0x{:064x}{:064x}{}{:064x}{:064x}{:064x}{}{:064x}{:064x}{:064x}",
            32,
            2,
            "01".repeat(32),
            4001,
            101,
            1,
            "02".repeat(32),
            4002,
            102,
            0
        );
        assert_eq!(page, encode_entries(&[(1, true), (2, false)]));
        let expected = vec![test_entry(1, true), test_entry(2, false)];
        assert_eq!(decode_reject_list_entries(&page).unwrap(), expected);
        // an empty page
        let empty = format!("0x{:064x}{:064x}", 32, 0);
        assert!(decode_reject_list_entries(&empty).unwrap().is_empty());
        // a bool word other than 0 or 1 is refused
        let bad_bool = format!(
            "0x{:064x}{:064x}{}{:064x}{:064x}{:064x}",
            32,
            1,
            "01".repeat(32),
            1,
            0,
            2
        );
        assert!(decode_reject_list_entries(&bad_bool).is_err());
        // an assignment type or output number not fitting a uint16 is refused
        for (ty, no) in [(1 << 16, 0), (0, 1 << 16)] {
            let bad_u16 = format!(
                "0x{:064x}{:064x}{}{:064x}{:064x}{:064x}",
                32,
                1,
                "01".repeat(32),
                ty,
                no,
                1
            );
            let result = decode_reject_list_entries(&bad_u16);
            assert!(
                matches!(result, Err(Error::Network { ref details }) if details.contains("bad uint16")),
                "{result:?}"
            );
        }
        // a truncated page is refused
        assert!(decode_reject_list_entries(&page[..page.len() - 64]).is_err());
        // an offset or a length beyond the data is refused, without overflowing the word indexes
        for (offset, len) in [(1u64 << 63, 0u64), (u64::MAX - 31, 0), (32 * 3, 0)] {
            let bad_offset = format!("0x{offset:064x}{len:064x}");
            let result = decode_reject_list_entries(&bad_offset);
            assert!(
                matches!(result, Err(Error::Network { ref details }) if details.contains("bad array offset")),
                "{result:?}"
            );
        }
        for len in [1u64 << 62, u64::MAX, 1] {
            let bad_len = format!("0x{:064x}{len:064x}", 32);
            let result = decode_reject_list_entries(&bad_len);
            assert!(
                matches!(result, Err(Error::Network { ref details }) if details.contains("bad array length")),
                "{result:?}"
            );
        }
        assert!(abi_word("0x", usize::MAX).is_err());

        let mut server = mockito::Server::new();
        let block_number = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_blockNumber","params":[]}"#.to_string(),
            ))
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":"0x10"}"#)
            .create();
        let finalized_block = mock_finalized_block(&mut server, 8);
        let finalized_length = mock_eth_call_at(
            &mut server,
            8,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 1),
        );
        let length = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"method":"eth_call","params":[{{"data":"{REJECT_LIST_LENGTH_SELECTOR}"}},"0x10"]}}"#
            )))
            .with_body(format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":"0x{:064x}"}}"#,
                2
            ))
            .create();
        let entries = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"method":"eth_call","params":[{{"data":"{REJECT_LIST_ENTRIES_SELECTOR}{:064x}{:064x}"}},"0x10"]}}"#,
                0, 2
            )))
            .with_body(format!(r#"{{"jsonrpc":"2.0","id":1,"result":"{page}"}}"#))
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(
            client.reject_list_entries(&EvmAddress::default()).unwrap(),
            RejectListLog {
                entries: expected,
                finalized: 1,
            }
        );
        block_number.assert();
        finalized_block.assert();
        finalized_length.assert();
        length.assert();
        entries.assert();
    }

    /// ABI-encode reject list entries as returned by `entries`, each entry being the byte its
    /// opout is derived from (see `test_opout`) and whether it is rejected.
    fn encode_entries(entries: &[(u8, bool)]) -> String {
        let mut data = format!("0x{:064x}{:064x}", 32, entries.len());
        for (b, reject) in entries {
            let opout = test_opout(*b);
            data.push_str(&hex::encode(opout.op.to_byte_array()));
            data.push_str(&format!("{:064x}", opout.ty.to_inner()));
            data.push_str(&format!("{:064x}", opout.no));
            data.push_str(&format!("{:064x}", *reject as u8));
        }
        data
    }

    /// Mock an `eth_call` at block 0x10 (the latest one) with the given call `data`, returning
    /// `result`.
    fn mock_eth_call(server: &mut mockito::Server, data: &str, result: &str) -> mockito::Mock {
        mock_eth_call_at(server, 0x10, data, result)
    }

    /// Mock an `eth_call` at `block` with the given call `data`, returning `result`.
    fn mock_eth_call_at(
        server: &mut mockito::Server,
        block: impl Into<CallBlock>,
        data: &str,
        result: &str,
    ) -> mockito::Mock {
        mock_eth_call_hits(server, block, data, result, 1)
    }

    /// Mock an `eth_call` at `block` with the given call `data`, expected `hits` times, returning
    /// `result`.
    fn mock_eth_call_hits(
        server: &mut mockito::Server,
        block: impl Into<CallBlock>,
        data: &str,
        result: &str,
        hits: usize,
    ) -> mockito::Mock {
        server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"method":"eth_call","params":[{{"data":"{data}"}},"{}"]}}"#,
                block.into().param()
            )))
            .with_body(format!(r#"{{"jsonrpc":"2.0","id":1,"result":"{result}"}}"#))
            .expect(hits)
            .create()
    }

    fn mock_block_number(server: &mut mockito::Server) -> mockito::Mock {
        server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_blockNumber","params":[]}"#.to_string(),
            ))
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":"0x10"}"#)
            .create()
    }

    fn entries_call(start: u64, end: u64) -> String {
        format!("{REJECT_LIST_ENTRIES_SELECTOR}{start:064x}{end:064x}")
    }

    #[test]
    fn reject_list_entries_pages() {
        let all: Vec<(u8, bool)> = (1..=5).map(|i| (i, i % 2 == 0)).collect();
        let expected: Vec<RejectListEntry> = all
            .iter()
            .map(|(b, reject)| test_entry(*b, *reject))
            .collect();

        // 5 entries in pages of 2: [0, 2), [2, 4) and the last partial one [4, 5), all read as
        // of the block read first, the first 3 being finalized
        let mut server = mockito::Server::new();
        let _block_number = mock_block_number(&mut server);
        let _finalized_block = mock_finalized_block(&mut server, 8);
        let finalized_length = mock_eth_call_at(
            &mut server,
            8,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 3),
        );
        let length = mock_eth_call(
            &mut server,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 5),
        );
        let pages = [(0, 2), (2, 4), (4, 5)].map(|(start, end)| {
            mock_eth_call(
                &mut server,
                &entries_call(start, end),
                &encode_entries(&all[start as usize..end as usize]),
            )
        });
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(
            client
                .reject_list_entries_paged(&EvmAddress::default(), 2)
                .unwrap(),
            RejectListLog {
                entries: expected,
                finalized: 3,
            }
        );
        finalized_length.assert();
        length.assert();
        pages.iter().for_each(|p| p.assert());

        // a page with fewer entries than requested is refused instead of leaving a gap
        let mut server = mockito::Server::new();
        let _block_number = mock_block_number(&mut server);
        let _finalized_block = mock_finalized_block(&mut server, 8);
        let _finalized_length = mock_eth_call_at(
            &mut server,
            8,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 0),
        );
        let _length = mock_eth_call(
            &mut server,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 5),
        );
        let _page = mock_eth_call(&mut server, &entries_call(0, 2), &encode_entries(&all[..1]));
        let client = EvmClient::new(&server.url()).unwrap();
        let result = client.reject_list_entries_paged(&EvmAddress::default(), 2);
        assert!(
            matches!(result, Err(Error::Network { details }) if details.contains("unexpected reject list page size"))
        );

        // a finalized block past the latest one is taken as the latest one
        let mut server = mockito::Server::new();
        let _block_number = mock_block_number(&mut server);
        let _finalized_block = mock_finalized_block(&mut server, 0x20);
        let _length = mock_eth_call(
            &mut server,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 0),
        )
        .expect(2);
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(
            client
                .reject_list_entries_paged(&EvmAddress::default(), 2)
                .unwrap(),
            RejectListLog {
                entries: vec![],
                finalized: 0,
            }
        );

        // a list shorter at the latest block than at the finalized one is refused
        let mut server = mockito::Server::new();
        let _block_number = mock_block_number(&mut server);
        let _finalized_block = mock_finalized_block(&mut server, 8);
        let _finalized_length = mock_eth_call_at(
            &mut server,
            8,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 2),
        );
        let _length = mock_eth_call(
            &mut server,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 1),
        );
        let client = EvmClient::new(&server.url()).unwrap();
        let result = client.reject_list_entries_paged(&EvmAddress::default(), 2);
        assert!(
            matches!(result, Err(Error::Network { ref details }) if details.contains("reject list shorter at the latest block")),
            "{result:?}"
        );
    }

    #[test]
    fn reject_list_decisions() {
        let log = |entries: &[(u8, bool)], finalized: usize| RejectListLog {
            entries: entries
                .iter()
                .map(|(b, reject)| test_entry(*b, *reject))
                .collect(),
            finalized,
        };
        let set = |bytes: &[u8]| bytes.iter().map(|b| test_opout(*b)).collect::<HashSet<_>>();

        // finalized decisions hold, the latest entry for an opout winning
        let (reject, allow) = log(&[(1, true), (2, false), (3, false), (3, true)], 4).decisions();
        assert_eq!(reject, set(&[1, 3]));
        assert_eq!(allow, set(&[2]));

        // non-finalized rejects count right away, superseding finalized allows
        let (reject, allow) = log(&[(2, false), (1, true), (2, true)], 1).decisions();
        assert_eq!(reject, set(&[1, 2]));
        assert!(allow.is_empty());

        // non-finalized allows don't count: an opout allowed only by them is not listed, and
        // one they would allow again stays rejected
        let (reject, allow) = log(&[(1, true), (2, false), (1, false)], 1).decisions();
        assert_eq!(reject, set(&[1]));
        assert!(allow.is_empty());

        // a non-finalized reject counts even if a later non-finalized allow supersedes it, as a
        // reorg could drop the allow or move it before the reject
        let (reject, allow) = log(
            &[(1, false), (1, true), (1, false), (2, true), (2, false)],
            1,
        )
        .decisions();
        assert_eq!(reject, set(&[1, 2]));
        assert!(allow.is_empty());

        // once finalized, the latest entry wins again
        let (reject, allow) = log(
            &[(1, false), (1, true), (1, false), (2, true), (2, false)],
            5,
        )
        .decisions();
        assert!(reject.is_empty());
        assert_eq!(allow, set(&[1, 2]));

        // decisions are about single opouts: rejecting an output of an operation leaves its other
        // outputs alone
        let rejected = test_opout(1);
        let sibling = Opout {
            no: rejected.no + 1,
            ..rejected
        };
        let (reject, allow) = RejectListLog {
            entries: vec![RejectListEntry {
                opout: rejected,
                reject: true,
            }],
            finalized: 1,
        }
        .decisions();
        assert_eq!(reject, HashSet::from([rejected]));
        assert!(!reject.contains(&sibling));
        assert!(allow.is_empty());
    }

    #[test]
    fn reject_list_entries_too_long() {
        // a list longer than supported is refused before reading any page
        let mut server = mockito::Server::new();
        let _block_number = mock_block_number(&mut server);
        let _finalized_block = mock_finalized_block(&mut server, 8);
        let _finalized_length = mock_eth_call_at(&mut server, 8, REJECT_LIST_LENGTH_SELECTOR, "0x");
        let length = mock_eth_call(
            &mut server,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", MAX_REJECT_LIST_ENTRIES + 1),
        );
        let entries = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"params":[{{"data":"{REJECT_LIST_ENTRIES_SELECTOR}"}}]}}"#
            )))
            .expect(0)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        let result = client.reject_list_entries(&EvmAddress::default());
        assert!(
            matches!(result, Err(Error::Network { ref details }) if details.contains("reject list too long")),
            "{result:?}"
        );
        length.assert();
        entries.assert();
    }

    #[test]
    fn reject_list_entries_not_a_contract() {
        // calling an address with no code returns no data: refused, not taken as an empty list
        let mut server = mockito::Server::new();
        let _block_number = mock_block_number(&mut server);
        let _finalized_block = mock_finalized_block(&mut server, 8);
        let _finalized_length = mock_eth_call_at(&mut server, 8, REJECT_LIST_LENGTH_SELECTOR, "0x");
        let length = mock_eth_call(&mut server, REJECT_LIST_LENGTH_SELECTOR, "0x");
        let entries = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"params":[{{"data":"{REJECT_LIST_ENTRIES_SELECTOR}"}}]}}"#
            )))
            .expect(0)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        let result = client.reject_list_entries(&EvmAddress::default());
        assert!(
            matches!(result, Err(Error::Network { ref details }) if details.contains("no contract at the reject list address")),
            "{result:?}"
        );
        length.assert();
        entries.assert();

        // a contract deployed after the finalized block has no finalized entries
        let mut server = mockito::Server::new();
        let _block_number = mock_block_number(&mut server);
        let _finalized_block = mock_finalized_block(&mut server, 8);
        let _finalized_length = mock_eth_call_at(&mut server, 8, REJECT_LIST_LENGTH_SELECTOR, "0x");
        let _length = mock_eth_call(
            &mut server,
            REJECT_LIST_LENGTH_SELECTOR,
            &format!("0x{:064x}", 1),
        );
        let _page = mock_eth_call(
            &mut server,
            &entries_call(0, 1),
            &encode_entries(&[(1, true)]),
        );
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(
            client.reject_list_entries(&EvmAddress::default()).unwrap(),
            RejectListLog {
                entries: vec![test_entry(1, true)],
                finalized: 0,
            }
        );

        // a call reverting (e.g. a contract without `length()`) is refused too
        let mut server = mockito::Server::new();
        let _block_number = mock_block_number(&mut server);
        let _finalized_block = mock_finalized_block(&mut server, 8);
        let _length = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_call"}"#.to_string(),
            ))
            .with_body(
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":3,"message":"execution reverted"}}"#,
            )
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        let result = client.reject_list_entries(&EvmAddress::default());
        assert!(
            matches!(result, Err(Error::Network { ref details }) if details.contains("execution reverted")),
            "{result:?}"
        );
    }

    #[test]
    fn finalized_block_number() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_getBlockByNumber","params":["finalized",false]}"#.to_string(),
            ))
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"number":"0x1a4","hash":"0x00"}}"#)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(client.finalized_block_number().unwrap(), 420);
        mock.assert();

        // a node with no finalized block (e.g. not supporting the tag) is reported, never
        // silently taken as "nothing finalized yet"
        let _mock = server
            .mock("POST", "/")
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":null}"#)
            .create();
        let result = EvmClient::new(&server.url())
            .unwrap()
            .finalized_block_number();
        assert!(
            matches!(result, Err(Error::Network { details }) if details.contains("no finalized block"))
        );
    }

    #[test]
    fn chain_id() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_chainId","params":[]}"#.to_string(),
            ))
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":"0x7a69"}"#)
            .expect(2)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(client.chain_id().unwrap(), 31337);
        assert_eq!(
            MintClaimResolver::new(&client).evm_chain_id().unwrap(),
            31337
        );
        mock.assert();

        // an RPC error is reported as the resolver's issue, with its details
        let _mock = server
            .mock("POST", "/")
            .with_body(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"not found"}}"#)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        let result = MintClaimResolver::new(&client).evm_chain_id();
        assert_eq!(
            result,
            Err(ClaimResolverError::ResolverIssue(s!(
                "eth_chainId error -32601: not found"
            )))
        );
    }

    #[test]
    fn post_retries() {
        let chain_id_mock = |server: &mut mockito::Server, status: usize, hits: usize| {
            server
                .mock("POST", "/")
                .match_body(mockito::Matcher::PartialJsonString(
                    r#"{"method":"eth_chainId","params":[]}"#.to_string(),
                ))
                .with_status(status)
                .with_body(r#"{"jsonrpc":"2.0","id":1,"result":"0x7a69"}"#)
                .expect(hits)
                .create()
        };

        // a node temporarily unavailable is asked again
        for status in [502, 503, 504] {
            let mut server = mockito::Server::new();
            let unavailable = chain_id_mock(&mut server, status, RPC_RETRIES as usize);
            let available = chain_id_mock(&mut server, 200, 1);
            let client = EvmClient::new(&server.url()).unwrap();
            assert_eq!(client.chain_id().unwrap(), 31337);
            unavailable.assert();
            available.assert();
        }

        // up to RPC_RETRIES times, then its answer is handled as any other
        let mut server = mockito::Server::new();
        let unavailable = server
            .mock("POST", "/")
            .with_status(503)
            .with_body("<html>503 Service Temporarily Unavailable</html>")
            .expect(RPC_RETRIES as usize + 1)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        assert!(matches!(client.chain_id(), Err(Error::Network { .. })));
        unavailable.assert();

        // other errors are not retried
        let mut server = mockito::Server::new();
        let refused = chain_id_mock(&mut server, 500, 1);
        let client = EvmClient::new(&server.url()).unwrap();
        client.chain_id().unwrap();
        refused.assert();

        // a request that cannot be sent is sent again, then reported
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let client = EvmClient::new(&url).unwrap();
        let start = std::time::Instant::now();
        assert!(matches!(client.chain_id(), Err(Error::Network { .. })));
        let pauses: u32 = (1..=RPC_RETRIES).sum();
        assert!(start.elapsed() >= RPC_RETRY_PAUSE * pauses);
    }

    #[cfg(feature = "_online")]
    #[test]
    fn endpoint_hides_api_key() {
        let endpoint = |url: &str| EvmClient::new(url).unwrap().endpoint();
        assert_eq!(
            endpoint("https://mainnet.infura.io/v3/SECRET"),
            "https://mainnet.infura.io"
        );
        assert_eq!(
            endpoint("https://user:SECRET@node.example:8545/?key=SECRET"),
            "https://node.example:8545"
        );
        assert_eq!(endpoint("not a URL SECRET"), "<invalid URL>");

        // the errors of the HTTP client don't carry the URL either
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v3/SECRET", listener.local_addr().unwrap());
        drop(listener);
        let result = EvmClient::new(&url).unwrap().chain_id();
        assert!(
            matches!(result, Err(Error::Network { ref details }) if !details.contains("SECRET")),
            "{result:?}"
        );
    }

    /// The `getFundsInBlock` call data for the lock with ID `lock_id` and `amount`.
    fn funds_in_call(lock_id: &LockId, amount: u64) -> String {
        format!(
            "{FUNDS_IN_BLOCK_SELECTOR}{}{amount:064x}",
            hex::encode(lock_id.to_byte_array())
        )
    }

    /// Mock an `eth_getBlockByNumber` reporting `finalized` as the finalized block.
    fn mock_finalized_block(server: &mut mockito::Server, finalized: u64) -> mockito::Mock {
        server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_getBlockByNumber","params":["finalized",false]}"#.to_string(),
            ))
            .with_body(format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"number":"0x{finalized:x}"}}}}"#
            ))
            .create()
    }

    #[test]
    fn funds_in_block() {
        let contract = EvmAddress::default();
        let lock_id = LockId::from([1u8; 32]);
        let funds_in_block = |result: &str| {
            let mut server = mockito::Server::new();
            let call = mock_eth_call(&mut server, &funds_in_call(&lock_id, 7), result);
            let block = EvmClient::new(&server.url())
                .unwrap()
                .funds_in_block(&contract, &lock_id, 7, 0x10);
            call.assert();
            block
        };

        // the record is the block plus one
        assert_eq!(
            funds_in_block(&format!("0x{:064x}", 0x12)).unwrap(),
            Some(0x11)
        );
        assert_eq!(funds_in_block(&format!("0x{:064x}", 1)).unwrap(), Some(0));
        // zero means there's no record
        assert_eq!(funds_in_block(&format!("0x{:064x}", 0)).unwrap(), None);
        // an address with no code has no records
        assert_eq!(funds_in_block("0x").unwrap(), None);
        // nor a contract without the function, which reverts the call, while another error isn't
        // taken for a missing record
        let call_error = |error: &str| {
            let mut server = mockito::Server::new();
            let call = server
                .mock("POST", "/")
                .match_body(mockito::Matcher::PartialJsonString(s!(
                    r#"{"method":"eth_call"}"#
                )))
                .with_body(format!(r#"{{"jsonrpc":"2.0","id":1,"error":{error}}}"#))
                .create();
            let block = EvmClient::new(&server.url())
                .unwrap()
                .funds_in_block(&contract, &lock_id, 7, 0x10);
            call.assert();
            block
        };
        assert_eq!(
            call_error(r#"{"code":3,"message":"execution reverted"}"#).unwrap(),
            None
        );
        assert_eq!(
            call_error(r#"{"code":-32000,"message":"execution reverted: paused"}"#).unwrap(),
            None
        );
        assert!(call_error(r#"{"code":-32000,"message":"missing trie node"}"#).is_err());
        // a result that isn't a single word is refused, whatever its content
        assert!(funds_in_block(&format!("0x{:062x}", 1)).is_err());
        assert!(funds_in_block(&format!("0x{:064x}{:064x}", 1, 0)).is_err());
        // a block that doesn't fit in a u64 is refused, not truncated
        assert!(funds_in_block(&format!("0x{}1{}", "0".repeat(47), "0".repeat(16))).is_err());
    }

    #[cfg(feature = "_online")]
    #[test]
    fn circulating_supply() {
        let contract = EvmAddress::default();
        // the supply read as of the latest block, with the `latest` tag, without asking for the
        // finalized block first
        let circulating_supply = |result: &str| {
            let mut server = mockito::Server::new();
            let finalized = mock_finalized_block(&mut server, 0x20).expect(0);
            let call = mock_eth_call_at(
                &mut server,
                CallBlock::Latest,
                CIRCULATING_SUPPLY_SELECTOR,
                result,
            );
            let supply = EvmClient::new(&server.url())
                .unwrap()
                .circulating_supply(&contract);
            call.assert();
            finalized.assert();
            supply
        };

        assert_eq!(
            circulating_supply(&format!("0x{:064x}", 0)).unwrap(),
            Some(0)
        );
        assert_eq!(
            circulating_supply(&format!("0x{:064x}", u64::MAX)).unwrap(),
            Some(u64::MAX)
        );
        // an address with no code doesn't account it
        assert_eq!(circulating_supply("0x").unwrap(), None);
        // nor a contract without the function, which reverts the call
        let mut server = mockito::Server::new();
        let call = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(s!(
                r#"{"method":"eth_call"}"#
            )))
            .with_body(
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":3,"message":"execution reverted"}}"#,
            )
            .create();
        let supply = EvmClient::new(&server.url())
            .unwrap()
            .circulating_supply(&contract);
        call.assert();
        assert_eq!(supply.unwrap(), None);
        // a result that isn't a single word is refused, whatever its content
        assert!(circulating_supply(&format!("0x{:062x}", 1)).is_err());
        assert!(circulating_supply(&format!("0x{:064x}{:064x}", 1, 0)).is_err());
        // a supply that doesn't fit in a u64 is refused, not truncated
        assert!(circulating_supply(&format!("0x{}1{}", "0".repeat(47), "0".repeat(16))).is_err());
    }

    #[test]
    fn mint_lock_status() {
        let contract = EvmAddress::default();
        let lock_id = LockId::from([1u8; 32]);
        // the status of the lock, with the given record as of the finalized block 0x20, which the
        // caller provides
        let status = |record: u64, before_block: u64| {
            let mut server = mockito::Server::new();
            let finalized = mock_finalized_block(&mut server, 0x20).expect(0);
            let call = mock_eth_call_at(
                &mut server,
                0x20,
                &funds_in_call(&lock_id, 7),
                &format!("0x{record:064x}"),
            );
            let status = EvmClient::new(&server.url())
                .unwrap()
                .mint_lock_status(&contract, &lock_id, 7, before_block, 0x20)
                .unwrap();
            call.assert();
            finalized.assert();
            status
        };

        // a lock included in block 0x11 confirms the mint only if that's before the block
        assert_eq!(status(0x12, 0x12), MintLockStatus::Confirmed);
        assert_eq!(status(0x12, 0x11), MintLockStatus::Expired);
        // with no lock recorded as of block 0x20, one may only be included from block 0x21
        assert_eq!(status(0, 0x22), MintLockStatus::Pending);
        assert_eq!(status(0, 0x21), MintLockStatus::Expired);
        assert_eq!(status(0, 0x10), MintLockStatus::Expired);
    }

    #[cfg(feature = "_online")]
    #[test]
    fn mint_lock_recorded() {
        let contract = EvmAddress::default();
        let lock_id = LockId::from([1u8; 32]);
        // whether the lock is recorded, with the given record as of the latest block, read with
        // the `latest` tag, without asking for the block number first
        let recorded = |record: u64, before_block: u64| {
            let mut server = mockito::Server::new();
            let block_number = mock_block_number(&mut server).expect(0);
            let call = server
                .mock("POST", "/")
                .match_body(mockito::Matcher::PartialJsonString(format!(
                    r#"{{"method":"eth_call","params":[{{"data":"{}"}},"latest"]}}"#,
                    funds_in_call(&lock_id, 7)
                )))
                .with_body(format!(
                    r#"{{"jsonrpc":"2.0","id":1,"result":"0x{record:064x}"}}"#
                ))
                .create();
            let recorded = EvmClient::new(&server.url())
                .unwrap()
                .mint_lock_recorded(&contract, &lock_id, 7, before_block)
                .unwrap();
            call.assert();
            block_number.assert();
            recorded
        };

        // a lock included in block 0x11 is recorded for the mint only if that's before the block
        assert!(recorded(0x12, 0x12));
        assert!(!recorded(0x12, 0x11));
        assert!(!recorded(0, 0x12));
    }

    #[test]
    fn is_confirmed_reads_finalized_record() {
        let location = BridgeLocation::Evm(EvmContract {
            chain_id: 31337,
            address: EvmAddress::default(),
        });
        let opid = OpId::from([0u8; 32]);
        // from hex, as `from_byte_array` needs the bitcoin `Hash` trait, which the crate only
        // imports in a wallet build
        let witness_id = RgbTxid::from_str(&"01".repeat(32)).unwrap();
        let claim_in = |witness_id: RgbTxid, amount: u64, before_block: u64| Claim::Lock {
            opid,
            witness_id,
            amount,
            before_block,
        };
        let claim = |amount: u64, before_block: u64| claim_in(witness_id, amount, before_block);
        let lock_id = LockId::with(opid, witness_id);
        // the lock of 7 recorded in block 0x11, read as of the finalized block 0x20
        let mut server = mockito::Server::new();
        // read once, however many claims are resolved
        let finalized = mock_finalized_block(&mut server, 0x20).expect(1);
        let record = mock_eth_call_hits(
            &mut server,
            0x20,
            &funds_in_call(&lock_id, 7),
            &format!("0x{:064x}", 0x12),
            2,
        );
        let no_record = mock_eth_call_at(
            &mut server,
            0x20,
            &funds_in_call(&lock_id, 8),
            &format!("0x{:064x}", 0),
        );
        let other_witness_id = RgbTxid::from_str(&"02".repeat(32)).unwrap();
        let other_tx_no_record = mock_eth_call_at(
            &mut server,
            0x20,
            &funds_in_call(&LockId::with(opid, other_witness_id), 7),
            &format!("0x{:064x}", 0),
        );
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = MintClaimResolver::new(&client);
        assert!(resolver.is_confirmed(&location, &claim(7, 0x30)).unwrap());
        // the record is keyed by the amount too: another amount isn't confirmed
        assert!(!resolver.is_confirmed(&location, &claim(8, 0x30)).unwrap());
        // and by the witness TX: the same mint in another TX isn't confirmed by the lock
        assert!(
            !resolver
                .is_confirmed(&location, &claim_in(other_witness_id, 7, 0x30))
                .unwrap()
        );
        // the lock has to be included before the block the claim commits to
        assert!(!resolver.is_confirmed(&location, &claim(7, 0x11)).unwrap());
        assert!(resolver.is_expired(&OpId::from([0u8; 32])));
        record.assert();
        no_record.assert();
        other_tx_no_record.assert();
        finalized.assert();
    }
}
