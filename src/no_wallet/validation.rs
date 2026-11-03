//! Consignment validation.
//!
//! This module defines the consignment validation that doesn't require a wallet (with resolvers
//! provided by the caller, or with network clients with an indexer validation feature, without the
//! `wallet` one), the objects it returns and the indexer support it needs, which is shared with
//! online wallets.

use super::*;

#[cfg(any(feature = "electrum_validation", feature = "esplora_validation"))]
pub(crate) const INDEXER_TIMEOUT: u64 = 10;
#[cfg(any(feature = "electrum_validation", feature = "esplora_validation"))]
pub(crate) const INDEXER_RETRIES: u8 = 3;

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

/// Where a contract's reject list is published, see [`ResolveRejectList`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RejectListSource {
    /// A list of opouts served at a URL
    Url(String),
    /// A list of opouts kept by an `IRgbRejectList` EVM contract
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
/// ones along its history, is decided when walking the history (see `rejected_opouts`).
#[derive(Debug, Default)]
pub struct RejectList {
    pub(crate) reject_opouts: HashSet<Opout>,
    pub(crate) allow_opouts: HashSet<Opout>,
}

impl RejectList {
    /// Build a reject list from the given decisions, in the order they were taken: each opout
    /// rejected or, if `true`, allowed (the last decision for an opout wins).
    pub fn from_decisions(decisions: impl IntoIterator<Item = (Opout, bool)>) -> Self {
        let mut reject_list = Self::default();
        for (opout, allow) in decisions {
            reject_list.insert(opout, allow);
        }
        reject_list
    }

    /// Record a decision about `opout`, replacing any earlier one.
    pub(crate) fn insert(&mut self, opout: Opout, allow: bool) {
        let (decided, replaced) = if allow {
            (&mut self.allow_opouts, &mut self.reject_opouts)
        } else {
            (&mut self.reject_opouts, &mut self.allow_opouts)
        };
        replaced.remove(&opout);
        decided.insert(opout);
    }

    /// Parse a reject list served at a URL: one opout per line, rejected or, if prefixed by `!`,
    /// allowed (the last line for an opout wins). Each line is trimmed of leading and trailing
    /// whitespace, and empty lines are skipped. Invalid lines (e.g. comments) are ignored and
    /// counted.
    ///
    /// Returns the reject list and the number of invalid lines.
    pub fn parse(list: &str) -> (Self, usize) {
        let mut reject_list = Self::default();
        let mut invalid = 0;
        for line in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (is_allow, opout_str) = line.strip_prefix("!").map_or((false, line), |s| (true, s));
            match Opout::from_str(opout_str) {
                Ok(opout) => reject_list.insert(opout, is_allow),
                Err(_) => invalid += 1,
            }
        }
        (reject_list, invalid)
    }

    /// Read the reject list kept by the `IRgbRejectList` contract at `address`.
    #[cfg(feature = "_validation")]
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
/// been broadcast yet. If the consignment has claims (the mint locks of a BFA asset),
/// they're confirmed on the chain the asset's bridge lives on, through the EVM RPC URL configured
/// for its chain ID in `evm_rpc_urls` (EVM JSON-RPC endpoints by the EIP-155 chain ID of the
/// chain they are connected to), reading the lock confirming each mint from the records the
/// bridge contract keeps, as of the latest finalized block. A mint whose lock wasn't included
/// before the block it commits to will never be confirmed, and is reported as an
/// [`Error::MintLockExpired`].
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
/// - the EVM RPC endpoints are trusted to report the bridge contract records (and the finalized
///   block) and the reject list entries correctly
/// - the reject list server is trusted to serve the asset's reject list correctly
/// - the witness transaction with the [`ValidationOptions::offchain_txid`] is accepted as it is in
///   the consignment, without checking it against the chain: it may never be broadcast, or
///   conflict with one that is
///
/// If the asset has a reject list, the operations the consignment proves (its terminal operations,
/// the ones nothing in the consignment builds on, e.g. the last transfer or a burn) are checked
/// against it, inputs and outputs: an [`Error::ConsignmentRejected`] is returned if any of them has
/// a rejected opout along its history, unless an allowed one shields it (an allowed output doesn't
/// shield the inputs of its own operation, which are checked on their own). A wallet receiving a
/// transfer checks the assignments it receives instead. A reject list kept by an EVM contract is
/// read through the EVM RPC URL configured for its chain ID in `evm_rpc_urls`.
#[cfg(feature = "_validation")]
pub fn validate_consignment(
    consignment_path: &str,
    indexer_config: &IndexerConfig,
    evm_rpc_urls: &HashMap<u64, String>,
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
    validate_consignment_file(
        consignment_path,
        &ChainResolver { inner: &resolver },
        ClaimResolution::Evm(evm_rpc_urls),
        Some(&RemoteRejectListResolver {
            evm_rpc_urls,
            evm_chains_checked: false,
        }),
        safe_height,
        options,
    )
}

