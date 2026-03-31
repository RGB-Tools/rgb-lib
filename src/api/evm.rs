use super::*;

/// keccak256("FundsIn(address,uint256,uint64)")
const FUNDS_IN_TOPIC: &str = "0xf1a18caea297591892fc07ea412a5e617d8e51e1155912d8871793e1d4e70f87";

/// Selector of `length()` on a reject list contract (`IRgbRejectList`)
const REJECT_LIST_LENGTH_SELECTOR: &str = "0x1f7b6d32";
/// Selector of `entries(uint256,uint256)` on a reject list contract (`IRgbRejectList`)
const REJECT_LIST_ENTRIES_SELECTOR: &str = "0x17b937e6";
/// Maximum number of reject list entries requested with a single `entries` call
///
/// The contract doesn't bound pages, the `eth_call` gas cap and timeout of the node do.
const REJECT_LIST_PAGE_SIZE: u64 = 1000;
/// Maximum number of blocks whose logs are requested with a single `eth_getLogs` call
///
/// Providers bound the block range (or the number of results) of a single call, each with its
/// own limit: a range the node refuses is halved and requested again.
const LOGS_BLOCK_RANGE: u64 = 1000;
/// Number of times a JSON-RPC request is sent again when it fails for a temporary reason (see
/// [`EvmClient::post`])
const RPC_RETRIES: u32 = 2;
/// Pause before sending a JSON-RPC request again, multiplied by the number of the retry
#[cfg(not(test))]
const RPC_RETRY_PAUSE: Duration = Duration::from_secs(1);
#[cfg(test)]
const RPC_RETRY_PAUSE: Duration = Duration::from_millis(10);

/// The outcome of a scan for the `FundsIn` event confirming a mint (see
/// [`EvmClient::find_funds_in`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FundsInScan {
    /// A confirming event was found in a finalized block
    Confirmed,
    /// No confirming event was found up to the given finalized block
    NotFound { finalized_block: u64 },
}

/// JSON-RPC client for an EVM node.
pub struct EvmClient {
    client: RestClient,
    rpc_url: String,
}

/// A single log entry returned by `eth_getLogs`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvmLog {
    /// Indexed topic hashes (topic[0] = event signature hash).
    pub topics: Vec<String>,
    /// ABI-encoded non-indexed parameters.
    pub data: String,
    /// Block number (hex).
    pub block_number: Option<String>,
}

/// Decoded FundsIn event from the bridge contract.
#[derive(Debug)]
pub(crate) struct FundsInEvent {
    /// Locked amount.
    pub amount: u64,
    /// RGB operation ID (32 bytes).
    pub operation_id: [u8; 32],
    /// Block the event was emitted in, if reported by the node.
    pub block_number: Option<u64>,
}

