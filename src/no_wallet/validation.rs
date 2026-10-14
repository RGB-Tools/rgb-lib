//! Consignment validation.
//!
//! This module defines the consignment validation that doesn't require a wallet (available with an
//! indexer validation feature, without the `wallet` one), the objects it returns and the indexer
//! support it needs, which is shared with online wallets.

use super::*;

#[cfg(any(feature = "electrum_validation", feature = "esplora_validation"))]
pub(crate) const INDEXER_TIMEOUT: u64 = 10;
#[cfg(any(feature = "electrum_validation", feature = "esplora_validation"))]
pub(crate) const INDEXER_RETRIES: u8 = 3;

/// Maximum number of blocks, after the one a mint event commits to (its `after_block`), in which
/// the event confirming it is searched for, when validating a consignment and for the mints in the
/// history of a consignment received by a wallet. It is about 70 days on Ethereum, 11 on Base or
/// Optimism and 37 hours on Arbitrum, at most 500 `eth_getLogs` calls per mint.
///
/// It is the same for everyone, and cannot be configured: a consignment accepted with a higher
/// limit could carry mints that others refuse, so assets received with it couldn't be spent to
/// them.
///
/// The `after_block` is chosen by whoever created the mint, so without a limit an untrusted
/// consignment could make the scan go through the whole EVM chain (e.g. with `after_block` set to
/// 0). A mint whose event is not found within the limit makes the consignment invalid, so the
/// limit needs to cover the time a minter may take to lock the funds after beginning the mint.
///
/// TODO: the limit will be set by each asset, with a global of the BFA schema (to be added in
/// rgb-schemas): remove this hardcoded value then.
pub(crate) const MAX_MINT_SCAN_BLOCKS: u64 = 500_000;

/// Maximum number of blocks scanned for a mint event (see [`MAX_MINT_SCAN_BLOCKS`]).
pub(crate) fn max_mint_scan_blocks() -> u64 {
    #[cfg(all(test, feature = "_online"))]
    if let Some(max_scan_blocks) = mock_max_mint_scan_blocks() {
        return max_scan_blocks;
    }
    MAX_MINT_SCAN_BLOCKS
}

/// Indexer to connect to, see `OnlineOptions::indexer_config` (with the `wallet` feature) and
/// [`validate_consignment`].
///
/// The variant selects the protocol to speak, so support for it must be compiled in: using an
/// indexer whose feature is disabled fails with [`Error::InvalidIndexer`].
#[cfg(feature = "_validation")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum IndexerConfig {
    /// A Bitcoin Core node. Requires the `bitcoind` feature (or `bitcoind_validation` to only
    /// validate consignments).
    ///
    /// The node must maintain a synced transaction index (`-txindex`), as RGB witnesses are
    /// looked up by TXID alone. The wallet is synced by scanning blocks from its last known
    /// checkpoint, so its first sync goes through the whole chain: with a block filter index on
    /// the node (`-blockfilterindex`) only the blocks matching the wallet scripts are fetched,
    /// which makes this considerably faster.
    Bitcoind {
        /// URL of the Bitcoin Core node RPC endpoint
        url: String,
        /// Credentials for the RPC connection
        auth: BitcoindAuth,
    },
    /// An indexer implementing the electrum protocol. Requires the `electrum` feature (or
    /// `electrum_validation` to only validate consignments).
    Electrum {
        /// URL of the electrum server
        url: String,
    },
    /// An indexer implementing the esplora protocol. Requires the `esplora` feature (or
    /// `esplora_validation` to only validate consignments).
    Esplora {
        /// URL of the esplora server
        url: String,
    },
}

#[cfg(feature = "_validation")]
impl IndexerConfig {
    /// URL of the indexer.
    pub fn url(&self) -> &str {
        match self {
            Self::Bitcoind { url, .. } | Self::Electrum { url } | Self::Esplora { url } => url,
        }
    }

    /// Protocol the indexer speaks.
    pub fn protocol(&self) -> IndexerProtocol {
        match self {
            Self::Bitcoind { .. } => IndexerProtocol::Bitcoind,
            Self::Electrum { .. } => IndexerProtocol::Electrum,
            Self::Esplora { .. } => IndexerProtocol::Esplora,
        }
    }
}

/// Authentication for a Bitcoin Core node RPC connection, see [`IndexerConfig::Bitcoind`].
#[cfg(feature = "_validation")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum BitcoindAuth {
    /// RPC username and password (`rpcuser` and `rpcpassword` or `rpcauth`)
    UserPass {
        /// RPC username
        username: String,
        /// RPC password
        password: String,
    },
    /// Path to the cookie file written by the node (`.cookie` in its data directory)
    CookieFile {
        /// Cookie file path
        path: String,
    },
}

/// Indexer protocol
#[cfg(feature = "_validation")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum IndexerProtocol {
    /// A Bitcoin Core node
    Bitcoind,
    /// An indexer implementing the electrum protocol
    Electrum,
    /// An indexer implementing the esplora protocol
    Esplora,
}