/// Validate the RGB consignment (transfer or contract) at the given path, without a wallet,
/// resolving its witness transactions with the provided resolver, which must be for the
/// [`ValidationOptions::bitcoin_network`], its claims (the mint locks of a BFA asset) with
/// the provided claim resolver, which must be for the chain the asset's bridge lives on, and the
/// asset's reject list with the provided reject list resolver.
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
/// broadcast yet, and the terminal operations are checked against the asset's reject list (if
/// any). When `safe_height` is provided, witnesses mined above it (including unconfirmed ones) are
/// reported in a warning. The same trust assumptions apply, with the provided resolvers trusted in
/// place of the indexer, the EVM RPC endpoints and the reject list server.
///
/// The claim resolver is only needed if the consignment has claims, and the reject
/// list resolver if the asset has a reject list: an [`Error::NoClaimResolver`] or an
/// [`Error::NoRejectListResolver`] is returned if a needed one isn't provided. It's up to the
/// claim resolver to only confirm a mint once its lock, made for the [`LockId`] of the claim (which
/// commits to the mint operation and to the TX including it), of the claim's amount and included
/// before the block the mint commits to, is final, and a mint it doesn't confirm is reported as an
/// [`Error::MintLockNotFound`], whether or not it may still be confirmed.
///
/// <div class="warning">This method is meant for special usage and is normally not needed, use
/// it only if you know what you're doing</div>
pub fn validate_consignment_with_resolver(
    consignment_path: &str,
    resolver: &dyn ResolveWitness,
    claim_resolver: Option<&dyn ResolveClaim>,
    reject_list_resolver: Option<&dyn ResolveRejectList>,
    safe_height: Option<NonZeroU32>,
    options: &ValidationOptions,
) -> Result<ConsignmentValidation, Error> {
    validate_consignment_file(
        consignment_path,
        resolver,
        ClaimResolution::Resolver(claim_resolver),
        reject_list_resolver,
        safe_height,
        options,
    )
}

/// Validate the consignment at the given path, see [`validate_consignment_with_resolver`].
fn validate_consignment_file(
    consignment_path: &str,
    resolver: &dyn ResolveWitness,
    claim_resolution: ClaimResolution,
    reject_list_resolver: Option<&dyn ResolveRejectList>,
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
            claim_resolution,
            reject_list_resolver,
            safe_height,
            offchain_witness_id,
            options,
        ),
        Ok(UniversalFile::Contract(consignment)) => validate_loaded_consignment(
            &consignment,
            resolver,
            claim_resolution,
            reject_list_resolver,
            safe_height,
            offchain_witness_id,
            options,
        ),
        _ => Err(Error::InvalidConsignment),
    }
}