impl FundsInEvent {
    /// Whether this event confirms a mint anchor: it commits to the given `opid` with the exact
    /// `amount` and follows `after_block` (the external chain block committed by the transition).
    ///
    /// Whether the event is final is not decided here: callers only look events up in finalized
    /// blocks (see [`EvmClient::finalized_block_number`]). An event with no block number (still
    /// pending) never confirms.
    pub(crate) fn confirms(&self, opid: &OpId, amount: u64, after_block: u64) -> bool {
        OpId::from(self.operation_id) == *opid
            && self.amount == amount
            && self.block_number.is_some_and(|block| block > after_block)
    }
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

#[derive(Debug, Serialize)]
struct NullRequest;

/// Decode a hex-encoded ABI word (32 bytes) at the given word index from `data`.
/// `data` must start with "0x".
fn abi_word(data: &str, index: usize) -> Result<[u8; 32], Error> {
    // work on bytes: slicing the str could split a (bogus) multi-byte char and panic
    let hex = strip_hex_prefix(data, "ABI data")?.as_bytes();
    let start = index * 64;
    let end = start + 64;
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
    let offset = abi_word_u64(data, 0)?
        .filter(|o| o % 32 == 0)
        .ok_or_else(|| abi_error("bad array offset"))?;
    let len_index = (offset / 32) as usize;
    let len = abi_word_u64(data, len_index)?.ok_or_else(|| abi_error("bad array length"))?;
    let mut entries = vec![];
    for i in 0..len as usize {
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

impl EvmLog {
    /// Try to parse this log as a FundsIn event.
    ///
    /// Returns `None` if the log topic doesn't match, or if the locked amount doesn't fit in a
    /// `u64`: RGB amounts are `u64`, so such an event can never match a mint and truncating it
    /// could make it wrongly validate a mint of the truncated amount.
    pub fn as_funds_in(&self) -> Result<Option<FundsInEvent>, Error> {
        let Some(topic0) = self.topics.first() else {
            return Ok(None);
        };

        if !topic0.eq_ignore_ascii_case(FUNDS_IN_TOPIC) {
            return Ok(None);
        }

        let block_number = self
            .block_number
            .as_deref()
            .map(parse_block_number)
            .transpose()?;

        // event FundsIn(address indexed sender, uint256 indexed operationId, uint64 amount)
        // topics = [signature, sender, operationId]
        // data = abi.encode(amount), the uint64 amount still padded to a 32-byte word
        let [_, _, operation_id_topic] = self.topics.as_slice() else {
            return Err(Error::Network {
                details: format!(
                    "unexpected FundsIn topic count: {} (expected 3)",
                    self.topics.len()
                ),
            });
        };
        // compare the hex length exactly: a length rounded to whole words would let trailing
        // data through
        let data_hex = strip_hex_prefix(&self.data, "ABI data")?;
        if data_hex.len() != 64 {
            return Err(Error::Network {
                details: format!(
                    "unexpected FundsIn ABI payload size: {} hex chars (expected 64)",
                    data_hex.len()
                ),
            });
        }
        let operation_id = abi_word(operation_id_topic, 0)?;
        let Some(amount) = abi_word_u64(&self.data, 0)? else {
            return Ok(None);
        };
        Ok(Some(FundsInEvent {
            amount,
            operation_id,
            block_number,
        }))
    }
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
    /// Block timestamp (hex, seconds since the Unix epoch).
    #[serde(default)]
    timestamp: Option<String>,
}

/// Where a FundsIn event matching a mint was included on the external chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FundsInInclusion {
    /// Timestamp of the block the event was included in.
    pub timestamp: u64,
    /// Whether that block is finalized.
    pub finalized: bool,
}

/// Filter object for `eth_getLogs`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogFilter {
    /// Contract address to filter on.
    address: String,
    /// Start block (hex or tag).
    from_block: String,
    /// End block (hex or tag).
    to_block: String,
    /// Optional topic filters.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    topics: Vec<Option<String>>,
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

    fn req_err(e: impl std::fmt::Display) -> Error {
        Error::Network {
            details: format!("EVM RPC: {e}"),
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

    /// Fetch the `FundsIn` logs for `opid` emitted by `contract` from `from_block` to `to_block`
    /// (both included).
    ///
    /// The operation ID is an indexed parameter of the event, so the node filters the logs by it.
    ///
    /// An error reported by the node (e.g. for a range exceeding the provider's limits) is
    /// returned apart from the other failures, so that the caller can retry with a smaller range.
    /// Not when the HTTP status tells that the request is refused whatever its range (rate limited,
    /// or not authorized, e.g. for blocks the provider plan doesn't cover): a smaller range would
    /// only make more requests, so that's reported as a failure instead. An HTTP 413 (Payload Too
    /// Large) is instead always returned as a refused range, whatever its body, which some
    /// providers don't send as a JSON-RPC response.
    fn get_logs(
        &self,
        contract: &EvmAddress,
        opid: &OpId,
        from_block: u64,
        to_block: u64,
    ) -> Result<Result<Vec<EvmLog>, RpcError>, Error> {
        let body = RpcRequest {
            jsonrpc: "2.0",
            method: "eth_getLogs",
            params: [LogFilter {
                address: contract.to_string(),
                from_block: format!("0x{from_block:x}"),
                to_block: format!("0x{to_block:x}"),
                topics: vec![
                    Some(FUNDS_IN_TOPIC.to_string()),
                    None,
                    Some(format!("0x{}", hex::encode(opid.to_byte_array()))),
                ],
            }],
            id: 1,
        };

        let resp = self.post(&body)?;
        let status = resp.status();
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            let text = resp.text().unwrap_or_default();
            let details: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            return Ok(Err(RpcError {
                code: status.as_u16().into(),
                message: format!(
                    "(HTTP {status}) {}",
                    details.chars().take(200).collect::<String>()
                ),
            }));
        }
        let resp: RpcResponse<Vec<EvmLog>> = resp.json().map_err(Self::req_err)?;

        if let Some(err) = resp.error {
            if matches!(
                status,
                StatusCode::TOO_MANY_REQUESTS | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) {
                return Err(Error::Network {
                    details: format!(
                        "eth_getLogs error {} (HTTP {status}): {}",
                        err.code, err.message
                    ),
                });
            }
            return Ok(Err(err));
        }

        resp.result.map(Ok).ok_or_else(|| Error::Network {
            details: s!("eth_getLogs returned null result"),
        })
    }

    /// Whether `contract` emitted a `FundsIn` event committing to `opid` with the exact `amount`,
    /// past `after_block` and in a block the chain reports as finalized.
    ///
    /// The event can only follow the block the anchor commits to and is only accepted once
    /// finalized, so logs are scanned from the block after `scanned_block` (`after_block`, or a
    /// later finalized block a previous scan already covered without finding the event) up to the
    /// latest finalized one (read once, so that every range is checked against the same finality),
    /// in ranges of at most `block_range` blocks, stopping at the first range holding a confirming
    /// event. A range the node refuses is halved and requested again, down to a single block,
    /// adapting the scan to whatever limit the provider sets.
    fn find_funds_in(
        &self,
        contract: &EvmAddress,
        opid: &OpId,
        amount: u64,
        after_block: u64,
        scanned_block: u64,
        block_range: u64,
    ) -> Result<FundsInScan, Error> {
        let finalized_block = self.finalized_block_number()?;
        let confirming_block = self.find_funds_in_block(
            contract,
            opid,
            amount,
            after_block,
            max(scanned_block, after_block),
            finalized_block,
            block_range,
        )?;
        Ok(match confirming_block {
            Some(_) => FundsInScan::Confirmed,
            None => FundsInScan::NotFound { finalized_block },
        })
    }

    /// The number of the first block after `scanned_block` and up to `last_block` including a
    /// FundsIn event confirming the mint of `amount` with operation `opid` (so past
    /// `after_block`), if any. Logs are requested in ranges of at most `block_range` blocks,
    /// halved when the node refuses one.
    #[allow(clippy::too_many_arguments)]
    fn find_funds_in_block(
        &self,
        contract: &EvmAddress,
        opid: &OpId,
        amount: u64,
        after_block: u64,
        scanned_block: u64,
        last_block: u64,
        mut block_range: u64,
    ) -> Result<Option<u64>, Error> {
        let mut from_block = scanned_block.saturating_add(1);
        while from_block <= last_block {
            let to_block = std::cmp::min(from_block.saturating_add(block_range - 1), last_block);
            let logs = match self.get_logs(contract, opid, from_block, to_block)? {
                Ok(logs) => logs,
                Err(_) if to_block > from_block => {
                    let requested_range = to_block - from_block + 1;
                    block_range = requested_range / 2;
                    continue;
                }
                Err(err) => {
                    return Err(Error::Network {
                        details: format!("eth_getLogs error {}: {}", err.code, err.message),
                    });
                }
            };
            let confirming_block = logs
                .iter()
                .filter_map(|log| log.as_funds_in().transpose())
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .find(|event| event.confirms(opid, amount, after_block))
                .and_then(|event| event.block_number);
            if confirming_block.is_some() {
                return Ok(confirming_block);
            }
            let Some(next_block) = to_block.checked_add(1) else {
                break;
            };
            from_block = next_block;
        }
        Ok(None)
    }

    /// Where a FundsIn event confirming the mint of `amount` with operation `opid`, committing
    /// to `after_block`, was included, finalized or not, if any.
    pub(crate) fn funds_in_inclusion(
        &self,
        contract: &EvmAddress,
        opid: &OpId,
        amount: u64,
        after_block: u64,
    ) -> Result<Option<FundsInInclusion>, Error> {
        // the finalized block is read first, so it cannot be past the latest one read after it
        let finalized_block = self.finalized_block_number()?;
        let latest_block = self.block_number()?;
        let Some(block) = self.find_funds_in_block(
            contract,
            opid,
            amount,
            after_block,
            after_block,
            latest_block,
            LOGS_BLOCK_RANGE,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(FundsInInclusion {
            timestamp: self.block_timestamp(block)?,
            finalized: block <= finalized_block,
        }))
    }

    /// Timestamp of the block with the given number (`eth_getBlockByNumber`).
    pub(crate) fn block_timestamp(&self, block: u64) -> Result<u64, Error> {
        let header: BlockHeader =
            self.rpc("eth_getBlockByNumber", (format!("0x{block:x}"), false))?;
        let timestamp = header.timestamp.ok_or_else(|| Error::Network {
            details: format!("the node reports no timestamp for block {block}"),
        })?;
        parse_quantity(&timestamp, "block timestamp")
    }

    /// Call a JSON-RPC method, failing if it reports an error or no result.
    fn rpc<P: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<R, Error> {
        let body = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id: 1,
        };
        let resp: RpcResponse<R> = self.post(&body)?.json().map_err(Self::req_err)?;

        if let Some(err) = resp.error {
            return Err(Error::Network {
                details: format!("{method} error {}: {}", err.code, err.message),
            });
        }

        resp.result.ok_or_else(|| Error::Network {
            details: format!("{method} returned null result"),
        })
    }

    /// Number of the latest block (`eth_blockNumber`).
    fn block_number(&self) -> Result<u64, Error> {
        parse_block_number(&self.rpc::<_, String>("eth_blockNumber", NullRequest)?)
    }

    /// Call `contract` with `data` (`eth_call`) as of `block`, returning the raw result.
    fn eth_call(&self, contract: &EvmAddress, data: String, block: u64) -> Result<String, Error> {
        let call = CallObject {
            to: contract.to_string(),
            data,
        };
        self.rpc("eth_call", (call, format!("0x{block:x}")))
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

    /// Call a JSON-RPC method taking no parameters and returning a string.
    fn call_no_params(&self, method: &'static str) -> Result<String, Error> {
        let body: RpcRequest<NullRequest> = RpcRequest {
            jsonrpc: "2.0",
            method,
            params: NullRequest,
            id: 1,
        };
        let resp: RpcResponse<String> = self.post(&body)?.json().map_err(Self::req_err)?;

        if let Some(err) = resp.error {
            return Err(Error::Network {
                details: format!("{method} error {}: {}", err.code, err.message),
            });
        }

        resp.result.ok_or_else(|| Error::Network {
            details: format!("{method} returned null result"),
        })
    }

    /// Number of the latest finalized block (`eth_getBlockByNumber` with the `finalized` tag).
    ///
    /// Finality is the chain's own notion of irreversibility, whatever its consensus: two
    /// justified epochs on Ethereum, a milestone on Polygon, a batch finalized on L1 for a
    /// rollup, and so on. A reorged-out mint event would permanently invalidate the RGB-side
    /// history for every future receiver, so events are only accepted once finalized.
    pub(crate) fn finalized_block_number(&self) -> Result<u64, Error> {
        let method = "eth_getBlockByNumber";
        let body = RpcRequest {
            jsonrpc: "2.0",
            method,
            params: ("finalized", false),
            id: 1,
        };
        let resp: RpcResponse<Option<BlockHeader>> =
            self.post(&body)?.json().map_err(Self::req_err)?;

        if let Some(err) = resp.error {
            return Err(Error::Network {
                details: format!("{method} error {}: {}", err.code, err.message),
            });
        }

        let header = resp.result.flatten().ok_or_else(|| Error::Network {
            details: s!("the node reports no finalized block"),
        })?;
        parse_block_number(&header.number)
    }

    /// Check that the node serves what confirming mint events needs, failing with an
    /// [`Error::InvalidEvmRpcUrl`] if it doesn't: the latest finalized block, since events are
    /// only accepted once finalized, and the logs of any block, since a mint in the history of a
    /// received asset can have its event in any block after its bridge was deployed.
    ///
    /// The logs of the genesis block are requested to check the latter: some nodes (e.g. on the
    /// free plans of some providers) only serve the logs of recent blocks, which would make the
    /// confirmation of an older mint fail later, when receiving an asset.
    pub(crate) fn check_mint_support(&self) -> Result<(), Error> {
        self.finalized_block_number()
            .map_err(|e| Error::InvalidEvmRpcUrl {
                details: format!("cannot get the latest finalized block: {e}"),
            })?;
        let logs_error = |details: String| Error::InvalidEvmRpcUrl {
            details: format!(
                "cannot get the logs of the oldest blocks, an endpoint serving the whole log \
                 history is needed: {details}"
            ),
        };
        match self.get_logs(&EvmAddress::default(), &OpId::from([0u8; 32]), 0, 0) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(logs_error(format!(
                "eth_getLogs error {}: {}",
                err.code, err.message
            ))),
            Err(e) => Err(logs_error(e.to_string())),
        }
    }

    /// EIP-155 chain ID of the chain the node is connected to (`eth_chainId`).
    pub(crate) fn chain_id(&self) -> Result<u64, Error> {
        parse_quantity(&self.call_no_params("eth_chainId")?, "chain ID")
    }
}

/// Report a JSON-RPC client failure as the anchor resolver's issue.
///
/// The client only fails with [`Error::Network`]s: their details are reported as they are, to
/// avoid wrapping them in a second network error once mapped back to an [`Error`].
fn resolver_issue(e: Error) -> AnchorResolverError {
    match e {
        Error::Network { details } => AnchorResolverError::ResolverIssue(details),
        e => AnchorResolverError::ResolverIssue(e.to_string()),
    }
}

impl ResolveAnchor for EvmClient {
    fn evm_chain_id(&self) -> Result<u64, AnchorResolverError> {
        self.chain_id().map_err(resolver_issue)
    }