#[cfg(feature = "_validation")]
impl fmt::Display for IndexerProtocol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg_attr(
    feature = "_validation",
    doc = "Options for the [`validate_consignment`] and [`validate_consignment_with_resolver`] \
           functions."
)]
#[cfg_attr(
    not(feature = "_validation"),
    doc = "Options for the [`validate_consignment_with_resolver`] function."
)]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct ValidationOptions {
    /// Bitcoin network the consignment is expected to be for
    pub bitcoin_network: BitcoinNetwork,
    /// TXID of a witness transaction to be taken from the consignment, as it may not have been
    /// broadcast yet, instead of being resolved by the indexer.
    ///
    /// The transaction is accepted as it is in the consignment, without checking it against the
    /// chain: it may never be broadcast, or conflict with one that is. Validation fails with an
    /// [`Error::OffchainTxidNotFound`] if the consignment has no witness with this TXID.
    pub offchain_txid: Option<String>,
    /// EVM JSON-RPC endpoints to confirm external anchors (mint events of a BFA asset) and read
    /// reject lists kept by EVM contracts with, by the EIP-155 chain ID of the chain they are
    /// connected to
    pub evm_rpc_urls: HashMap<u64, String>,
}

#[cfg_attr(
    feature = "_validation",
    doc = "Result of validating an RGB consignment, see [`validate_consignment`]."
)]
#[cfg_attr(
    not(feature = "_validation"),
    doc = "Result of validating an RGB consignment, see [`validate_consignment_with_resolver`]."
)]
///
/// A consignment that fails validation is reported as an [`Error::ConsignmentValidationFailed`]
/// instead, so a result is only returned for a valid one, which may still have warnings.
///
/// A notable exception is when [`Self::safe_height`] is `None`: no confirmation policy was
/// applied, so the consignment may rely on witness transactions that are unconfirmed (or not even
/// broadcast) and may never be mined, e.g. because they get double spent, which would make it
/// invalid.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct ConsignmentValidation {
    /// ID of the consignment
    pub consignment_id: String,
    /// Whether this is a transfer consignment (`false` for a contract consignment)
    pub is_transfer: bool,
    /// ID of the asset
    pub asset_id: String,
    /// Schema of the asset
    pub asset_schema: AssetSchema,
    /// Height the witnesses were checked against: the ones mined above it, or not mined at all,
    /// are reported in a [`ValidationWarning::UnsafeHistory`]. `None` if no confirmation policy
    /// was applied, in which case no witness is reported, whatever its status: check the
    /// [`Self::witnesses`] to apply one. With no confirmation policy applied, the consignment may
    /// be invalid, as it may rely on witness transactions that will never be mined
    pub safe_height: Option<u32>,
    /// Witness transactions of the consignment, sorted by TXID
    pub witnesses: Vec<ValidatedWitness>,
    /// Validation warnings, empty if there's none
    pub warnings: Vec<ValidationWarning>,
}

/// A witness transaction of a validated consignment, see [`ConsignmentValidation::witnesses`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct ValidatedWitness {
    /// TXID of the witness transaction
    pub txid: String,
    /// Status of the witness transaction
    pub status: WitnessValidationStatus,
    /// Whether the witness transaction has been taken from the consignment (as it's the one with
    /// the [`ValidationOptions::offchain_txid`]), without checking it against the chain
    pub from_consignment: bool,
}

/// Status of a witness transaction of a validated consignment, see [`ValidatedWitness::status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub enum WitnessValidationStatus {
    /// Mined in a block
    Mined {
        /// Height of the block
        height: u32,
        /// Timestamp of the block
        timestamp: i64,
    },
    /// Not mined, e.g. in the mempool, or not broadcast yet if taken from the consignment
    Tentative,
    /// To be ignored, as the resolver reported it
    Ignored,
    /// Archived (e.g. replaced or double spent), as the resolver reported it
    Archived,
}

impl From<WitnessOrd> for WitnessValidationStatus {
    fn from(ord: WitnessOrd) -> Self {
        match ord {
            WitnessOrd::Mined(pos) => Self::Mined {
                height: pos.height().get(),
                timestamp: pos.timestamp(),
            },
            WitnessOrd::Tentative => Self::Tentative,
            WitnessOrd::Ignored => Self::Ignored,
            WitnessOrd::Archived => Self::Archived,
        }
    }
}

/// A warning of a validated consignment, see [`ConsignmentValidation::warnings`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub enum ValidationWarning {
    /// Witness transactions mined above the [`ConsignmentValidation::safe_height`], or not mined
    UnsafeHistory {
        /// TXIDs of the witness transactions, sorted
        txids: Vec<String>,
    },
    /// The SPV proof carried by the consignment for a bundle is invalid, so its witness
    /// transaction has been resolved without it
    InvalidSpvProof {
        /// ID of the bundle
        bundle_id: String,
        /// TXID of the witness transaction
        txid: String,
    },
    /// Another warning
    Other {
        /// Warning details
        details: String,
    },
}

impl From<&Warning> for ValidationWarning {
    fn from(warning: &Warning) -> Self {
        match warning {
            Warning::UnsafeHistory(unsafe_history) => {
                let mut txids: Vec<String> = unsafe_history
                    .values()
                    .flatten()
                    .map(|txid| txid.to_string())
                    .collect();
                txids.sort();
                Self::UnsafeHistory { txids }
            }
            Warning::InvalidSpvProof(bundle_id, txid) => Self::InvalidSpvProof {
                bundle_id: bundle_id.to_string(),
                txid: txid.to_string(),
            },
            Warning::Custom(details) => Self::Other {
                details: details.clone(),
            },
        }
    }
}