/// Validate the given consignment, see [`validate_consignment_with_resolver`], then check its
/// terminal operations against the reject list of its asset (if any).
fn validate_loaded_consignment<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
    resolver: &dyn ResolveWitness,
    claim_resolution: ClaimResolution,
    reject_list_resolver: Option<&dyn ResolveRejectList>,
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
        build_ops_dag: asset_schema.supports_reject_list(),
        safe_height,
    };
    let valid_consignment = validate_consignment_internal(
        consignment,
        offchain_witness_id,
        resolver,
        claim_resolution,
        &validation_config,
    )?;

    check_reject_list(
        asset_schema,
        consignment.genesis(),
        valid_consignment.validation_status().ops_dag.as_ref(),
        reject_list_resolver,
        &reject_check_origins(consignment),
    )?;

    Ok(ConsignmentValidation::new(
        &valid_consignment,
        safe_height,
        offchain_witness_id,
    ))
}

/// Trait to read the reject list of an asset, see [`validate_consignment_with_resolver`].
pub trait ResolveRejectList {
    /// The reject list published at the given source.
    ///
    /// The source is taken from the asset's genesis, so it's chosen by the issuer: it may point
    /// to a list that never ends, or to a URL on the local network. It's up to the implementor to
    /// bound the size of the list and the time taken to read it, and to choose which URLs to
    /// request. The resolver `validate_consignment` uses refuses lists longer than 100000
    /// entries, times out, only requests `http` and `https` URLs and doesn't follow redirects, but
    /// it does request a URL on the local network if the genesis states one.
    fn get_reject_list(&self, source: &RejectListSource) -> Result<RejectList, Error>;
}

/// Reject list resolver reading the reject lists served at a URL over HTTP and the ones kept by an
/// EVM contract through the EVM RPC URL configured for its chain ID.
#[cfg(feature = "_validation")]
pub(crate) struct RemoteRejectListResolver<'a> {
    pub(crate) evm_rpc_urls: &'a HashMap<u64, String>,
    /// Whether the EVM RPC endpoints are known to be on the chain they're configured for (a
    /// wallet checks that when going online), so they're not asked which chain they are on
    /// before reading a list
    pub(crate) evm_chains_checked: bool,
}

#[cfg(feature = "_validation")]
impl RemoteRejectListResolver<'_> {
    /// The reject list published at the given source, along with the number of invalid lines
    /// ignored when it's served at a URL (see [`RejectList::parse`]).
    pub(crate) fn get_reject_list_counting(
        &self,
        source: &RejectListSource,
    ) -> Result<(RejectList, usize), Error> {
        Ok(match source {
            RejectListSource::Url(url) => {
                RejectList::parse(&RejectListClient::new(url)?.get_reject_list()?)
            }
            RejectListSource::Evm(contract) => {
                let evm_client = if self.evm_chains_checked {
                    evm_client(self.evm_rpc_urls, contract.chain_id)?
                } else {
                    checked_evm_client(self.evm_rpc_urls, contract.chain_id)?
                };
                (RejectList::from_evm(&evm_client, &contract.address)?, 0)
            }
        })
    }
}

#[cfg(feature = "_validation")]
impl ResolveRejectList for RemoteRejectListResolver<'_> {
    fn get_reject_list(&self, source: &RejectListSource) -> Result<RejectList, Error> {
        Ok(self.get_reject_list_counting(source)?.0)
    }
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

/// How the claims (mint locks) of a consignment are confirmed.
pub(crate) enum ClaimResolution<'a> {
    /// Through the given claim resolver, if any
    Resolver(Option<&'a dyn ResolveClaim>),
    /// Through the EVM RPC URL configured for the chain the asset's bridge lives on, among the
    /// given ones
    #[cfg(feature = "_validation")]
    Evm(&'a HashMap<u64, String>),
}

/// A claim resolver provided as a trait object, as rgb-ops only implements [`ResolveClaim`]
/// for references to sized resolvers.
struct DynClaimResolver<'a>(&'a dyn ResolveClaim);

impl ResolveClaim for DynClaimResolver<'_> {
    fn evm_chain_id(&self) -> Result<u64, ClaimResolverError> {
        self.0.evm_chain_id()
    }
    fn is_confirmed(
        &self,
        location: &BridgeLocation,
        claim: &Claim,
    ) -> Result<bool, ClaimResolverError> {
        self.0.is_confirmed(location, claim)
    }
}