    /// Whether the bridge contract at `location` emitted a `FundsIn` event confirming `anchor`:
    /// one committing to the anchor's operation ID and amount, past the block the anchor commits
    /// to and in a block the chain reports as finalized.
    fn is_confirmed(
        &self,
        location: &BridgeLocation,
        anchor: &ExternalAnchor,
    ) -> Result<bool, AnchorResolverError> {
        let BridgeLocation::Evm(EvmContract { address, .. }) = location;
        let ExternalAnchor::MintEvent {
            opid,
            amount,
            after_block,
        } = anchor;

        let scan = self
            .find_funds_in(
                address,
                opid,
                *amount,
                *after_block,
                *after_block,
                LOGS_BLOCK_RANGE,
            )
            .map_err(resolver_issue)?;
        Ok(scan == FundsInScan::Confirmed)
    }
}

/// An anchor resolver over an [`EvmClient`] scanning for each mint event only the given number of
/// blocks after the one the anchor commits to (`after_block`).
///
/// The `after_block` of an anchor is chosen by whoever created the mint, so without a limit a
/// consignment can make the scan go through the whole chain (e.g. with `after_block` set to 0).
pub(crate) struct BoundedAnchorResolver<'a> {
    client: &'a EvmClient,
    /// Maximum number of blocks to scan after `after_block`
    max_scan_blocks: u64,
    /// Operation IDs of the mints whose event was not found in any of the blocks within the
    /// limit, all of them finalized
    exhausted: RefCell<HashSet<OpId>>,
}