impl ConsignmentValidation {
    /// Report the given validated consignment, validated with the given safe height, having the
    /// witness with the given ID (if any) taken from the consignment.
    pub(crate) fn new<const TRANSFER: bool>(
        valid_consignment: &ValidConsignment<TRANSFER>,
        safe_height: Option<NonZeroU32>,
        offchain_witness_id: Option<RgbTxid>,
    ) -> Self {
        let status = valid_consignment.validation_status();
        let mut witnesses: Vec<ValidatedWitness> = status
            .tx_ord_map
            .iter()
            .map(|(txid, ord)| ValidatedWitness {
                txid: txid.to_string(),
                status: (*ord).into(),
                from_consignment: offchain_witness_id == Some(*txid),
            })
            .collect();
        witnesses.sort_by(|a, b| a.txid.cmp(&b.txid));
        Self {
            consignment_id: valid_consignment.consignment_id().to_string(),
            is_transfer: TRANSFER,
            asset_id: valid_consignment.contract_id().to_string(),
            asset_schema: AssetSchema::try_from(valid_consignment.schema_id())
                .expect("validated consignment has a supported schema"),
            safe_height: safe_height.map(NonZeroU32::get),
            witnesses,
            warnings: status
                .warnings
                .iter()
                .map(ValidationWarning::from)
                .collect(),
        }
    }
}

/// Where a contract's reject list is published.
#[derive(Debug, Clone)]
pub enum RejectListSource {
    /// A list of opouts served at a URL
    Url(String),
    /// A list of operation IDs kept by an `IRgbRejectList` EVM contract
    Evm(EvmContract),
}

impl From<RejectListLocation> for RejectListSource {
    fn from(location: RejectListLocation) -> Self {
        match location {
            RejectListLocation::Url(url) => Self::Url(url.to_string()),
            RejectListLocation::Evm(contract) => Self::Evm(contract),
        }
    }
}

/// The decisions of a reject list about single opouts, each holding only its latest decision.
///
/// This only tells whether an opout is listed: which decision applies to an allocation, given the
/// ones along its history, is decided when walking the history (see `check_dag`).
#[derive(Debug, Default)]
pub struct RejectList {
    pub(crate) reject_opouts: HashSet<Opout>,
    pub(crate) allow_opouts: HashSet<Opout>,
}

impl RejectList {
    /// Parse a reject list served at a URL: one opout per line, rejected or, if prefixed by `!`,
    /// allowed (the last line for an opout wins). Each line is trimmed of leading and trailing
    /// whitespace, and empty lines are skipped. Invalid lines (e.g. comments) are ignored and
    /// counted.
    pub(crate) fn parse(list: &str) -> (Self, usize) {
        let mut opout_map = HashMap::new();
        let mut invalid = 0;
        for line in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (is_allow, opout_str) = line.strip_prefix("!").map_or((false, line), |s| (true, s));
            match Opout::from_str(opout_str) {
                Ok(opout) => {
                    opout_map.insert(opout, is_allow);
                }
                Err(_) => invalid += 1,
            }
        }
        let (allow_opouts, reject_opouts) = opout_map.into_iter().fold(
            (HashSet::new(), HashSet::new()),
            |(mut allow, mut reject), (o, allowed)| {
                if allowed {
                    allow.insert(o);
                } else {
                    reject.insert(o);
                }
                (allow, reject)
            },
        );
        (
            Self {
                reject_opouts,
                allow_opouts,
            },
            invalid,
        )
    }

    /// Read the reject list kept by the `IRgbRejectList` contract at `address`.
    pub(crate) fn from_evm(evm_client: &EvmClient, address: &EvmAddress) -> Result<Self, Error> {
        let (reject_opouts, allow_opouts) = evm_client.reject_list_entries(address)?.decisions();
        Ok(Self {
            reject_opouts,
            allow_opouts,
        })
    }

    pub(crate) fn rejects(&self, opout: &Opout) -> bool {
        self.reject_opouts.contains(opout)
    }

    pub(crate) fn allows(&self, opout: &Opout) -> bool {
        self.allow_opouts.contains(opout)
    }
}

/// Client of the indexer described by an [`IndexerConfig`], used to validate consignments without
/// a wallet.
#[cfg(feature = "_validation")]
pub(crate) enum IndexerClient {
    #[cfg(feature = "bitcoind_validation")]
    Bitcoind(Box<BitcoindRpcClient>),
    #[cfg(feature = "electrum_validation")]
    Electrum(Box<ElectrumClient>),
    #[cfg(feature = "esplora_validation")]
    Esplora(Box<EsploraClient>),
}