/// Validate the given consignment with the given validation config, resolving all its witnesses
/// with the given chain resolver, except `offchain_witness_id` if it's `Some`: that one is taken
/// from the consignment as tentative, as it may not have been broadcast yet.
///
/// Claims (mint locks) are confirmed on the chain the asset's bridge lives on, as the
/// given claim resolution says: one that is not confirmed makes the validation fail.
pub(crate) fn validate_consignment_internal<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
    offchain_witness_id: Option<RgbTxid>,
    chain_resolver: &(impl ResolveWitness + ?Sized),
    claim_resolution: ClaimResolution,
    validation_config: &ValidationConfig,
) -> Result<ValidConsignment<TRANSFER>, Error> {
    let pending = validate_with_claims(consignment, claim_resolution, validation_config)?;
    resolve_witnesses(
        pending,
        &OffchainResolver {
            witness_id: offchain_witness_id,
            consignment,
            fallback: chain_resolver,
        },
    )
}

/// Phase 1 of the validation of the given consignment with the given validation config (see
/// [`validate_deterministic`]), followed by the confirmation of its claims (mint locks) on the
/// chain the asset's bridge lives on, as the given claim resolution says: one that is not
/// confirmed makes the validation fail. What is left is resolving the witnesses (see
/// [`resolve_witnesses`]).
pub(crate) fn validate_with_claims<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
    claim_resolution: ClaimResolution,
    validation_config: &ValidationConfig,
) -> Result<PendingConsignment<TRANSFER>, Error> {
    let asset_schema = AssetSchema::try_from(consignment.schema_id())?;
    let mut pending = validate_deterministic(consignment, asset_schema, validation_config)?;

    if let Some(PendingClaims {
        bridge_location,
        claims,
    }) = pending_claims(&mut pending, asset_schema)?
    {
        let unconfirmed = match claim_resolution {
            ClaimResolution::Resolver(claim_resolver) => {
                let claim_resolver = claim_resolver.ok_or(Error::NoClaimResolver)?;
                let claim_resolver =
                    CheckedClaimResolver::with(DynClaimResolver(claim_resolver), bridge_location)?;
                // a custom resolver doesn't tell an expired mint from one that may still be confirmed
                resolve_claims(&mut pending, &claim_resolver, &claims, |_| false)?
            }
            #[cfg(feature = "_validation")]
            ClaimResolution::Evm(evm_rpc_urls) => {
                let BridgeLocation::Evm(EvmContract { chain_id, .. }) = &bridge_location;
                let evm_rpc_url = evm_rpc_urls.get(chain_id).ok_or(Error::NoEvmRpcUrl {
                    chain_id: *chain_id,
                })?;
                let evm_client = EvmClient::new(evm_rpc_url)?;
                let mint_resolver = MintClaimResolver::new(&evm_client);
                let claim_resolver = CheckedClaimResolver::with(&mint_resolver, bridge_location)?;
                resolve_claims(&mut pending, &claim_resolver, &claims, |opid| {
                    mint_resolver.is_expired(opid)
                })?
            }
        };
        if let Some(opid) = unconfirmed.first() {
            return Err(Error::MintLockNotFound {
                opid: opid.to_string(),
            });
        }
    }

    Ok(pending)
}

/// Map an error validating a consignment: an invalid consignment is reported as an
/// [`Error::ConsignmentValidationFailed`], a resolver failure as an [`Error::Network`].
fn validation_error(e: ValidationError) -> Error {
    match e {
        ValidationError::InvalidConsignment(e) => Error::ConsignmentValidationFailed {
            details: e.to_string(),
        },
        ValidationError::ResolverError(e) => Error::Network {
            details: e.to_string(),
        },
        e => Error::Internal {
            details: format!("unexpected consignment validation error: {e}"),
        },
    }
}