impl<'a> BoundedAnchorResolver<'a> {
    pub(crate) fn new(client: &'a EvmClient, max_scan_blocks: u64) -> Self {
        Self {
            client,
            max_scan_blocks,
            exhausted: RefCell::new(HashSet::new()),
        }
    }

    /// Whether the event confirming the mint with the given operation ID was not found in any of
    /// the blocks within the limit, all of them finalized, so it will never be found within it.
    ///
    /// A mint that is not confirmed but not exhausted may still be, once more blocks within the
    /// limit get finalized.
    pub(crate) fn is_exhausted(&self, opid: &OpId) -> bool {
        self.exhausted.borrow().contains(opid)
    }
}

impl ResolveAnchor for BoundedAnchorResolver<'_> {
    fn evm_chain_id(&self) -> Result<u64, AnchorResolverError> {
        self.client.evm_chain_id()
    }

    /// Whether `anchor` is confirmed, as [`EvmClient`] resolves it, within the scan limit.
    fn is_confirmed(
        &self,
        location: &BridgeLocation,
        anchor: &ExternalAnchor,
    ) -> Result<bool, AnchorResolverError> {
        let BridgeLocation::Evm(EvmContract { address, .. }) = location;
        let ExternalAnchor::MintEvent {
            opid,
            amount,
            after_block,
        } = anchor;
        let finalized_block = self
            .client
            .finalized_block_number()
            .map_err(resolver_issue)?;
        let limit_block = after_block.saturating_add(self.max_scan_blocks);
        let last_block = std::cmp::min(finalized_block, limit_block);
        let confirming_block = self
            .client
            .find_funds_in_block(
                address,
                opid,
                *amount,
                *after_block,
                *after_block,
                last_block,
                LOGS_BLOCK_RANGE,
            )
            .map_err(resolver_issue)?;
        if confirming_block.is_none() && last_block == limit_block {
            self.exhausted.borrow_mut().insert(*opid);
        }
        Ok(confirming_block.is_some())
    }
}

/// An anchor resolver over an [`EvmClient`] resuming the scan for each mint event after the last
/// finalized block a previous scan covered without finding it, and recording how far each scan
/// got, so that a mint waited for across refreshes doesn't have its blocks scanned again.
///
/// As with a [`BoundedAnchorResolver`], each mint event is only scanned for the given number of
/// blocks after the one the anchor commits to (`after_block`).
pub(crate) struct ResumingAnchorResolver<'a> {
    client: &'a EvmClient,
    /// Last finalized block scanned without finding the event, by mint operation ID
    scanned_blocks: RefCell<HashMap<OpId, u64>>,
    /// Maximum number of blocks to scan after `after_block`
    max_scan_blocks: u64,
    /// Operation IDs of the mints whose event was not found in any of the blocks within the
    /// limit, all of them finalized
    exhausted: RefCell<HashSet<OpId>>,
}

impl<'a> ResumingAnchorResolver<'a> {
    pub(crate) fn new(
        client: &'a EvmClient,
        scanned_blocks: HashMap<OpId, u64>,
        max_scan_blocks: u64,
    ) -> Self {
        Self {
            client,
            scanned_blocks: RefCell::new(scanned_blocks),
            max_scan_blocks,
            exhausted: RefCell::new(HashSet::new()),
        }
    }

    /// The last finalized block scanned without finding the event, by mint operation ID.
    pub(crate) fn scanned_blocks(&self) -> HashMap<OpId, u64> {
        self.scanned_blocks.borrow().clone()
    }

    /// Whether the event confirming the mint with the given operation ID was not found in any of
    /// the blocks within the limit, all of them finalized, so it will never be found within it.
    pub(crate) fn is_exhausted(&self, opid: &OpId) -> bool {
        self.exhausted.borrow().contains(opid)
    }
}

impl ResolveAnchor for ResumingAnchorResolver<'_> {
    fn evm_chain_id(&self) -> Result<u64, AnchorResolverError> {
        self.client.evm_chain_id()
    }

    /// Whether `anchor` is confirmed, as [`EvmClient`] resolves it, resuming its scan, within the
    /// scan limit.
    fn is_confirmed(
        &self,
        location: &BridgeLocation,
        anchor: &ExternalAnchor,
    ) -> Result<bool, AnchorResolverError> {
        let BridgeLocation::Evm(EvmContract { address, .. }) = location;
        let ExternalAnchor::MintEvent {
            opid,
            amount,
            after_block,
        } = anchor;

        // finalized blocks cannot be reorged, so the ones already scanned needn't be again
        let scanned_block = self
            .scanned_blocks
            .borrow()
            .get(opid)
            .copied()
            .unwrap_or(*after_block);
        let finalized_block = self
            .client
            .finalized_block_number()
            .map_err(resolver_issue)?;
        let limit_block = after_block.saturating_add(self.max_scan_blocks);
        let last_block = std::cmp::min(finalized_block, limit_block);
        let confirming_block = self
            .client
            .find_funds_in_block(
                address,
                opid,
                *amount,
                *after_block,
                max(scanned_block, *after_block),
                last_block,
                LOGS_BLOCK_RANGE,
            )
            .map_err(resolver_issue)?;
        if confirming_block.is_some() {
            return Ok(true);
        }
        self.scanned_blocks
            .borrow_mut()
            .insert(*opid, max(scanned_block, last_block));
        if last_block == limit_block {
            self.exhausted.borrow_mut().insert(*opid);
        }
        Ok(false)
    }
}