#[cfg(feature = "_validation")]
impl IndexerClient {
    /// Build the client of the indexer described by `indexer_config`, checking it can be reached
    /// and, for a Bitcoin Core node, it has a synced transaction index.
    ///
    /// Fails if support for the configured indexer is not compiled in.
    pub(crate) fn new(indexer_config: &IndexerConfig) -> Result<Self, Error> {
        let unreachable = |e: &dyn fmt::Display| Error::InvalidIndexer {
            details: unreachable_indexer_details(indexer_config, e),
        };
        let client = match indexer_config {
            #[cfg(feature = "bitcoind_validation")]
            IndexerConfig::Bitcoind { url, auth } => Self::Bitcoind(Box::new(
                BitcoindRpcClient::new(url, auth.into()).map_err(|e| unreachable(&e))?,
            )),
            #[cfg(feature = "electrum_validation")]
            IndexerConfig::Electrum { url } => {
                let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
                Self::Electrum(Box::new(
                    ElectrumClient::from_config(url, electrum_config())
                        .map_err(|e| unreachable(&e))?,
                ))
            }
            #[cfg(feature = "esplora_validation")]
            IndexerConfig::Esplora { url } => {
                Self::Esplora(Box::new(EsploraClient::from_builder(esplora_config(url))))
            }
            #[cfg(not(all(
                feature = "bitcoind_validation",
                feature = "electrum_validation",
                feature = "esplora_validation"
            )))]
            _ => return Err(unsupported_indexer(indexer_config)),
        };

        // this also checks connectivity (and credentials for bitcoind)
        client.block_hash(0).map_err(|e| unreachable(&e))?;
        #[allow(irrefutable_let_patterns)]
        #[cfg(feature = "bitcoind_validation")]
        if let Self::Bitcoind(client) = &client {
            checked_bitcoind_indexes(client)?;
        }

        Ok(client)
    }

    fn block_hash(&self, height: usize) -> Result<String, IndexerError> {
        Ok(match self {
            #[cfg(feature = "bitcoind_validation")]
            Self::Bitcoind(client) => client.get_block_hash(height as u64)?.to_string(),
            #[cfg(feature = "electrum_validation")]
            Self::Electrum(client) => client.block_header(height)?.block_hash().to_string(),
            #[cfg(feature = "esplora_validation")]
            Self::Esplora(client) => client.get_block_hash(height as u32)?.to_string(),
        })
    }

    pub(crate) fn get_latest_block_height(&self) -> Result<u32, Error> {
        Ok(match self {
            #[cfg(feature = "bitcoind_validation")]
            Self::Bitcoind(client) => {
                let height = client.get_block_count().map_err(IndexerError::from)?;
                u32::try_from(height).map_err(|_| Error::Indexer {
                    details: s!("bitcoind returned invalid height"),
                })?
            }
            #[cfg(feature = "electrum_validation")]
            Self::Electrum(client) => {
                let header = client
                    .block_headers_subscribe()
                    .map_err(IndexerError::from)?;
                u32::try_from(header.height).map_err(|_| Error::Indexer {
                    details: s!("electrs returned invalid height"),
                })?
            }
            #[cfg(feature = "esplora_validation")]
            Self::Esplora(client) => client.get_height().map_err(IndexerError::from)?,
        })
    }
}

/// Status of the Bitcoin Core node indexes relevant to RGB.
#[cfg(feature = "bitcoind_validation")]
pub(crate) struct BitcoindIndexes {
    #[cfg_attr(not(feature = "bitcoind"), allow(dead_code))]
    pub(crate) block_filters: bool,
}

/// Which indexes the Bitcoin Core node maintains and has finished building, failing if it has no
/// synced transaction index, needed as RGB witnesses are looked up by TXID alone.
#[cfg(feature = "bitcoind_validation")]
pub(crate) fn checked_bitcoind_indexes(
    client: &BitcoindRpcClient,
) -> Result<BitcoindIndexes, Error> {
    let info = client.get_index_info().map_err(IndexerError::from)?;
    let synced = |status: Option<BitcoindIndexStatus>| matches!(status, Some(s) if s.synced);
    if !synced(info.txindex) {
        return Err(Error::InvalidIndexer {
            details: s!("bitcoind has no synced transaction index (-txindex)"),
        });
    }
    Ok(BitcoindIndexes {
        block_filters: synced(info.basic_block_filter_index),
    })
}

/// Build the witness resolver for the indexer described by `indexer_config`, checking it's for the
/// given network.
#[cfg(feature = "_validation")]
pub(crate) fn get_resolver(
    indexer_config: &IndexerConfig,
    bitcoin_network: BitcoinNetwork,
) -> Result<AnyResolver, Error> {
    let invalid = |details: String| Error::InvalidIndexer { details };
    let resolver = match indexer_config {
        #[cfg(feature = "bitcoind_validation")]
        IndexerConfig::Bitcoind { url, auth } => {
            AnyResolver::bitcoind_blocking(url, auth.into()).map_err(invalid)?
        }
        #[cfg(feature = "electrum_validation")]
        IndexerConfig::Electrum { url } => {
            AnyResolver::electrum_blocking(url, Some(electrum_config())).map_err(invalid)?
        }
        #[cfg(feature = "esplora_validation")]
        IndexerConfig::Esplora { url } => AnyResolver::esplora_blocking(esplora_config(url))
            .expect("esplora_blocking wraps an infallible builder and always returns Ok"),
        #[cfg(not(all(
            feature = "bitcoind_validation",
            feature = "electrum_validation",
            feature = "esplora_validation"
        )))]
        _ => return Err(unsupported_indexer(indexer_config)),
    };
    resolver
        .check_chain_net(bitcoin_network.into())
        .map_err(|e| invalid(e.to_string()))?;
    Ok(resolver)
}

/// Details for the [`Error::InvalidIndexer`] raised when the service at `indexer_config` cannot be
/// reached or doesn't speak the configured protocol.
#[cfg(feature = "_validation")]
pub(crate) fn unreachable_indexer_details(
    indexer_config: &IndexerConfig,
    err: &(impl fmt::Display + ?Sized),
) -> String {
    match indexer_config {
        IndexerConfig::Bitcoind { .. } => format!("cannot connect to bitcoind: {err}"),
        IndexerConfig::Electrum { .. } => s!("not a valid electrum server"),
        IndexerConfig::Esplora { .. } => s!("not a valid esplora server"),
    }
}