/// Phase 1 of the validation of the given consignment, of an asset with the given schema:
/// everything that only depends on the consignment itself.
///
/// An [`Error::ConsignmentValidationFailed`] is returned if the consignment is invalid.
pub(crate) fn validate_deterministic<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
    asset_schema: AssetSchema,
    validation_config: &ValidationConfig,
) -> Result<PendingConsignment<TRANSFER>, Error> {
    consignment
        .clone()
        .validate_deterministic(&asset_schema.schema_rules(), validation_config)
        .map_err(validation_error)
}

/// The claims (mint locks) of a consignment left to confirm, see [`pending_claims`].
pub(crate) struct PendingClaims {
    /// Location of the bridge whose chain the claims live on
    pub(crate) bridge_location: BridgeLocation,
    /// The claims to confirm
    pub(crate) claims: Vec<Claim>,
}

/// The claims (mint locks) the given pending consignment, of an asset with the given
/// schema, needs confirmed, if any.
///
/// claims live on the chain the asset's bridge contract is deployed on and cannot be
/// resolved by the witness resolver: they need to be confirmed against that chain (see
/// [`resolve_claims`]) before the witnesses are resolved. An
/// [`Error::ConsignmentValidationFailed`] is returned if the schema doesn't support them.
pub(crate) fn pending_claims<const TRANSFER: bool>(
    pending: &mut PendingConsignment<TRANSFER>,
    asset_schema: AssetSchema,
) -> Result<Option<PendingClaims>, Error> {
    let claims: Vec<Claim> = pending.pending().unresolved_claims().cloned().collect();
    if claims.is_empty() {
        return Ok(None);
    }
    if !asset_schema.supports_mint() {
        return Err(Error::ConsignmentValidationFailed {
            details: format!("{asset_schema} schema does not support claims"),
        });
    }
    // the bridge location is an immutable genesis global naming the chain the mint locks live on
    let bridge_location = BridgedFungibleAsset::bridge_location(pending.consignment().genesis())
        .map_err(|e| Error::Internal {
            details: format!("unexpected bridge location error: {e}"),
        })?;
    Ok(Some(PendingClaims {
        bridge_location,
        claims,
    }))
}