/// An anchor resolver over an [`EvmClient`] for the mints of a received consignment: terminal
/// mints resumed, history mints not, all of them within the same scan limit.
///
/// The ones paying the recipient (the terminal ones) may still be waited for, so their scan is
/// resumed across refreshes by a [`ResumingAnchorResolver`]. The ones in their history must
/// already be confirmed, so they're resolved by a [`BoundedAnchorResolver`] instead. Both only
/// scan the same number of blocks after the one the anchor commits to (`after_block`), which is
/// chosen by whoever created the mint.
pub(crate) struct MintAnchorResolver<'a> {
    terminal: ResumingAnchorResolver<'a>,
    history: BoundedAnchorResolver<'a>,
    /// Operation IDs of the terminal mints
    terminal_opids: &'a HashSet<OpId>,
}

impl<'a> MintAnchorResolver<'a> {
    pub(crate) fn new(
        client: &'a EvmClient,
        terminal_opids: &'a HashSet<OpId>,
        scanned_blocks: HashMap<OpId, u64>,
        max_scan_blocks: u64,
    ) -> Self {
        Self {
            terminal: ResumingAnchorResolver::new(client, scanned_blocks, max_scan_blocks),
            history: BoundedAnchorResolver::new(client, max_scan_blocks),
            terminal_opids,
        }
    }

    /// The last finalized block scanned without finding the event, by terminal mint operation ID.
    pub(crate) fn scanned_blocks(&self) -> HashMap<OpId, u64> {
        self.terminal.scanned_blocks()
    }

    /// Whether the event confirming the mint with the given operation ID will never be found
    /// within the scan limit (see [`BoundedAnchorResolver::is_exhausted`]).
    pub(crate) fn is_exhausted(&self, opid: &OpId) -> bool {
        if self.terminal_opids.contains(opid) {
            self.terminal.is_exhausted(opid)
        } else {
            self.history.is_exhausted(opid)
        }
    }
}