/// The [`Error::InvalidIndexer`] raised when support for the indexer at `indexer_config` is not
/// compiled in.
#[cfg(not(all(
    feature = "bitcoind_validation",
    feature = "electrum_validation",
    feature = "esplora_validation"
)))]
#[cfg(feature = "_validation")]
fn unsupported_indexer(indexer_config: &IndexerConfig) -> Error {
    let feature = match indexer_config {
        IndexerConfig::Bitcoind { .. } => "bitcoind",
        IndexerConfig::Electrum { .. } => "electrum",
        IndexerConfig::Esplora { .. } => "esplora",
    };
    Error::InvalidIndexer {
        details: format!(
            "{feature} support is not enabled (requires the `{feature}_validation` feature)"
        ),
    }
}

#[cfg(feature = "bitcoind_validation")]
impl From<&BitcoindAuth> for BitcoindRpcAuth {
    fn from(auth: &BitcoindAuth) -> Self {
        match auth {
            BitcoindAuth::UserPass { username, password } => {
                BitcoindRpcAuth::UserPass(username.clone(), password.clone())
            }
            BitcoindAuth::CookieFile { path } => BitcoindRpcAuth::CookieFile(PathBuf::from(path)),
        }
    }
}

#[cfg(feature = "electrum_validation")]
pub(crate) fn electrum_config() -> ElectrumConfig {
    ConfigBuilder::new()
        .retry(INDEXER_RETRIES)
        .timeout(Some(Duration::from_secs(INDEXER_TIMEOUT)))
        .build()
}

#[cfg(feature = "esplora_validation")]
pub(crate) fn esplora_config(indexer_url: &str) -> EsploraBuilder {
    EsploraBuilder::new(indexer_url)
        .max_retries(INDEXER_RETRIES.into())
        .timeout(INDEXER_TIMEOUT)
}

/// Validate the RGB consignment (transfer or contract) at the given path, without a wallet.
///
/// The witness transactions are resolved with the indexer described by `indexer_config`, which
/// must be for the [`ValidationOptions::bitcoin_network`], except for the one with the
/// [`ValidationOptions::offchain_txid`], which is taken from the consignment as it may not have
/// been broadcast yet. If the consignment has external anchors (mint events of a BFA asset),
/// they're confirmed on the chain the asset's bridge lives on, through the EVM RPC URL configured
/// for its chain ID in [`ValidationOptions::evm_rpc_urls`].
///
/// When `min_confirmations` is provided, witnesses having less confirmations, and unconfirmed
/// ones in any case (even with 0), are reported in a warning of the returned
/// [`ConsignmentValidation`].
/// Without it no witness is reported, whatever its status, see
/// [`ConsignmentValidation::witnesses`].
///
/// An [`Error::ConsignmentValidationFailed`] is returned if the consignment is invalid.
///
/// The validation relies on the following trust assumptions:
/// - the indexer is trusted to report the witness transactions and their status (including the
///   block headers the SPV proofs carried by the consignment are checked against) correctly
/// - the EVM RPC endpoints are trusted to report the mint events (and the finalized block) and
///   the reject list entries correctly
/// - the reject list server is trusted to serve the asset's reject list correctly
/// - the witness transaction with the [`ValidationOptions::offchain_txid`] is accepted as it is in
///   the consignment, without checking it against the chain: it may never be broadcast, or
///   conflict with one that is
///
/// If the asset has a reject list, the consignment terminals are checked against it, as a wallet
/// receiving them does: an [`Error::ConsignmentRejected`] is returned if any of them has a rejected
/// opout along its history (unless an allowed one shields it). A reject list kept by an EVM
/// contract is read through the EVM RPC URL configured for its chain ID in
/// [`ValidationOptions::evm_rpc_urls`].
#[cfg(feature = "_validation")]
pub fn validate_consignment(
    consignment_path: &str,
    indexer_config: &IndexerConfig,
    min_confirmations: Option<u8>,
    options: &ValidationOptions,
) -> Result<ConsignmentValidation, Error> {
    let indexer = IndexerClient::new(indexer_config)?;
    let resolver = get_resolver(indexer_config, options.bitcoin_network)?;
    let safe_height = min_confirmations
        .map(|min_confirmations| -> Result<_, Error> {
            Ok(indexer
                .get_latest_block_height()?
                .saturating_sub(min_confirmations as u32)
                + 1)
        })
        .transpose()?
        .and_then(NonZeroU32::new);
    validate_consignment_with_resolver(
        consignment_path,
        &ChainResolver { inner: &resolver },
        safe_height,
        options,
    )
}

/// Validate the RGB consignment (transfer or contract) at the given path, without a wallet,
/// resolving its witness transactions with the provided resolver, which must be for the
/// [`ValidationOptions::bitcoin_network`].
#[doc = ""]
#[cfg_attr(
    feature = "_validation",
    doc = "This works like [`validate_consignment`], which uses the indexer it's given: the \
           witness with"
)]
#[cfg_attr(
    not(feature = "_validation"),
    doc = "This works like `validate_consignment` (available with an indexer validation \
           feature), which uses an indexer: the witness with"
)]
/// the [`ValidationOptions::offchain_txid`] is taken from the consignment, as it may not have been
/// broadcast yet, external anchors are confirmed through the [`ValidationOptions::evm_rpc_urls`]
/// and the terminals are checked against the asset's reject list (if any). When `safe_height` is
/// provided, witnesses mined above it (including unconfirmed ones) are reported in a warning. The
/// same trust assumptions apply, with the provided resolver trusted in place of the indexer.
///
/// <div class="warning">This method is meant for special usage and is normally not needed, use
/// it only if you know what you're doing</div>
pub fn validate_consignment_with_resolver(
    consignment_path: &str,
    resolver: &dyn ResolveWitness,
    safe_height: Option<NonZeroU32>,
    options: &ValidationOptions,
) -> Result<ConsignmentValidation, Error> {
    check_file_exists(consignment_path)?;
    let offchain_witness_id = options
        .offchain_txid
        .as_deref()
        .map(|txid| RgbTxid::from_str(txid).map_err(|_| Error::InvalidTxid))
        .transpose()?;
    match UniversalFile::load_file(consignment_path) {
        Ok(UniversalFile::Transfer(consignment)) => validate_loaded_consignment(
            &consignment,
            resolver,
            safe_height,
            offchain_witness_id,
            options,
        ),
        Ok(UniversalFile::Contract(consignment)) => validate_loaded_consignment(
            &consignment,
            resolver,
            safe_height,
            offchain_witness_id,
            options,
        ),
        _ => Err(Error::InvalidConsignment),
    }
}