/// Confirm the given claims of the given pending consignment with the given claim
/// resolver, returning the operation IDs of the ones that are not confirmed (yet).
///
/// A mint the resolver reports as expired (see `is_expired`), its lock not included before the
/// block it commits to, will never be confirmed: an [`Error::MintLockExpired`] is returned for
/// it.
pub(crate) fn resolve_claims<const TRANSFER: bool, R: ResolveClaim>(
    pending: &mut PendingConsignment<TRANSFER>,
    claim_resolver: &CheckedClaimResolver<R>,
    claims: &[Claim],
    is_expired: impl Fn(&OpId) -> bool,
) -> Result<Vec<OpId>, Error> {
    let mut unconfirmed = vec![];
    // go through all the claims, not stopping at the first unconfirmed one, so that an expired
    // mint after it is still reported: it will never be confirmed, while an unconfirmed one may
    // still be, so a wallet must refuse the transfer instead of waiting for it
    for claim in claims {
        let Claim::Lock { before_block, .. } = claim;
        match pending.resolve_claim(claim_resolver, claim) {
            Ok(()) => {}
            // tell a mint that will never be confirmed from one that may still be
            Err(ClaimResolverError::Unconfirmed(opid)) if is_expired(&opid) => {
                return Err(Error::MintLockExpired {
                    opid: opid.to_string(),
                    before_block: *before_block,
                });
            }
            Err(ClaimResolverError::Unconfirmed(opid)) => unconfirmed.push(opid),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(unconfirmed)
}

/// Phase 2 of the validation of the given pending consignment, whose claims must all
/// be confirmed by now: resolve the witnesses with the given resolver, then finalize.
///
/// An [`Error::ConsignmentValidationFailed`] is returned if the consignment is invalid, an
/// [`Error::Network`] if the resolver fails.
pub(crate) fn resolve_witnesses<const TRANSFER: bool>(
    mut pending: PendingConsignment<TRANSFER>,
    resolver: &impl ResolveWitness,
) -> Result<ValidConsignment<TRANSFER>, Error> {
    pending
        .pending()
        .resolve_all(resolver)
        .map_err(validation_error)?;
    Ok(pending.finalize())
}

/// Check the given `opouts` against the reject list of the asset with the given schema and
/// genesis, read with the given resolver: an [`Error::ConsignmentRejected`] is returned with the
/// ones it rejects (see [`rejected_opouts`]), if any. Nothing is checked if the asset has no
/// reject list.
///
/// The `ops_dag` of the validated operations must be provided if the schema supports a reject
/// list (see [`AssetSchema::supports_reject_list`]). An [`Error::NoRejectListResolver`] is
/// returned if the asset has a reject list but no resolver is provided.
pub(crate) fn check_reject_list(
    asset_schema: AssetSchema,
    genesis: &Genesis,
    ops_dag: Option<&OpsDag>,
    reject_list_resolver: Option<&dyn ResolveRejectList>,
    opouts: &HashSet<Opout>,
) -> Result<(), Error> {
    let Some(source) = genesis_reject_list_source(asset_schema, genesis)? else {
        return Ok(());
    };
    let reject_list = reject_list_resolver
        .ok_or(Error::NoRejectListResolver)?
        .get_reject_list(&source)?;
    let ops_dag = ops_dag.ok_or_else(|| Error::Internal {
        details: s!("no operations DAG to check against the reject list"),
    })?;
    let rejected = rejected_opouts(ops_dag, &reject_list, opouts)?;
    if rejected.is_empty() {
        return Ok(());
    }
    let mut opouts: Vec<String> = rejected.iter().map(|o| o.to_string()).collect();
    opouts.sort();
    Err(Error::ConsignmentRejected { opouts })
}

/// The EVM RPC client for the chain with the given `chain_id`, among the given `evm_rpc_urls`.
#[cfg(feature = "_validation")]
pub(crate) fn evm_client(
    evm_rpc_urls: &HashMap<u64, String>,
    chain_id: u64,
) -> Result<EvmClient, Error> {
    let evm_rpc_url = evm_rpc_urls
        .get(&chain_id)
        .ok_or(Error::NoEvmRpcUrl { chain_id })?;
    EvmClient::new(evm_rpc_url)
}

/// The EVM RPC client for the chain with the given `chain_id`, among the given `evm_rpc_urls`,
/// checked to be connected to it.
#[cfg(feature = "_validation")]
pub(crate) fn checked_evm_client(
    evm_rpc_urls: &HashMap<u64, String>,
    chain_id: u64,
) -> Result<EvmClient, Error> {
    let evm_client = evm_client(evm_rpc_urls, chain_id)?;
    let actual = evm_client.chain_id()?;
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

/// The opouts the reject list check of a consignment starts from: the inputs and the outputs of
/// its terminal operations, the ones nothing in the consignment builds on (for a transfer, the
/// last transition; for a burn, the burn itself, whose inputs are checked even if it has no
/// output).
///
/// The inputs are checked on their own, so that an allowed output can't shield the history of the
/// operation producing it: what the consignment proves is the operation as a whole. Unspent
/// outputs of other operations, e.g. the change of an earlier transfer, are revealed by the
/// consignment but not proven by it, so they are never checked.
pub(crate) fn reject_check_origins<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
) -> HashSet<Opout> {
    let terminal_ops = consignment.terminal_ops();
    let mut opouts = HashSet::new();
    for KnownTransition { opid, transition } in consignment
        .bundles
        .iter()
        .flat_map(|wb| wb.bundle.known_transitions.iter())
        .filter(|kt| terminal_ops.contains(&kt.opid))
    {
        opouts.extend(transition.inputs());
        for (ass_type, typed_assigns) in transition.assignments.iter() {
            for no in 0..typed_assigns.len_u16() {
                opouts.insert(Opout::new(*opid, *ass_type, no));
            }
        }
    }
    opouts
}

/// The opouts, among the given `check_opouts`, the given reject list rejects: the ones having a
/// rejected opout along their history (the operations DAG built by the validation), unless an
/// allowed opout shields it.
pub(crate) fn rejected_opouts(
    ops_dag: &OpsDag,
    reject_list: &RejectList,
    check_opouts: &HashSet<Opout>,
) -> Result<HashSet<Opout>, Error> {
    let mut to_reject = HashSet::new();
    // shared by the walks, so that a history common to several opouts is walked once
    let mut rejected = HashMap::new();

    // for each opout we are checking, traverse its ancestor chain
    for check_opout in check_opouts {
        // allow shields this path upward: do not traverse this branch further
        if reject_list.allows(check_opout) {
            continue;
        }
        // encountering a reject opout on an unshielded path: reject
        if reject_list.rejects(check_opout) {
            to_reject.insert(*check_opout);
            continue;
        }
        let op_node = ops_dag.op_node(&check_opout.op).ok_or(Error::Internal {
            details: s!("opout not found in DAG"),
        })?;

        // traverse from the operation creating it to its ancestors, checking the opouts each
        // operation spends
        if history_rejected(ops_dag, reject_list, op_node, &mut rejected) {
            to_reject.insert(*check_opout);
        }
    }
    Ok(to_reject)
}

/// Whether the history of the operation at `start` in the given `ops_dag` has an opout the given
/// reject list rejects along a path no opout it allows shields.
///
/// The answer depends on the operation only, not on the opout the walk started from, so the
/// answer for each operation walked is kept in `rejected`, where a walk sharing part of the
/// history stops: checking many opouts with a common history walks it once rather than once per
/// opout.
fn history_rejected(
    ops_dag: &OpsDag,
    reject_list: &RejectList,
    start: NodeIndex,
    rejected: &mut HashMap<NodeIndex, bool>,
) -> bool {
    // depth first, settling an operation once all the operations it spends from are settled (the
    // DAG has no cycles)
    let mut stack = vec![start];
    while let Some(&node) = stack.last() {
        if rejected.contains_key(&node) {
            stack.pop();
            continue;
        }
        let base = stack.len();
        let mut node_rejected = false;
        for (spent_opout, parent) in ops_dag.spent_opouts(node) {
            // allow shields this path upward: do not traverse this branch further
            if reject_list.allows(&spent_opout) {
                continue;
            }
            // encountering a reject opout, or a history having one, on an unshielded path: reject
            if reject_list.rejects(&spent_opout) || rejected.get(&parent) == Some(&true) {
                node_rejected = true;
                break;
            }
            if !rejected.contains_key(&parent) {
                stack.push(parent);
            }
        }
        if node_rejected {
            // the operations pushed aren't needed to settle this one anymore
            stack.truncate(base);
        } else if stack.len() > base {
            // settle the operations it spends from first, then come back to it
            continue;
        }
        rejected.insert(node, node_rejected);
        stack.pop();
    }
    rejected[&start]
}

#[cfg(all(test, feature = "electrum_validation"))]
pub(crate) mod tests {
    use super::*;

    const GENESIS_HEADER_HEX: &str = "0100000000000000000000000000000000000000000000000000000000000000000000003ba3edfd7a7b12b27ac72c3e67768f617fc81bc3888a51323a9fb8aa4b1e5e4a29ab5f49ffff001d1dac2b7c";

    /// Start a mock electrum server that accepts a single connection, refusing any further one,
    /// and answers the requests needed to check it can be reached, returning its URL.
    pub(crate) fn start_single_connection_electrum_mock() -> String {
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
        };
        let result = validate_consignment(
            "unused",
            &IndexerConfig::Electrum { url },
            &HashMap::new(),
            None,
            &options,
        );
        assert_matches!(result, Err(Error::InvalidIndexer { .. }));
    }

    // the history of an operation in the given DAG, checked against the reject list with a fresh
    // walk, as each check opout was before the walks shared their answers
    fn history_rejected_unshared(
        ops_dag: &OpsDag,
        reject_list: &RejectList,
        start: NodeIndex,
    ) -> bool {
        let mut stack = vec![start];
        let mut visited = HashSet::new();
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            for (spent_opout, parent) in ops_dag.spent_opouts(node) {
                if reject_list.allows(&spent_opout) {
                    continue;
                }
                if reject_list.rejects(&spent_opout) {
                    return true;
                }
                stack.push(parent);
            }
        }
        false
    }

    // walks sharing their answers decide as walks starting afresh, whatever their order, on random
    // DAGs with shared histories and random reject lists
    #[test]
    fn history_rejected_matches_unshared_walks() {
        // xorshift, deterministic so that a failure can be reproduced
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |bound: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % bound
        };
        let op = |i: u64| {
            let mut id = [0u8; 32];
            id[..8].copy_from_slice(&i.to_le_bytes());
            OpId::from(id)
        };
        let (mut rejected_count, mut not_rejected_count) = (0, 0);
        for _ in 0..1000 {
            let op_count = 1 + next(40);
            let mut ops_dag = OpsDag::new();
            let mut reject_list = RejectList::default();
            // each operation spends opouts of earlier ones, so the DAG has no cycles, often enough
            // for histories to be shared and to have several paths between two operations, some
            // operations spending more opouts of the same one
            let density = 1 + next(4);
            for i in 0..op_count {
                ops_dag.register_op(op(i));
                for j in 0..i {
                    for no in 0..3 {
                        if next(3 * i + 1) >= density {
                            continue;
                        }
                        let input = Opout::new(op(j), AssignmentType::with(4000), no);
                        match next(10) {
                            0 => {
                                reject_list.reject_opouts.insert(input);
                            }
                            1 => {
                                reject_list.allow_opouts.insert(input);
                            }
                            2 if next(4) == 0 => {
                                reject_list.reject_opouts.insert(input);
                                reject_list.allow_opouts.insert(input);
                            }
                            _ => {}
                        }
                        ops_dag.connect_input(input, op(i));
                    }
                }
            }

            let mut starts: Vec<NodeIndex> = (0..op_count)
                .map(|i| ops_dag.op_node(&op(i)).unwrap())
                .collect();
            for i in (1..starts.len()).rev() {
                starts.swap(i, next(i as u64 + 1) as usize);
            }
            let mut rejected = HashMap::new();
            for start in starts {
                let expected = history_rejected_unshared(&ops_dag, &reject_list, start);
                assert_eq!(
                    history_rejected(&ops_dag, &reject_list, start, &mut rejected),
                    expected
                );
                if expected {
                    rejected_count += 1;
                } else {
                    not_rejected_count += 1;
                }
            }
        }
        // both decisions are exercised
        assert!(rejected_count > 1000 && not_rejected_count > 1000);
    }

    #[test]
    fn reject_list_latest_decision() {
        let opout = Opout::new(OpId::from([1; 32]), AssignmentType::with(0), 0);
        let mut reject_list = RejectList::default();
        assert!(!reject_list.rejects(&opout) && !reject_list.allows(&opout));
        reject_list.insert(opout, false);
        assert!(reject_list.rejects(&opout) && !reject_list.allows(&opout));
        reject_list.insert(opout, true);
        assert!(!reject_list.rejects(&opout) && reject_list.allows(&opout));
        reject_list.insert(opout, false);
        assert!(reject_list.rejects(&opout) && !reject_list.allows(&opout));

        // the last line for an opout wins
        let (reject_list, invalid) = RejectList::parse(&format!("{opout}\n!{opout}\n"));
        assert_eq!(invalid, 0);
        assert!(!reject_list.rejects(&opout) && reject_list.allows(&opout));
        let reject_list = RejectList::from_decisions([(opout, true), (opout, false)]);
        assert!(reject_list.rejects(&opout) && !reject_list.allows(&opout));
    }
}