impl ResolveAnchor for MintAnchorResolver<'_> {
    fn evm_chain_id(&self) -> Result<u64, AnchorResolverError> {
        self.terminal.evm_chain_id()
    }

    /// Whether `anchor` is confirmed, resolved as a terminal or as a history mint.
    fn is_confirmed(
        &self,
        location: &BridgeLocation,
        anchor: &ExternalAnchor,
    ) -> Result<bool, AnchorResolverError> {
        let ExternalAnchor::MintEvent { opid, .. } = anchor;
        if self.terminal_opids.contains(opid) {
            self.terminal.is_confirmed(location, anchor)
        } else {
            self.history.is_confirmed(location, anchor)
        }
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

        let mut server = mockito::Server::new();
        let block_number = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_blockNumber"}"#.to_string(),
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
        block: u64,
        data: &str,
        result: &str,
    ) -> mockito::Mock {
        server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"method":"eth_call","params":[{{"data":"{data}"}},"0x{block:x}"]}}"#
            )))
            .with_body(format!(r#"{{"jsonrpc":"2.0","id":1,"result":"{result}"}}"#))
            .expect(1)
            .create()
    }

    fn mock_block_number(server: &mut mockito::Server) -> mockito::Mock {
        server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_blockNumber"}"#.to_string(),
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
                r#"{"method":"eth_chainId"}"#.to_string(),
            ))
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":"0x7a69"}"#)
            .expect(2)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(client.chain_id().unwrap(), 31337);
        assert_eq!(client.evm_chain_id().unwrap(), 31337);
        mock.assert();

        // an RPC error is reported as the resolver's issue, with its details
        let _mock = server
            .mock("POST", "/")
            .with_body(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"not found"}}"#)
            .create();
        let result = EvmClient::new(&server.url()).unwrap().evm_chain_id();
        assert_eq!(
            result,
            Err(AnchorResolverError::ResolverIssue(s!(
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
                    r#"{"method":"eth_chainId"}"#.to_string(),
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

    #[test]
    fn is_confirmed_before_after_block() {
        // finality hasn't advanced past the block the anchor commits to: no finalized event can
        // exist, so the logs are not even queried
        let mut server = mockito::Server::new();
        let block_number = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_getBlockByNumber"}"#.to_string(),
            ))
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"number":"0x10"}}"#)
            .create();
        let get_logs = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_getLogs"}"#.to_string(),
            ))
            .expect(0)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        let location = BridgeLocation::Evm(EvmContract {
            chain_id: 31337,
            address: EvmAddress::default(),
        });
        let anchor = ExternalAnchor::MintEvent {
            opid: OpId::from([0u8; 32]),
            amount: 7,
            after_block: 16,
        };
        assert!(!client.is_confirmed(&location, &anchor).unwrap());
        block_number.assert();
        get_logs.assert();
    }

    #[test]
    fn is_confirmed_skips_wrong_amount() {
        let location = BridgeLocation::Evm(EvmContract {
            chain_id: 31337,
            address: EvmAddress::default(),
        });
        let anchor = ExternalAnchor::MintEvent {
            opid: OpId::from([0u8; 32]),
            amount: 7,
            after_block: 16,
        };
        let log = |amount: u64| funds_in_log(amount, 0x11);
        let is_confirmed = |logs: &[String]| {
            let mut server = mockito::Server::new();
            let _block_number = server
                .mock("POST", "/")
                .match_body(mockito::Matcher::PartialJsonString(
                    r#"{"method":"eth_getBlockByNumber"}"#.to_string(),
                ))
                .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"number":"0x20"}}"#)
                .create();
            let get_logs = server
                .mock("POST", "/")
                .match_body(mockito::Matcher::PartialJsonString(
                    r#"{"method":"eth_getLogs"}"#.to_string(),
                ))
                .with_body(format!(
                    r#"{{"jsonrpc":"2.0","id":1,"result":[{}]}}"#,
                    logs.join(",")
                ))
                .create();
            let confirmed = EvmClient::new(&server.url())
                .unwrap()
                .is_confirmed(&location, &anchor)
                .unwrap();
            get_logs.assert();
            confirmed
        };

        // an event for the same operation with a wrong amount listed before the right one doesn't
        // hide it
        assert!(is_confirmed(&[log(8), log(7)]));
        // the wrong amount alone doesn't confirm
        assert!(!is_confirmed(&[log(8)]));
    }

    /// Mock an `eth_getBlockByNumber` reporting `finalized` as the finalized block.
    fn mock_finalized_block(server: &mut mockito::Server, finalized: u64) -> mockito::Mock {
        server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_getBlockByNumber"}"#.to_string(),
            ))
            .with_body(format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"number":"0x{finalized:x}"}}}}"#
            ))
            .expect(1)
            .create()
    }

    /// Mock an `eth_getLogs` from `from_block` to `to_block`, expected `hits` times, answering
    /// with `body` (the result or error member of the response).
    fn mock_get_logs(
        server: &mut mockito::Server,
        from_block: u64,
        to_block: u64,
        body: &str,
        hits: usize,
    ) -> mockito::Mock {
        server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"method":"eth_getLogs","params":[{{"fromBlock":"0x{from_block:x}","toBlock":"0x{to_block:x}","topics":["{FUNDS_IN_TOPIC}",null,"0x{}"]}}]}}"#,
                "00".repeat(32)
            )))
            .with_body(format!(r#"{{"jsonrpc":"2.0","id":1,{body}}}"#))
            .expect(hits)
            .create()
    }

    /// The topics of a `FundsIn` event for the operation whose ID is all `opid_byte`.
    fn funds_in_topics(opid_byte: u8) -> Vec<String> {
        vec![
            FUNDS_IN_TOPIC.to_string(),
            format!("0x{}{}", "00".repeat(12), "aa".repeat(20)),
            format!("0x{}", format!("{opid_byte:02x}").repeat(32)),
        ]
    }

    /// A `FundsIn` log (as JSON) for the all-zeros operation ID, emitted in `block`.
    fn funds_in_log(amount: u64, block: u64) -> String {
        format!(
            r#"{{"topics":{:?},"data":"0x{amount:064x}","blockNumber":"0x{block:x}"}}"#,
            funds_in_topics(0)
        )
    }

    const NO_LOGS: &str = r#""result":[]"#;
    const RANGE_TOO_LARGE: &str =
        r#""error":{"code":-32005,"message":"query exceeds max block range"}"#;

    #[test]
    fn find_funds_in_ranges() {
        let opid = OpId::from([0u8; 32]);

        // blocks 17 to 32 in ranges of 5 blocks, the last one capped at the finalized block:
        // the scan stops at the range holding the event, later ranges are not requested
        let mut server = mockito::Server::new();
        let finalized = mock_finalized_block(&mut server, 32);
        let ranges = [
            mock_get_logs(&mut server, 17, 21, NO_LOGS, 1),
            mock_get_logs(&mut server, 22, 26, NO_LOGS, 1),
            mock_get_logs(
                &mut server,
                27,
                31,
                &format!(r#""result":[{}]"#, funds_in_log(7, 28)),
                1,
            ),
            mock_get_logs(&mut server, 32, 32, NO_LOGS, 0),
        ];
        let client = EvmClient::new(&server.url()).unwrap();
        let address = EvmAddress::default();
        assert_eq!(
            client.find_funds_in(&address, &opid, 7, 16, 16, 5).unwrap(),
            FundsInScan::Confirmed
        );
        finalized.assert();
        ranges.iter().for_each(|r| r.assert());

        // with no confirming event every range up to the finalized block is requested
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 32);
        let ranges = [(17, 21), (22, 26), (27, 31), (32, 32)]
            .map(|(from, to)| mock_get_logs(&mut server, from, to, NO_LOGS, 1));
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(
            client.find_funds_in(&address, &opid, 7, 16, 16, 5).unwrap(),
            FundsInScan::NotFound {
                finalized_block: 32
            }
        );
        ranges.iter().for_each(|r| r.assert());

        // a scan resumed after a block already scanned starts from the following one
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 32);
        let ranges =
            [(27, 31), (32, 32)].map(|(from, to)| mock_get_logs(&mut server, from, to, NO_LOGS, 1));
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(
            client.find_funds_in(&address, &opid, 7, 16, 26, 5).unwrap(),
            FundsInScan::NotFound {
                finalized_block: 32
            }
        );
        ranges.iter().for_each(|r| r.assert());

        // a scanned block before after_block doesn't extend the scan before it
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 20);
        let ranges = [mock_get_logs(&mut server, 17, 20, NO_LOGS, 1)];
        let client = EvmClient::new(&server.url()).unwrap();
        assert_eq!(
            client.find_funds_in(&address, &opid, 7, 16, 3, 5).unwrap(),
            FundsInScan::NotFound {
                finalized_block: 20
            }
        );
        ranges.iter().for_each(|r| r.assert());
    }

    #[test]
    fn resuming_resolver() {
        let opid = OpId::from([0u8; 32]);
        let location = BridgeLocation::Evm(EvmContract {
            chain_id: 31337,
            address: EvmAddress::default(),
        });
        let anchor = ExternalAnchor::MintEvent {
            opid,
            amount: 7,
            after_block: 16,
        };

        // with nothing scanned yet, the scan starts after the block the anchor commits to and
        // the last finalized block scanned is recorded
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 1020);
        let ranges = [
            mock_get_logs(&mut server, 17, 1016, NO_LOGS, 1),
            mock_get_logs(&mut server, 1017, 1020, NO_LOGS, 1),
        ];
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = ResumingAnchorResolver::new(&client, HashMap::new(), 100_000);
        assert!(!resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());
        let scanned_blocks = resolver.scanned_blocks();
        assert_eq!(scanned_blocks, HashMap::from([(opid, 1020)]));
        assert!(!resolver.is_exhausted(&opid));

        // a later resolution resumes after it, until the event is found
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 1030);
        let ranges = [mock_get_logs(
            &mut server,
            1021,
            1030,
            &format!(r#""result":[{}]"#, funds_in_log(7, 1025)),
            1,
        )];
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = ResumingAnchorResolver::new(&client, scanned_blocks.clone(), 100_000);
        assert!(resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());

        // a scan resumed only covers the blocks within the limit: with no event in them, all
        // finalized, the mint is exhausted
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 2000);
        let ranges = [mock_get_logs(&mut server, 1021, 1026, NO_LOGS, 1)];
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = ResumingAnchorResolver::new(&client, scanned_blocks.clone(), 1010);
        assert!(!resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());
        assert_eq!(resolver.scanned_blocks(), HashMap::from([(opid, 1026)]));
        assert!(resolver.is_exhausted(&opid));

        // as it is when resuming after the limit, with no more blocks to scan
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 2000);
        let get_logs = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"method":"eth_getLogs"}"#.to_string(),
            ))
            .expect(0)
            .create();
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = ResumingAnchorResolver::new(&client, scanned_blocks, 10);
        assert!(!resolver.is_confirmed(&location, &anchor).unwrap());
        get_logs.assert();
        assert_eq!(resolver.scanned_blocks(), HashMap::from([(opid, 1020)]));
        assert!(resolver.is_exhausted(&opid));
    }

    #[test]
    fn bounded_resolver() {
        let opid = OpId::from([0u8; 32]);
        let location = BridgeLocation::Evm(EvmContract {
            chain_id: 31337,
            address: EvmAddress::default(),
        });
        let anchor = ExternalAnchor::MintEvent {
            opid,
            amount: 7,
            after_block: 16,
        };

        // the scan stops at the limit, even if more blocks are finalized: with no event within it,
        // the mint is exhausted
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 1020);
        let ranges = [mock_get_logs(&mut server, 17, 26, NO_LOGS, 1)];
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = BoundedAnchorResolver::new(&client, 10);
        assert!(!resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());
        assert!(resolver.is_exhausted(&opid));

        // with a limit past the finalized block, the scan stops at the finalized block: the event
        // may still be in a later block within the limit, so the mint is not exhausted
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 20);
        let ranges = [mock_get_logs(&mut server, 17, 20, NO_LOGS, 1)];
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = BoundedAnchorResolver::new(&client, 10);
        assert!(!resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());
        assert!(!resolver.is_exhausted(&opid));

        // an event within the limit confirms the mint
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 1020);
        let ranges = [mock_get_logs(
            &mut server,
            17,
            26,
            &format!(r#""result":[{}]"#, funds_in_log(7, 26)),
            1,
        )];
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = BoundedAnchorResolver::new(&client, 10);
        assert!(resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());
        assert!(!resolver.is_exhausted(&opid));
    }

    #[test]
    fn mint_resolver() {
        let opid = OpId::from([0u8; 32]);
        let location = BridgeLocation::Evm(EvmContract {
            chain_id: 31337,
            address: EvmAddress::default(),
        });
        let anchor = ExternalAnchor::MintEvent {
            opid,
            amount: 7,
            after_block: 16,
        };

        // a terminal mint is scanned resuming after the scanned block, up to the finalized one
        // while within the limit, then it's exhausted
        let terminal_opids = HashSet::from([opid]);
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 1020);
        let ranges = [mock_get_logs(&mut server, 1001, 1020, NO_LOGS, 1)];
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = MintAnchorResolver::new(
            &client,
            &terminal_opids,
            HashMap::from([(opid, 1000)]),
            2000,
        );
        assert!(!resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());
        assert_eq!(resolver.scanned_blocks(), HashMap::from([(opid, 1020)]));
        assert!(!resolver.is_exhausted(&opid));
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 1020);
        let ranges = [mock_get_logs(&mut server, 17, 26, NO_LOGS, 1)];
        let client = EvmClient::new(&server.url()).unwrap();
        let resolver = MintAnchorResolver::new(&client, &terminal_opids, HashMap::new(), 10);
        assert!(!resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());
        assert!(resolver.is_exhausted(&opid));

        // a history mint is scanned up to the limit only, and exhausted with no event within it
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 1020);
        let ranges = [mock_get_logs(&mut server, 17, 26, NO_LOGS, 1)];
        let client = EvmClient::new(&server.url()).unwrap();
        let terminal_opids = HashSet::from([OpId::from([1u8; 32])]);
        let resolver = MintAnchorResolver::new(&client, &terminal_opids, HashMap::new(), 10);
        assert!(!resolver.is_confirmed(&location, &anchor).unwrap());
        ranges.iter().for_each(|r| r.assert());
        assert!(resolver.scanned_blocks().is_empty());
        assert!(resolver.is_exhausted(&opid));
    }

    #[test]
    fn find_funds_in_shrinks_refused_range() {
        let opid = OpId::from([0u8; 32]);
        let address = EvmAddress::default();

        // the node refuses 8 blocks: the range is halved and the smaller one kept for the rest
        // of the scan
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 32);
        let refused = mock_get_logs(&mut server, 17, 24, RANGE_TOO_LARGE, 1);
        let ranges = [(17, 20), (21, 24), (25, 28), (29, 32)]
            .map(|(from, to)| mock_get_logs(&mut server, from, to, NO_LOGS, 1));
        let client = EvmClient::new(&server.url()).unwrap();
        assert!(matches!(
            client.find_funds_in(&address, &opid, 7, 16, 16, 8).unwrap(),
            FundsInScan::NotFound { .. }
        ));
        refused.assert();
        ranges.iter().for_each(|r| r.assert());

        // a range capped by the finalized block is halved from its actual size
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 20);
        let refused = mock_get_logs(&mut server, 17, 20, RANGE_TOO_LARGE, 1);
        let ranges =
            [(17, 18), (19, 20)].map(|(from, to)| mock_get_logs(&mut server, from, to, NO_LOGS, 1));
        let client = EvmClient::new(&server.url()).unwrap();
        assert!(matches!(
            client
                .find_funds_in(&address, &opid, 7, 16, 16, 1000)
                .unwrap(),
            FundsInScan::NotFound { .. }
        ));
        refused.assert();
        ranges.iter().for_each(|r| r.assert());

        // a range refused with an HTTP 413 is halved too, whatever the answer body
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 24);
        let refused = server
            .mock("POST", "/")
            .match_body(mockito::Matcher::PartialJsonString(format!(
                r#"{{"method":"eth_getLogs","params":[{{"fromBlock":"0x{:x}","toBlock":"0x{:x}"}}]}}"#,
                17, 24
            )))
            .with_status(413)
            .with_body("<html>413 Request Entity Too Large</html>")
            .expect(1)
            .create();
        let ranges =
            [(17, 20), (21, 24)].map(|(from, to)| mock_get_logs(&mut server, from, to, NO_LOGS, 1));
        let client = EvmClient::new(&server.url()).unwrap();
        assert!(matches!(
            client.find_funds_in(&address, &opid, 7, 16, 16, 8).unwrap(),
            FundsInScan::NotFound { .. }
        ));
        refused.assert();
        ranges.iter().for_each(|r| r.assert());

        // a request refused whatever its range (rate limited, or not authorized) is reported
        // right away, without halving the range
        for status in [429, 401, 403] {
            let mut server = mockito::Server::new();
            let _finalized = mock_finalized_block(&mut server, 32);
            let refused = server
                .mock("POST", "/")
                .match_body(mockito::Matcher::PartialJsonString(
                    r#"{"method":"eth_getLogs"}"#.to_string(),
                ))
                .with_status(status)
                .with_body(
                    r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32011,"message":"limited"}}"#,
                )
                .expect(1)
                .create();
            let client = EvmClient::new(&server.url()).unwrap();
            let result = client.find_funds_in(&address, &opid, 7, 16, 16, 8);
            assert!(
                matches!(result, Err(Error::Network { ref details }) if details.starts_with("eth_getLogs error -32011 (HTTP")),
                "{result:?}"
            );
            refused.assert();
        }

        // a single block refused too is reported, with the node's error
        let mut server = mockito::Server::new();
        let _finalized = mock_finalized_block(&mut server, 18);
        let _refused = [(17, 18), (17, 17)]
            .map(|(from, to)| mock_get_logs(&mut server, from, to, RANGE_TOO_LARGE, 1));
        let client = EvmClient::new(&server.url()).unwrap();
        let result = client.find_funds_in(&address, &opid, 7, 16, 16, 1000);
        assert!(
            matches!(result, Err(Error::Network { ref details }) if details == "eth_getLogs error -32005: query exceeds max block range"),
            "{result:?}"
        );
    }

    #[test]
    fn funds_in_layout() {
        // the operation ID is read from the third topic, in its byte order
        let opid_bytes: [u8; 32] = std::array::from_fn(|i| i as u8);
        let mut topics = funds_in_topics(0);
        topics[2] = format!("0x{}", hex::encode(opid_bytes));
        let log = EvmLog {
            topics,
            data: format!("0x{:064x}", 7u64),
            block_number: None,
        };
        let event = log.as_funds_in().unwrap().unwrap();
        assert_eq!(OpId::from(event.operation_id), OpId::from(opid_bytes));
        assert_eq!(event.amount, 7);

        // a log of another event is skipped
        let log = EvmLog {
            topics: vec![format!("0x{}", "11".repeat(32))],
            data: s!("0x"),
            block_number: None,
        };
        assert!(log.as_funds_in().unwrap().is_none());

        // a FundsIn event without the indexed operation ID (as emitted by older contracts, with
        // the operation ID in the data) is refused
        let log = EvmLog {
            topics: funds_in_topics(0)[..2].to_vec(),
            data: format!("0x{}{:064x}", "00".repeat(32), 7u64),
            block_number: None,
        };
        let result = log.as_funds_in();
        assert!(
            matches!(result, Err(Error::Network { ref details }) if details.contains("unexpected FundsIn topic count: 2")),
            "{result:?}"
        );

        // data other than the single amount word is refused
        let log = EvmLog {
            topics: funds_in_topics(0),
            data: format!("0x{}{:064x}", "00".repeat(32), 7u64),
            block_number: None,
        };
        let result = log.as_funds_in();
        assert!(
            matches!(result, Err(Error::Network { ref details }) if details.contains("unexpected FundsIn ABI payload size: 128 hex chars")),
            "{result:?}"
        );

        // hex values without the 0x prefix are refused
        let log = EvmLog {
            topics: funds_in_topics(0),
            data: format!("{:064x}", 7u64),
            block_number: None,
        };
        let result = log.as_funds_in();
        assert!(
            matches!(result, Err(Error::Network { ref details }) if details.contains("missing 0x prefix")),
            "{result:?}"
        );
        let mut topics = funds_in_topics(0);
        topics[2] = topics[2].trim_start_matches("0x").to_string();
        let log = EvmLog {
            topics,
            data: format!("0x{:064x}", 7u64),
            block_number: None,
        };
        assert!(log.as_funds_in().is_err());
        // as is data not made of whole words, even when its first word is valid
        for data in [format!("0x{:064x}0", 7u64), format!("0x{:063x}é", 7u64)] {
            let log = EvmLog {
                topics: funds_in_topics(0),
                data,
                block_number: None,
            };
            let result = log.as_funds_in();
            assert!(
                matches!(result, Err(Error::Network { ref details }) if details.contains("unexpected FundsIn ABI payload size: 65 hex chars")),
                "{result:?}"
            );
        }
        // a multi-byte char across a word boundary is a decode error, not a panic
        let page = format!("0x{:063x}é{:063x}", 32, 0);
        assert!(decode_reject_list_entries(&page).is_err());
        assert!(parse_block_number("10").is_err());
        assert!(parse_block_number("0x0x10").is_err());
    }

    #[test]
    fn funds_in_block_number() {
        let log = EvmLog {
            topics: funds_in_topics(0),
            data: format!("0x{:064x}", 7u64),
            block_number: Some(s!("0x10")),
        };
        let event = log.as_funds_in().unwrap().unwrap();
        assert_eq!(event.amount, 7);
        assert_eq!(event.block_number, Some(16));
        // the event has to follow the block the anchor commits to
        let opid = OpId::from([0u8; 32]);
        assert!(event.confirms(&opid, 7, 15));
        assert!(!event.confirms(&opid, 7, 16));
        assert!(!event.confirms(&opid, 8, 15));
    }

    #[test]
    fn funds_in_amount_range() {
        let amount_max = format!("{:064x}", u64::MAX);
        // one above u64::MAX (2^64)
        let amount_over = format!("{}1{}", "0".repeat(47), "0".repeat(16));

        // an amount of exactly u64::MAX is accepted
        let log = EvmLog {
            topics: funds_in_topics(0),
            data: format!("0x{amount_max}"),
            block_number: None,
        };
        let event = log.as_funds_in().unwrap().unwrap();
        assert_eq!(event.amount, u64::MAX);

        // an amount exceeding u64::MAX makes the event unmatchable, not truncated: the contract
        // guards against it but the event data of an arbitrary contract cannot be trusted
        let log = EvmLog {
            topics: funds_in_topics(0),
            data: format!("0x{amount_over}"),
            block_number: None,
        };
        assert!(log.as_funds_in().unwrap().is_none());
    }
}