/// Validate the given consignment, see [`validate_consignment_with_resolver`], then check its
/// terminals against the reject list of its asset (if any).
fn validate_loaded_consignment<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
    resolver: &dyn ResolveWitness,
    safe_height: Option<NonZeroU32>,
    offchain_witness_id: Option<RgbTxid>,
    options: &ValidationOptions,
) -> Result<ConsignmentValidation, Error> {
    // a TXID matching no witness would silently fall back to the resolver. This is generic over
    // the consignment kind only for simplicity: a contract consignment carries the genesis only
    // (validation refuses one with bundles), so it has no witness and any offchain TXID is refused
    if let Some(witness_id) = offchain_witness_id
        && !consignment
            .bundled_witnesses()
            .any(|bw| bw.witness_id() == witness_id)
    {
        return Err(Error::OffchainTxidNotFound {
            txid: witness_id.to_string(),
        });
    }

    let asset_schema = AssetSchema::try_from(consignment.schema_id())?;
    let validation_config = ValidationConfig {
        chain_net: options.bitcoin_network.into(),
        build_opouts_dag: asset_schema.supports_reject_list(),
        safe_height,
    };
    let valid_consignment = validate_consignment_internal(
        consignment,
        offchain_witness_id,
        resolver,
        &options.evm_rpc_urls,
        &validation_config,
    )?;

    // the reject list is a genesis global, so it can be read off the consignment
    if let Some(source) = genesis_reject_list_source(asset_schema, consignment.genesis())? {
        let reject_list = match &source {
            RejectListSource::Url(url) => {
                RejectList::parse(&RejectListClient::new(url)?.get_reject_list()?).0
            }
            RejectListSource::Evm(contract) => RejectList::from_evm(
                &checked_evm_client(&options.evm_rpc_urls, contract.chain_id)?,
                &contract.address,
            )?,
        };
        let dag_data = valid_consignment
            .validation_status()
            .dag_data_opt
            .as_ref()
            .expect("build_opouts_dag is set for schemas supporting a reject list");
        let rejected = rejected_opouts(dag_data, &reject_list, &terminal_opouts(consignment))?;
        if !rejected.is_empty() {
            let mut opouts: Vec<String> = rejected.iter().map(|o| o.to_string()).collect();
            opouts.sort();
            return Err(Error::ConsignmentRejected { opouts });
        }
    }

    Ok(ConsignmentValidation::new(
        &valid_consignment,
        safe_height,
        offchain_witness_id,
    ))
}

/// The wallet's on-chain witness resolver (the indexer-backed [`AnyResolver`]).
#[cfg(feature = "_validation")]
#[doc(hidden)]
pub struct ChainResolver<'a> {
    pub(crate) inner: &'a AnyResolver,
}

#[cfg(feature = "_validation")]
impl ResolveWitness for ChainResolver<'_> {
    fn resolve_witness(&self, witness_id: RgbTxid) -> Result<WitnessStatus, WitnessResolverError> {
        #[cfg(all(test, feature = "_online"))]
        if let Some((tx, height)) = mock_chain_tx(&witness_id.to_string()) {
            let height = NonZeroU32::new(height).expect("mock block height cannot be 0");
            let pos = WitnessPos::bitcoin(height, now().unix_timestamp())
                .expect("current time is after the genesis block");
            return Ok(WitnessStatus::Resolved(tx, WitnessOrd::Mined(pos)));
        }
        self.inner.resolve_witness(witness_id)
    }
    fn get_block_header(&self, height: NonZeroU32) -> Result<BlockHeader, WitnessResolverError> {
        self.inner.get_block_header(height)
    }
    fn check_chain_net(&self, chain_net: ChainNet) -> Result<(), WitnessResolverError> {
        self.inner.check_chain_net(chain_net)
    }
}

/// Witness resolver looking up the witness with the given ID (if any) in the consignment, as it may
/// not have been broadcast yet, and the other witnesses with the fallback chain resolver.
pub(crate) struct OffchainResolver<'cons, 'r, R: ResolveWitness + ?Sized, const TRANSFER: bool> {
    pub(crate) witness_id: Option<RgbTxid>,
    pub(crate) consignment: &'cons Consignment<TRANSFER>,
    pub(crate) fallback: &'r R,
}

impl<R: ResolveWitness + ?Sized, const TRANSFER: bool> ResolveWitness
    for OffchainResolver<'_, '_, R, TRANSFER>
{
    fn resolve_witness(&self, witness_id: RgbTxid) -> Result<WitnessStatus, WitnessResolverError> {
        if Some(witness_id) != self.witness_id {
            #[cfg(all(test, feature = "_online"))]
            if spv_only() {
                return Err(WitnessResolverError::ResolverIssue(
                    Some(witness_id),
                    s!("witness TX lookup refused (mock)"),
                ));
            }
            return self.fallback.resolve_witness(witness_id);
        }
        self.consignment
            .bundled_witnesses()
            .find(|bw| bw.witness_id() == witness_id)
            .map(|p| p.tx.clone())
            .map_or_else(
                || self.fallback.resolve_witness(witness_id),
                |tx| Ok(WitnessStatus::Resolved(tx, WitnessOrd::Tentative)),
            )
    }
    fn get_block_header(&self, height: NonZeroU32) -> Result<BlockHeader, WitnessResolverError> {
        // lets the SPV proofs carried by the consignment be verified via the indexer headers
        self.fallback.get_block_header(height)
    }
    fn check_chain_net(&self, chain_net: ChainNet) -> Result<(), WitnessResolverError> {
        self.fallback.check_chain_net(chain_net)
    }
}

/// Validate the given consignment with the given validation config, resolving the witness with
/// the given ID (if any) from the consignment itself, as it may not have been broadcast yet, and
/// the other witnesses with the given chain resolver.
///
/// External anchors (mint events) are confirmed on the chain the asset's bridge lives on, through
/// the EVM RPC URL configured for it in `evm_rpc_urls`, scanning for each mint event at most
/// [`MAX_MINT_SCAN_BLOCKS`] blocks after the one it commits to.
pub(crate) fn validate_consignment_internal<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
    offchain_witness_id: Option<RgbTxid>,
    chain_resolver: &(impl ResolveWitness + ?Sized),
    evm_rpc_urls: &HashMap<u64, String>,
    validation_config: &ValidationConfig,
) -> Result<ValidConsignment<TRANSFER>, Error> {
    let validation_error = |e: ValidationError| match e {
        ValidationError::InvalidConsignment(e) => Error::ConsignmentValidationFailed {
            details: e.to_string(),
        },
        ValidationError::ResolverError(e) => Error::Network {
            details: e.to_string(),
        },
        e => Error::Internal {
            details: format!("unexpected consignment validation error: {e}"),
        },
    };

    let asset_schema = AssetSchema::try_from(consignment.schema_id())?;
    // phase 1: everything that only depends on the consignment itself
    let mut pending = consignment
        .clone()
        .validate_deterministic(&asset_schema.schema_rules(), validation_config)
        .map_err(validation_error)?;

    // phase 2: resolve what depends on the chains, then finalize. External anchors (mint events)
    // live on the chain the asset's bridge contract is deployed on and cannot be resolved by the
    // witness resolver: confirm them against that chain before resolving the witnesses
    let anchors: Vec<ExternalAnchor> = pending.pending().unresolved_anchors().cloned().collect();
    if !anchors.is_empty() {
        if !asset_schema.supports_mint() {
            return Err(Error::ConsignmentValidationFailed {
                details: format!("{asset_schema} schema does not support external anchors"),
            });
        }
        let bridge_location = BridgedFungibleAsset::bridge_location(consignment.genesis())
            .map_err(|e| Error::Internal {
                details: format!("unexpected bridge location error: {e}"),
            })?;
        let BridgeLocation::Evm(EvmContract { chain_id, .. }) = &bridge_location;
        let evm_rpc_url = evm_rpc_urls.get(chain_id).ok_or(Error::NoEvmRpcUrl {
            chain_id: *chain_id,
        })?;
        let evm_client = EvmClient::new(evm_rpc_url)?;
        let bounded_resolver = BoundedAnchorResolver::new(&evm_client, max_mint_scan_blocks());
        let anchor_resolver = CheckedAnchorResolver::with(&bounded_resolver, bridge_location)?;
        for anchor in &anchors {
            match pending.resolve_anchor(&anchor_resolver, anchor) {
                // tell a mint that will never be confirmed from one that may still be
                Err(AnchorResolverError::Unconfirmed(opid))
                    if bounded_resolver.is_exhausted(&opid) =>
                {
                    return Err(Error::MintEventOutOfScanWindow {
                        opid: opid.to_string(),
                        max_scan_blocks: max_mint_scan_blocks(),
                    });
                }
                res => res?,
            }
        }
    }

    // then resolve the witnesses
    let resolver = OffchainResolver {
        witness_id: offchain_witness_id,
        consignment,
        fallback: chain_resolver,
    };
    pending
        .pending()
        .resolve_all(&resolver)
        .map_err(validation_error)?;
    Ok(pending.finalize())
}

/// The EVM RPC client for the chain with the given `chain_id`, among the given `evm_rpc_urls`,
/// checked to be connected to it.
pub(crate) fn checked_evm_client(
    evm_rpc_urls: &HashMap<u64, String>,
    chain_id: u64,
) -> Result<EvmClient, Error> {
    let evm_rpc_url = evm_rpc_urls
        .get(&chain_id)
        .ok_or(Error::NoEvmRpcUrl { chain_id })?;
    let evm_client = EvmClient::new(evm_rpc_url)?;
    let actual = evm_client.evm_chain_id()?;
    if actual != chain_id {
        return Err(Error::EvmChainIdMismatch {
            expected: chain_id,
            actual,
        });
    }
    Ok(evm_client)
}

/// Where the reject list of an asset with the given schema and genesis is published, if any.
///
/// The reject list is a genesis global, so it can be read off a consignment without a stock.
pub(crate) fn genesis_reject_list_source(
    asset_schema: AssetSchema,
    genesis: &Genesis,
) -> Result<Option<RejectListSource>, Error> {
    let global = |global_type: GlobalStateType| {
        genesis
            .globals
            .get(&global_type)
            .and_then(|values| values.first())
            .map(|value| value.to_inner())
    };
    let invalid = |e: DeserializeError| Error::Internal {
        details: format!("invalid reject list in a validated genesis: {e}"),
    };
    Ok(match asset_schema {
        AssetSchema::Bfa => global(GS_REJECT_LIST)
            .map(|data| RejectListLocation::from_strict_serialized(data).map_err(invalid))
            .transpose()?
            .map(RejectListSource::from),
        AssetSchema::Ifa => global(GS_REJECT_LIST_URL)
            .map(|data| RejectListUrl::from_strict_serialized(data).map_err(invalid))
            .transpose()?
            .map(|url| RejectListSource::Url(url.to_string())),
        AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa => None,
    })
}

/// The opouts assigned to the terminals of the given consignment.
pub(crate) fn terminal_opouts<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
) -> HashSet<Opout> {
    let mut opouts = HashSet::new();
    for witness_bundle in consignment.bundles.iter() {
        let Some(terminal_seals) = consignment
            .terminals
            .get(&witness_bundle.bundle.bundle_id())
        else {
            continue;
        };
        let terminal_seals: HashSet<SecretSeal> = terminal_seals
            .into_iter()
            .map(|s| s.to_secret_seal())
            .collect();
        for KnownTransition { opid, transition } in witness_bundle.bundle.known_transitions.iter() {
            for (ass_type, typed_assigns) in transition.assignments.iter() {
                for no in 0..typed_assigns.len_u16() {
                    let seal = typed_assigns.seal_at(no).expect("index in range");
                    if terminal_seals.contains(&seal.to_secret_seal()) {
                        opouts.insert(Opout::new(*opid, *ass_type, no));
                    }
                }
            }
        }
    }
    opouts
}

/// The opouts, among the given `check_opouts`, the given reject list rejects: the ones having a
/// rejected opout along their history (the opouts DAG built by the validation), unless an allowed
/// opout shields it.
pub(crate) fn rejected_opouts(
    dag_data: &OpoutsDagData,
    reject_list: &RejectList,
    check_opouts: &HashSet<Opout>,
) -> Result<HashSet<Opout>, Error> {
    let (dag, index) = dag_data;
    let mut to_reject = HashSet::new();

    // for each opout we are checking, traverse its ancestor chain
    for check_opout in check_opouts {
        let &opout_node = index.get(check_opout).ok_or(Error::Internal {
            details: s!("opout not found in DAG"),
        })?;

        // traverse from this node to its ancestors, depth first
        let mut stack = vec![opout_node];
        let mut visited = HashSet::new();
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            let node_opout = &dag[node];
            // allow shields this path upward: do not traverse this branch further
            if reject_list.allows(node_opout) {
                continue;
            }
            // encountering a reject node on an unshielded path: reject
            if reject_list.rejects(node_opout) {
                to_reject.insert(*check_opout);
                break;
            }
            for (_edge, parent) in dag.parents(node).iter(dag) {
                stack.push(parent);
            }
        }
    }
    Ok(to_reject)
}

#[cfg(all(test, feature = "electrum_validation"))]
mod tests {
    use super::*;

    const GENESIS_HEADER_HEX: &str = "0100000000000000000000000000000000000000000000000000000000000000000000003ba3edfd7a7b12b27ac72c3e67768f617fc81bc3888a51323a9fb8aa4b1e5e4a29ab5f49ffff001d1dac2b7c";

    /// Start a mock electrum server that accepts a single connection, refusing any further one,
    /// and answers the requests needed to check it can be reached, returning its URL.
    fn start_single_connection_electrum_mock() -> String {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock electrum");
        let url = format!(
            "tcp://{}",
            listener.local_addr().expect("mock electrum addr")
        );
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("mock electrum accept");
            // close the listener before answering, so the next connection is refused
            drop(listener);
            let mut reader = BufReader::new(stream.try_clone().expect("clone mock stream"));
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                let id: String = line
                    .split_once(r#""id":"#)
                    .expect("mock electrum request id")
                    .1
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                let result = if line.contains("server.version") {
                    s!(r#"["mock-electrum","1.6"]"#)
                } else if line.contains("blockchain.block.header") {
                    format!(r#""{GENESIS_HEADER_HEX}""#)
                } else {
                    return;
                };
                if writeln!(stream, r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#).is_err() {
                    return;
                }
                line.clear();
            }
        });
        url
    }

    #[test]
    fn validate_consignment_resolver_connection_refused() {
        // the indexer client connects, then the resolver connection is refused
        let url = start_single_connection_electrum_mock();
        let options = ValidationOptions {
            bitcoin_network: BitcoinNetwork::Regtest,
            offchain_txid: None,
            evm_rpc_urls: HashMap::new(),
        };
        let result =
            validate_consignment("unused", &IndexerConfig::Electrum { url }, None, &options);
        assert_matches!(result, Err(Error::InvalidIndexer { .. }));
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn get_indexer_and_resolver_resolver_connection_refused() {
        // the wallet indexer connects, then the resolver connection is refused
        let url = start_single_connection_electrum_mock();
        let result =
            get_indexer_and_resolver(&IndexerConfig::Electrum { url }, BitcoinNetwork::Regtest);
        // Indexer is not Debug, so the Ok variant cannot be printed by assert_matches
        assert!(matches!(result, Err(Error::InvalidIndexer { .. })));
    }
}
