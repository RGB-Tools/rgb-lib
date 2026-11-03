//! Wallet objects.
//!
//! This module defines the objects used by wallet methods.

use super::*;

// ────────────────────────────────────────────────────────────
// Wallet configuration & setup
// ────────────────────────────────────────────────────────────

/// Supported database types.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum DatabaseType {
    /// A SQLite database
    Sqlite,
}

/// Data that defines a [`Wallet`].
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct WalletData {
    /// Directory where the wallet directory is stored
    pub data_dir: String,
    /// Bitcoin network for the wallet
    pub bitcoin_network: BitcoinNetwork,
    /// Database type for the wallet
    pub database_type: DatabaseType,
    /// The max number of RGB allocations allowed per UTXO
    #[serde(deserialize_with = "from_str_or_number_mandatory")]
    pub max_allocations_per_utxo: u32,
    /// List of schemas the wallet should support (when issuing, sending and receiving). Empty list
    /// means the wallet should support all the schemas rgb-lib supports.
    pub supported_schemas: Vec<AssetSchema>,
}

/// Descriptors for an RGB wallet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct WalletDescriptors {
    /// Colored descriptor
    pub colored: String,
    /// Vanilla descriptor
    pub vanilla: String,
}

/// Data for operations that require the wallet to be online.
///
/// Methods not requiring an `Online` object don't need network access and can be performed
/// offline. Methods taking an optional `Online` will operate offline when it's missing and will
/// use local data only.
///
/// <div class="warning">This should not be manually constructed but should be obtained from the
/// [`Wallet::go_online`] method.</div>
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Online {
    /// Unique ID for this object
    pub id: u64,
}

#[cfg(feature = "_online")]
pub struct OnlineData {
    pub(crate) id: u64,
    pub(crate) indexer_config: IndexerConfig,
    pub(crate) indexer: Indexer,
    pub(crate) resolver: AnyResolver,
    // EVM chains by EIP-155 chain ID (see OnlineOptions::evm_chains)
    pub(crate) evm_chains: HashMap<u64, EvmChain>,
    // the RPC URLs of evm_chains, for what resolves claims and reject lists through them
    pub(crate) evm_rpc_urls: HashMap<u64, String>,
    pub(crate) hub_client: Option<MultisigHubClient>,
    pub(crate) user_role: Option<UserRole>,
    pub(crate) vanilla_sync_lookback: u32,
    pub(crate) failed_witness_receive_grace_secs: u32,
    // Ed25519 keys authenticating calls to proxies with proxy-level authentication enabled, by
    // proxy URL (see OnlineOptions::proxy_auth_keys)
    pub(crate) proxy_auth_keys: HashMap<String, ed25519_dalek::SigningKey>,
}

#[cfg(feature = "_online")]
impl OnlineData {
    /// Signature authenticating a call to the proxy at `proxy_url`, if a key has been configured
    /// for it. The proxy verifies the Ed25519 signature of the SHA-256 digest of `message` against
    /// its configured pubkeys.
    pub(crate) fn proxy_auth_signature(&self, proxy_url: &str, message: &str) -> Option<String> {
        self.proxy_auth_keys.get(proxy_url).map(|sk| {
            let digest = sha256_digest(message.as_bytes());
            hex::encode(sign_ed25519(sk, &digest))
        })
    }
}

/// Options for the [`Wallet::go_online`] and [`MultisigWallet::go_online`] methods.
#[cfg(feature = "_online")]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct OnlineOptions {
    /// Indexer the wallet should use
    pub indexer_config: IndexerConfig,
    /// Whether to skip the consistency check when going online.
    ///
    /// Setting this to false runs a check on UTXOs (BDK vs rgb-lib DB), assets (RGB vs rgb-lib DB)
    /// and media (DB vs actual files) to try and detect possible inconsistencies in the wallet.
    /// Setting this to true bypasses the check and allows operating an inconsistent wallet.
    ///
    /// <div class="warning">Warning: setting <tt>skip_consistency_check</tt> to true is dangerous,
    /// only do this if you know what you're doing!</div>
    pub skip_consistency_check: bool,
    /// Number of addresses before the last used (or last revealed if none) address to sync when
    /// doing an automatic FastSync for the vanilla keychain
    pub vanilla_sync_lookback: u32,
    /// Number of seconds, after a witness receive has failed, during which its script keeps being
    /// synced by FastSync for the colored keychain.
    ///
    /// The counterparty may broadcast its TX even if the receive has failed (e.g. if it was refused
    /// after the counterparty had already broadcast, or the counterparty ignored the refusal), in
    /// which case the BTC paid to the script is detected and becomes available as an ordinary UTXO.
    /// A longer grace period detects late payments for longer, at the cost of adding the scripts of
    /// recently failed witness receives to each FastSync. A payment arriving after the grace period
    /// is only detected by a FullSync or a FullScan. Setting this to 0 stops syncing the script as
    /// soon as the receive fails.
    pub failed_witness_receive_grace_secs: u32,
    /// Keys used to authenticate calls to proxies that require proxy-level authentication.
    ///
    /// A proxy can be started with a list of Ed25519 pubkeys, in which case its
    /// `recipientid.prepare` (reserving a recipient ID when receiving) and `ack.prepare`
    /// (registering a transfer when sending) calls must be signed with one of the corresponding
    /// private keys.
    ///
    /// Maps a transport endpoint (e.g. `rpc://host/json-rpc`) to the hex-encoded 32-byte Ed25519
    /// seed to sign calls to that proxy with. The keys are kept in memory only, they are neither
    /// persisted nor backed up. Going online again replaces any previously set keys.
    ///
    /// This is a standing credential that identifies the wallet to a proxy across every operation
    /// processed there, not a per-operation secret: it's supplied fresh each time the wallet goes
    /// online and can be rotated freely, so the wallet never retains it for longer than the caller
    /// chooses to keep supplying it, and it can be evicted once nothing pending still needs it.
    ///
    /// Because `refresh` processes pending transfers using the keys supplied when going online, the
    /// caller must keep supplying a correct key for all endpoints that any pending transfer relies
    /// on: a wrong or missing key for such an endpoint can make `refresh` fail to process it.
    pub proxy_auth_keys: HashMap<String, String>,
    /// EVM chains to be used for mint operations on bridged assets, by their EIP-155 chain ID.
    ///
    /// A bridged asset commits to the chain its bridge contract lives on: minting it, or
    /// receiving a mint of it, needs that chain to be configured and fails with
    /// [`Error::NoEvmRpcUrl`] otherwise. Going online checks each configured endpoint answers,
    /// is on the chain it is configured for and serves the latest finalized block and the contract
    /// state as of it (as locks are read from the records of the bridge contract once finalized),
    /// failing with an [`Error::InvalidEvmRpcUrl`] otherwise: nodes not keeping the state of that
    /// block (e.g. some pruned ones) are not supported. It also checks
    /// the mint lock blocks of each chain, failing with an [`Error::InvalidEvmChain`] if they
    /// aren't consistent.
    ///
    /// Only used when the wallet supports the [`AssetSchema::Bfa`] schema.
    pub evm_chains: HashMap<u64, EvmChain>,
}

/// An EVM chain mint operations use, see [`OnlineOptions::evm_chains`].
#[cfg(feature = "_online")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct EvmChain {
    /// URL of the EVM JSON-RPC endpoint connected to the chain.
    ///
    /// The wallet trusts this endpoint to report the state of the chain faithfully, as it
    /// confirms mints (the locks the bridge contract records) and reads reject lists through it:
    /// an endpoint reporting a lock as expired when it isn't makes the wallet refuse a mint that
    /// completes anyway (the minter checking the lock through its own endpoint), so the minted
    /// assets are lost for it, while one reporting a lock that doesn't exist makes it accept a
    /// mint that's invalid for everyone else (e.g. its later recipients). The same applies to the
    /// reject lists read through it. Use an endpoint you control or trust.
    pub rpc_url: String,
    /// Minimum number of blocks, after the latest block of the chain, the block a mint commits to
    /// (the one its lock has to be included before) has to be, when minting (see
    /// [`Wallet::mint_begin`]).
    ///
    /// The lock is made after minting begins and the recipient has checked the mint: one
    /// included late expires the mint, for everyone, getting the locked tokens back depending on
    /// the bridge contract. This leaves the lock time to be made and included. It only protects
    /// the minter, so it's not checked on received mints. The time this corresponds to depends
    /// on the block time of the chain. A value below 2 has the effect of 2, the lock needing a
    /// block after the latest one to be included in. It must be at most
    /// [`max_mint_lock_blocks`](EvmChain::max_mint_lock_blocks), going online failing with an
    /// [`Error::InvalidEvmChain`] otherwise.
    pub min_mint_lock_blocks: u64,
    /// Maximum number of blocks, after the latest block of the chain, the block a mint commits to
    /// (the one its lock has to be included before) can be.
    ///
    /// When refreshing, a received mint is checked before its lock is made (see
    /// [`get_received_mint`](crate::wallet::RgbWalletOpsOffline::get_received_mint)) and from
    /// then on its transfer is waited for until the lock is finalized or found expired, keeping
    /// the receiving UTXO unspendable meanwhile. A mint committing to a block further than this,
    /// as of when it is checked, is refused: a checked mint can then keep the UTXO unspendable
    /// for up to about this many blocks, plus the time the chain takes to finalize them. Minting
    /// refuses a further block too (see [`Wallet::mint_begin`]).
    ///
    /// The time this corresponds to depends on the block time of the chain. A value below 2
    /// refuses every mint, as the lock needs a block after the latest one to be included in.
    /// The cosigners of a multisig wallet should all configure the same value: a cosigner
    /// configuring a lower one refuses on its own a mint the others may accept.
    pub max_mint_lock_blocks: u64,
}

// ────────────────────────────────────────────────────────────
// Bitcoin primitives
// ────────────────────────────────────────────────────────────

/// Bitcoin transaction outpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Outpoint {
    /// ID of the transaction
    pub txid: String,
    /// Output index
    pub vout: u32,
}

impl fmt::Display for Outpoint {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}:{}", self.txid, self.vout)
    }
}

impl From<OutPoint> for Outpoint {
    fn from(x: OutPoint) -> Outpoint {
        Outpoint {
            txid: x.txid.to_string(),
            vout: x.vout,
        }
    }
}

impl From<DbReservedTxo> for Outpoint {
    fn from(x: DbReservedTxo) -> Outpoint {
        Outpoint {
            txid: x.txid,
            vout: x.vout,
        }
    }
}

impl From<DbTxo> for Outpoint {
    fn from(x: DbTxo) -> Outpoint {
        Outpoint {
            txid: x.txid,
            vout: x.vout,
        }
    }
}

impl From<Outpoint> for OutPoint {
    fn from(x: Outpoint) -> OutPoint {
        OutPoint::from_str(&x.to_string()).expect("outpoint should be parsable")
    }
}

/// A balance.
///
/// This structure is used both for RGB assets and BTC balances (in sats). When used for a BTC
/// balance it can be used both for the vanilla wallet and the colored wallet.
///
/// An RGB balance exceeding `u64::MAX` (e.g. a future balance including pending incoming transfers
/// that double-spend each other) is capped to `u64::MAX`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Balance {
    /// Settled balance, based on operations that have reached the final status
    pub settled: u64,
    /// Future balance, including settled operations plus ones are not yet finalized
    pub future: u64,
    /// Spendable balance, only including balance that can actually be spent. It's a subset of the
    /// settled balance. For the RGB balance this excludes the allocations on UTXOs related to
    /// pending operations
    pub spendable: u64,
}

/// The bitcoin balances (in sats) for the vanilla and colored wallets.
///
/// The settled balances include the confirmed balance.
/// The future balances also include the immature balance and the untrusted and trusted pending
/// balances.
/// The spendable balances include the settled balance and also the untrusted and trusted pending
/// balances.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct BtcBalance {
    /// Funds that will never hold RGB assets
    pub vanilla: Balance,
    /// Funds that may hold RGB assets
    pub colored: Balance,
}

/// Block height and timestamp of a block.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct BlockTime {
    /// Confirmation block height
    pub height: u32,
    /// Confirmation block timestamp
    pub timestamp: u64,
}

// ────────────────────────────────────────────────────────────
// Assets, tokens & media
// ────────────────────────────────────────────────────────────

/// An asset filter.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum AssetFilter {
    /// Match any or no asset
    AnyOrNone,
    /// Match no asset
    None,
    /// Match the asset with the given ID
    Id(String),
}

/// An asset media file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Media {
    /// Path of the media file
    pub file_path: String,
    /// Digest of the media file
    pub digest: String,
    /// Mime type of the media file
    pub mime: String,
}

impl Media {
    pub(crate) fn get_digest(&self) -> String {
        PathBuf::from(&self.file_path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    pub(crate) fn from_attachment<P: AsRef<Path>>(attachment: &Attachment, media_dir: P) -> Self {
        let digest = hex::encode(attachment.digest);
        let file_path = media_dir
            .as_ref()
            .join(&digest)
            .to_string_lossy()
            .to_string();
        Self {
            digest,
            mime: attachment.ty.to_string(),
            file_path,
        }
    }

    pub(crate) fn from_db_media<P: AsRef<Path>>(db_media: &DbMedia, media_dir: P) -> Self {
        let digest = db_media.digest.clone();
        let file_path = media_dir
            .as_ref()
            .join(&digest)
            .to_string_lossy()
            .to_string();
        Self {
            digest,
            mime: db_media.mime.clone(),
            file_path,
        }
    }
}

/// A media embedded in the contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct EmbeddedMedia {
    /// Mime of the embedded media
    pub mime: String,
    /// Bytes of the embedded media (max 16MB)
    pub data: Vec<u8>,
}

impl From<RgbEmbeddedMedia> for EmbeddedMedia {
    fn from(value: RgbEmbeddedMedia) -> Self {
        Self {
            mime: value.ty.to_string(),
            data: value.data.to_unconfined(),
        }
    }
}

/// A proof of reserves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct ProofOfReserves {
    /// Proof of reserves UTXO
    pub utxo: Outpoint,
    /// Proof bytes
    pub proof: Vec<u8>,
}

impl From<RgbProofOfReserves> for ProofOfReserves {
    fn from(value: RgbProofOfReserves) -> Self {
        Self {
            utxo: value.utxo.into(),
            proof: value.proof.to_unconfined(),
        }
    }
}

/// An RGB21 token.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Token {
    /// Index of the token
    pub index: u32,
    /// Ticker of the token
    pub ticker: Option<String>,
    /// Name of the token
    pub name: Option<String>,
    /// Details of the token
    pub details: Option<String>,
    /// Embedded media of the token
    pub embedded_media: Option<EmbeddedMedia>,
    /// Token primary media attachment
    pub media: Option<Media>,
    /// Token extra media attachments
    pub attachments: HashMap<u8, Media>,
    /// Proof of reserves of the token
    pub reserves: Option<ProofOfReserves>,
}

impl Token {
    pub(crate) fn from_token_data<P: AsRef<Path>>(token_data: &TokenData, media_dir: P) -> Self {
        Self {
            index: token_data.index.into(),
            ticker: token_data.ticker.clone().map(Into::into),
            name: token_data.name.clone().map(Into::into),
            details: token_data.details.clone().map(|d| d.to_string()),
            embedded_media: token_data.preview.clone().map(Into::into),
            media: token_data
                .media
                .clone()
                .map(|a| Media::from_attachment(&a, &media_dir)),
            attachments: token_data
                .attachments
                .to_unconfined()
                .into_iter()
                .map(|(i, a)| (i, Media::from_attachment(&a, &media_dir)))
                .collect(),
            reserves: token_data.reserves.clone().map(Into::into),
        }
    }
}

/// Light version of an RGB21 [`Token`], with embedded_media and reserves as booleans.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct TokenLight {
    /// Index of the token
    pub index: u32,
    /// Ticker of the token
    pub ticker: Option<String>,
    /// Name of the token
    pub name: Option<String>,
    /// Details of the token
    pub details: Option<String>,
    /// Whether the token has an embedded media
    pub embedded_media: bool,
    /// Token primary media attachment
    pub media: Option<Media>,
    /// Token extra media attachments
    pub attachments: HashMap<u8, Media>,
    /// Whether the token has proof of reserves
    pub reserves: bool,
}

impl From<Token> for TokenLight {
    fn from(value: Token) -> Self {
        Self {
            index: value.index,
            ticker: value.ticker,
            name: value.name,
            details: value.details,
            embedded_media: value.embedded_media.is_some(),
            media: value.media,
            attachments: value.attachments,
            reserves: value.reserves.is_some(),
        }
    }
}

/// Metadata of an RGB asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Metadata {
    /// Asset schema type
    pub asset_schema: AssetSchema,
    /// Initial issued supply
    pub initial_supply: u64,
    /// Max issued supply
    pub max_supply: u64,
    /// Known circulating supply
    pub known_circulating_supply: u64,
    /// Timestamp of asset genesis
    pub timestamp: i64,
    /// Asset name
    pub name: String,
    /// Asset precision
    pub precision: u8,
    /// Asset ticker
    pub ticker: Option<String>,
    /// Asset details
    pub details: Option<String>,
    /// Asset unique token
    pub token: Option<Token>,
    /// Reject list URL
    pub reject_list_url: Option<String>,
    /// ID of the contract the asset is linked from, if any
    pub linked_from_contract: Option<String>,
    /// ID of the contract the asset has been linked to, if linked by this wallet: a link assigns
    /// nothing, so no consignment of a later operation carries it
    pub linked_to_contract: Option<String>,
}

/// A Non-Inflatable Asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct AssetNIA {
    /// ID of the asset
    pub asset_id: String,
    /// Ticker of the asset
    pub ticker: String,
    /// Name of the asset
    pub name: String,
    /// Details of the asset
    pub details: Option<String>,
    /// Precision, also known as divisibility, of the asset
    pub precision: u8,
    /// Total issued amount
    pub issued_supply: u64,
    /// Timestamp of asset genesis
    pub timestamp: i64,
    /// Timestamp of asset import
    pub added_at: i64,
    /// Current balance of the asset
    pub balance: Balance,
    /// Asset media attachment
    pub media: Option<Media>,
}

impl AssetNIA {
    pub(crate) fn get_asset_details(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        transfers: Option<Vec<DbTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
        medias: Option<Vec<DbMedia>>,
    ) -> Result<AssetNIA, Error> {
        let media = {
            let medias = if let Some(m) = medias {
                m
            } else {
                txn.iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = txn.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let initial_supply = asset.initial_supply.parse::<u64>().unwrap();
        Ok(AssetNIA {
            asset_id: asset.id.clone(),
            ticker: asset.ticker.clone().unwrap(),
            name: asset.name.clone(),
            details: asset.details.clone(),
            precision: asset.precision,
            issued_supply: initial_supply,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            media,
        })
    }
}

/// A Unique Digital Asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct AssetUDA {
    /// ID of the asset
    pub asset_id: String,
    /// Ticker of the asset
    pub ticker: String,
    /// Name of the asset
    pub name: String,
    /// Details of the asset
    pub details: Option<String>,
    /// Precision, also known as divisibility, of the asset
    pub precision: u8,
    /// Timestamp of asset genesis
    pub timestamp: i64,
    /// Timestamp of asset import
    pub added_at: i64,
    /// Current balance of the asset
    pub balance: Balance,
    /// Asset media attachment
    pub media: Option<Media>,
    /// Asset unique token
    pub token: Option<TokenLight>,
}

impl AssetUDA {
    pub(crate) fn get_asset_details(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        token: Option<TokenLight>,
        transfers: Option<Vec<DbTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
        medias: Option<Vec<DbMedia>>,
    ) -> Result<AssetUDA, Error> {
        let media = {
            let medias = if let Some(m) = medias {
                m
            } else {
                txn.iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = txn.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        Ok(AssetUDA {
            asset_id: asset.id.clone(),
            details: asset.details.clone(),
            ticker: asset.ticker.clone().unwrap(),
            name: asset.name.clone(),
            precision: asset.precision,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            media,
            token,
        })
    }
}

/// A Collectible Fungible Asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct AssetCFA {
    /// ID of the asset
    pub asset_id: String,
    /// Name of the asset
    pub name: String,
    /// Details of the asset
    pub details: Option<String>,
    /// Precision, also known as divisibility, of the asset
    pub precision: u8,
    /// Total issued amount
    pub issued_supply: u64,
    /// Timestamp of asset genesis
    pub timestamp: i64,
    /// Timestamp of asset import
    pub added_at: i64,
    /// Current balance of the asset
    pub balance: Balance,
    /// Asset media attachment
    pub media: Option<Media>,
}

impl AssetCFA {
    pub(crate) fn get_asset_details(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        transfers: Option<Vec<DbTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
        medias: Option<Vec<DbMedia>>,
    ) -> Result<AssetCFA, Error> {
        let media = {
            let medias = if let Some(m) = medias {
                m
            } else {
                txn.iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = txn.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let initial_supply = asset.initial_supply.parse::<u64>().unwrap();
        Ok(AssetCFA {
            asset_id: asset.id.clone(),
            name: asset.name.clone(),
            details: asset.details.clone(),
            precision: asset.precision,
            issued_supply: initial_supply,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            media,
        })
    }
}

/// An Inflatable Fungible Asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct AssetIFA {
    /// ID of the asset
    pub asset_id: String,
    /// Ticker of the asset
    pub ticker: String,
    /// Name of the asset
    pub name: String,
    /// Details of the asset
    pub details: Option<String>,
    /// Precision, also known as divisibility, of the asset
    pub precision: u8,
    /// Initial issued supply
    pub initial_supply: u64,
    /// Max issued supply
    pub max_supply: u64,
    /// Known circulating supply
    pub known_circulating_supply: u64,
    /// Timestamp of asset genesis
    pub timestamp: i64,
    /// Timestamp of asset import
    pub added_at: i64,
    /// Current balance of the asset
    pub balance: Balance,
    /// Asset media attachment
    pub media: Option<Media>,
    /// Reject list URL
    pub reject_list_url: Option<String>,
}

impl AssetIFA {
    pub(crate) fn get_asset_details(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        transfers: Option<Vec<DbTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
        medias: Option<Vec<DbMedia>>,
    ) -> Result<AssetIFA, Error> {
        let media = {
            let medias = if let Some(m) = medias {
                m
            } else {
                txn.iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = txn.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let initial_supply = asset.initial_supply.parse::<u64>().unwrap();
        let max_supply = asset.max_supply.as_ref().unwrap().parse::<u64>().unwrap();
        let known_circulating_supply = asset
            .known_circulating_supply
            .as_ref()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        Ok(AssetIFA {
            asset_id: asset.id.clone(),
            ticker: asset.ticker.clone().unwrap(),
            name: asset.name.clone(),
            details: asset.details.clone(),
            precision: asset.precision,
            initial_supply,
            max_supply,
            known_circulating_supply,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            media,
            reject_list_url: asset.reject_list_url.clone(),
        })
    }
}

/// A Bridged Fungible Asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct AssetBFA {
    /// ID of the asset
    pub asset_id: String,
    /// Ticker of the asset
    pub ticker: String,
    /// Name of the asset
    pub name: String,
    /// Details of the asset
    pub details: Option<String>,
    /// Precision, also known as divisibility, of the asset
    pub precision: u8,
    /// Known circulating supply: the amount locked on the bridge contract and not released, as
    /// the contract accounted it when the wallet last read it (ending a mint or receiving the
    /// asset), 0 if it never did. It includes the locks of mints not completed (yet) and the ones
    /// in blocks that are not final, so it's an upper bound of the supply
    pub known_circulating_supply: u64,
    /// Timestamp of asset genesis
    pub timestamp: i64,
    /// Timestamp of asset import
    pub added_at: i64,
    /// Current balance of the asset
    pub balance: Balance,
    /// Asset media attachment
    pub media: Option<Media>,
    /// Reject list URL
    pub reject_list_url: Option<String>,
}

impl AssetBFA {
    pub(crate) fn get_asset_details(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        transfers: Option<Vec<DbTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
        medias: Option<Vec<DbMedia>>,
    ) -> Result<AssetBFA, Error> {
        let media = {
            let medias = if let Some(m) = medias {
                m
            } else {
                txn.iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = txn.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let known_circulating_supply = asset
            .known_circulating_supply
            .as_ref()
            .map_or(0, |s| s.parse::<u64>().unwrap());
        Ok(AssetBFA {
            asset_id: asset.id.clone(),
            ticker: asset.ticker.clone().unwrap(),
            name: asset.name.clone(),
            details: asset.details.clone(),
            precision: asset.precision,
            known_circulating_supply,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            media,
            reject_list_url: asset.reject_list_url.clone(),
        })
    }
}

/// List of RGB assets, grouped by asset schema.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Assets {
    /// List of NIA assets
    pub nia: Option<Vec<AssetNIA>>,
    /// List of UDA assets
    pub uda: Option<Vec<AssetUDA>>,
    /// List of CFA assets
    pub cfa: Option<Vec<AssetCFA>>,
    /// List of IFA assets
    pub ifa: Option<Vec<AssetIFA>>,
    /// List of BFA assets
    pub bfa: Option<Vec<AssetBFA>>,
}

pub(crate) trait IssuedAssetDetails: Sized {
    fn from_issuance(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        issue_data: &IssueData,
    ) -> Result<Self, Error>;
}

impl IssuedAssetDetails for AssetNIA {
    fn from_issuance(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        _issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(txn, wallet, asset, None, None, None, None, None, None)
    }
}

impl IssuedAssetDetails for AssetUDA {
    fn from_issuance(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(
            txn,
            wallet,
            asset,
            issue_data.asset_data.token.clone().map(|t| t.into()),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }
}

impl IssuedAssetDetails for AssetCFA {
    fn from_issuance(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        _issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(txn, wallet, asset, None, None, None, None, None, None)
    }
}

impl IssuedAssetDetails for AssetIFA {
    fn from_issuance(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        _issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(txn, wallet, asset, None, None, None, None, None, None)
    }
}

impl IssuedAssetDetails for AssetBFA {
    fn from_issuance(
        txn: &DbTxn,
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        _issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(txn, wallet, asset, None, None, None, None, None, None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LocalAssetData {
    pub(crate) asset_id: String,
    pub(crate) name: String,
    pub(crate) asset_schema: AssetSchema,
    pub(crate) precision: u8,
    pub(crate) ticker: Option<String>,
    pub(crate) details: Option<String>,
    pub(crate) media: Option<Media>,
    pub(crate) initial_supply: u64,
    pub(crate) max_supply: Option<u64>,
    pub(crate) known_circulating_supply: Option<u64>,
    pub(crate) reject_list_url: Option<String>,
    pub(crate) linked_from_contract: Option<String>,
    pub(crate) token: Option<Token>,
    pub(crate) timestamp: i64,
    pub(crate) added_at: i64,
}

#[derive(Debug, Clone)]
pub struct IssueData {
    pub(crate) asset_data: LocalAssetData,
    pub(crate) valid_contract: ValidContract,
    #[cfg(feature = "_online")]
    pub(crate) contract_path: PathBuf,
    pub(crate) issue_utxos: HashMap<i32, Vec<Assignment>>,
}

// ────────────────────────────────────────────────────────────
// RGB assignments & transitions
// ────────────────────────────────────────────────────────────

/// Collection of different RGB assignments.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct AssignmentsCollection {
    /// Fungible assignments
    pub fungible: u64,
    /// Non-fungible assignments
    pub non_fungible: bool,
    /// Inflation right assignments
    pub inflation: u64,
    /// Link right assignment
    pub link: bool,
    /// Mint right assignments
    pub mint: u64,
}

#[cfg(feature = "_online")]
impl AssignmentsCollection {
    fn add_fungible(&mut self, amt: u64) {
        self.fungible = self
            .fungible
            .checked_add(amt)
            .expect("total fungible amount cannot exceed u64::MAX");
    }

    fn add_non_fungible(&mut self) {
        self.non_fungible = true;
    }

    fn add_inflation(&mut self, amt: u64) {
        self.inflation = self
            .inflation
            .checked_add(amt)
            .expect("total inflation amount cannot exceed u64::MAX");
    }

    fn add_mint(&mut self) {
        self.mint = self
            .mint
            .checked_add(1)
            .expect("total mint rights cannot exceed u64::MAX");
    }

    pub(crate) fn add_opout_state(&mut self, opout: &Opout, state: &AllocatedState) {
        match state {
            AllocatedState::Amount(amt) if opout.ty == OS_ASSET => {
                self.add_fungible(amt.as_u64());
            }
            AllocatedState::Amount(amt) if opout.ty == OS_INFLATION => {
                self.add_inflation(amt.as_u64());
            }
            AllocatedState::Data(_) => {
                self.add_non_fungible();
            }
            AllocatedState::Void if opout.ty == OS_LINK => {
                self.link = true;
            }
            AllocatedState::Void if opout.ty == OS_MINT => {
                self.add_mint();
            }
            _ => {}
        }
    }

    pub(crate) fn opout_contributes(
        &self,
        opout: &Opout,
        state: &AllocatedState,
        needed: &Self,
    ) -> bool {
        match (state, opout.ty) {
            (AllocatedState::Amount(_), OS_ASSET) => {
                needed.fungible.saturating_sub(self.fungible) > 0
            }
            (AllocatedState::Amount(_), OS_INFLATION) => {
                needed.inflation.saturating_sub(self.inflation) > 0
            }
            (AllocatedState::Data(_), _) => needed.non_fungible && !self.non_fungible,
            (AllocatedState::Void, OS_LINK) => needed.link && !self.link,
            (AllocatedState::Void, OS_MINT) => needed.mint.saturating_sub(self.mint) > 0,
            _ => false,
        }
    }

    pub(crate) fn change(&self, needed: &Self) -> Self {
        Self {
            fungible: self
                .fungible
                .checked_sub(needed.fungible)
                .expect("selected inputs must cover outputs"),
            non_fungible: false,
            inflation: self
                .inflation
                .checked_sub(needed.inflation)
                .expect("selected inputs must cover outputs"),
            link: false,
            mint: self
                .mint
                .checked_sub(needed.mint)
                .expect("selected inputs must cover outputs"),
        }
    }

    pub(crate) fn enough(&self, needed: &Self) -> bool {
        if self.fungible < needed.fungible {
            return false;
        }
        if self.non_fungible != needed.non_fungible {
            return false;
        }
        if self.inflation < needed.inflation {
            return false;
        }
        if needed.link && !self.link {
            return false;
        }
        if self.mint < needed.mint {
            return false;
        }
        true
    }
}

// ────────────────────────────────────────────────────────────
// Invoices, recipients & transport
// ────────────────────────────────────────────────────────────

/// The type of an RGB recipient
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum RecipientType {
    /// Receive via blinded UTXO
    Blind,
    /// Receive via witness TX
    Witness,
}

impl fmt::Display for RecipientType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// RGB recipient information used to be paid
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RecipientInfo {
    /// Recipient ID
    pub recipient_id: String,
    /// Recipient type
    pub recipient_type: RecipientType,
    /// Recipient network
    pub network: BitcoinNetwork,
}

impl RecipientInfo {
    /// Builds a new [`RecipientInfo`] from the provided string, checking that it is valid.
    pub fn new(recipient_id: String) -> Result<Self, Error> {
        let xchainnet_beneficiary = XChainNet::<Beneficiary>::from_str(&recipient_id)
            .map_err(|_| Error::InvalidRecipientID)?;
        let recipient_type = match xchainnet_beneficiary.into_inner() {
            Beneficiary::WitnessVout(_, _) => RecipientType::Witness,
            Beneficiary::BlindedSeal(_) => RecipientType::Blind,
        };
        Ok(Self {
            recipient_id,
            recipient_type,
            network: xchainnet_beneficiary.chain_network().try_into()?,
        })
    }
}

/// A recipient of an RGB transfer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Recipient {
    /// Recipient ID
    pub recipient_id: String,
    /// Witness data (to be provided only with a witness recipient)
    pub witness_data: Option<WitnessData>,
    /// RGB assignment
    pub assignment: Assignment,
    /// Transport endpoints
    pub transport_endpoints: Vec<String>,
    /// Recipient's Ed25519 public key (hex-encoded), taken from the invoice `pk` field.
    ///
    /// Required when `transport_endpoints` is not empty: a proxy transfer needs it to encrypt data
    /// for, and verify the ACK from, the recipient. Ignored for the out-of-band exchange (empty
    /// `transport_endpoints`), where no proxy is involved and the consignment and ACK are exchanged
    /// by other means.
    pub public_key: Option<String>,
    /// The recipient's consignment/media encryption preference, taken from the invoice `enc` field.
    /// This is not a choice of the sender, who must honor it.
    pub encryption: Encryption,
    /// The invoice expiration (UNIX timestamp, seconds), taken from the invoice.
    ///
    /// Required when `transport_endpoints` is not empty: the transfer cannot be created after it
    /// and the operation expiration needs to leave the time to detect the ACK and broadcast the TX
    /// before it. Ignored for the out-of-band exchange (empty `transport_endpoints`).
    pub expiration_timestamp: Option<u64>,
}

/// The information needed to receive RGB assets in witness mode.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct WitnessData {
    /// The Bitcoin amount (in sats) to send to the recipient
    #[serde(deserialize_with = "from_str_or_number_mandatory")]
    pub amount_sat: u64,
    /// An optional blinding
    #[serde(deserialize_with = "from_str_or_number_optional")]
    pub blinding: Option<u64>,
}

/// A bitcoin address.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Address {
    /// The bitcoin address string
    address_string: String,
    /// The bitcoin network of the address
    bitcoin_network: BitcoinNetwork,
}

impl Address {
    /// Parse the provided `address_string`.
    /// Throws an error if the provided string is not a valid bitcoin address for the given
    /// network.
    pub fn new(address_string: String, bitcoin_network: BitcoinNetwork) -> Result<Self, Error> {
        parse_address_str(&address_string, bitcoin_network)?;
        Ok(Address {
            address_string,
            bitcoin_network,
        })
    }
}

/// An RGB invoice.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Invoice {
    /// The RGB invoice string
    invoice_string: String,
    /// The data of the RGB invoice
    pub(crate) invoice_data: InvoiceData,
}

impl Invoice {
    /// Parse the provided `invoice_string`.
    /// Throws an error if the provided string is not a valid RGB invoice.
    pub fn new(invoice_string: String) -> Result<Self, Error> {
        let decoded = RgbInvoice::from_str(&invoice_string).map_err(|e| Error::InvalidInvoice {
            details: e.to_string(),
        })?;
        let asset_id = decoded.contract.map(|cid| cid.to_string());
        let asset_schema = if let Some(schema_id) = decoded.schema {
            Some(AssetSchema::try_from(schema_id)?)
        } else {
            None
        };
        let assignment_name = decoded.assignment_name.map(|a| a.to_string());
        let assignment = match asset_schema {
            None => match (decoded.assignment_state, assignment_name.as_deref()) {
                (Some(InvoiceState::Amount(v)), Some(RGB_STATE_ASSET_OWNER)) => {
                    Assignment::Fungible(v.value())
                }
                (Some(InvoiceState::Amount(v)), Some(RGB_STATE_INFLATION_ALLOWANCE)) => {
                    Assignment::InflationRight(v.value())
                }
                (Some(InvoiceState::Amount(_)), _) => Assignment::Any,
                (Some(InvoiceState::Data(_)), Some(RGB_STATE_ASSET_OWNER) | None) => {
                    Assignment::NonFungible
                }
                (None, None) => Assignment::Any,
                (_, _) => {
                    return Err(Error::InvalidInvoice {
                        details: s!("unsupported assignment"),
                    });
                }
            },
            Some(AssetSchema::Nia) | Some(AssetSchema::Cfa) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Amount(v)), Some(RGB_STATE_ASSET_OWNER) | None) => {
                        Assignment::Fungible(v.value())
                    }
                    (None, Some(RGB_STATE_ASSET_OWNER) | None) => Assignment::Fungible(0),
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
            Some(AssetSchema::Uda) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Data(_)) | None, Some(RGB_STATE_ASSET_OWNER) | None) => {
                        Assignment::NonFungible
                    }
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
            Some(AssetSchema::Ifa) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Amount(v)), Some(RGB_STATE_ASSET_OWNER)) => {
                        Assignment::Fungible(v.value())
                    }
                    (None, Some(RGB_STATE_ASSET_OWNER)) => Assignment::Fungible(0),
                    (Some(InvoiceState::Amount(v)), Some(RGB_STATE_INFLATION_ALLOWANCE)) => {
                        Assignment::InflationRight(v.value())
                    }
                    (None, Some(RGB_STATE_INFLATION_ALLOWANCE)) => Assignment::InflationRight(0),
                    (Some(InvoiceState::Void) | None, Some(RGB_STATE_LINK_RIGHT)) => {
                        Assignment::LinkRight
                    }
                    (Some(InvoiceState::Amount(_)), None) => Assignment::Any,
                    (None, None) => Assignment::Any,
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
            Some(AssetSchema::Bfa) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Amount(v)), Some(RGB_STATE_ASSET_OWNER)) => {
                        Assignment::Fungible(v.value())
                    }
                    (None, Some(RGB_STATE_ASSET_OWNER)) => Assignment::Fungible(0),
                    (Some(InvoiceState::Void) | None, Some(RGB_STATE_LINK_RIGHT)) => {
                        Assignment::LinkRight
                    }
                    (Some(InvoiceState::Void) | None, Some(RGB_STATE_MINT_RIGHT)) => {
                        Assignment::MintRight
                    }
                    (Some(InvoiceState::Amount(_)), None) => Assignment::Any,
                    (None, None) => Assignment::Any,
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
        };
        let recipient_id = decoded.beneficiary.to_string();
        let transport_endpoints: Vec<String> =
            decoded.transports.iter().map(|t| t.to_string()).collect();
        // rgb-ops parses the invoice's public key into a 64-char hex string
        let public_key = decoded.public_key;
        let encryption = Encryption::from(decoded.encryption);

        let layer_1 = decoded.beneficiary.layer1();
        let network = match layer_1 {
            Layer1::Bitcoin => decoded.beneficiary.chain_network().try_into().unwrap(),
            _ => {
                return Err(Error::UnsupportedLayer1 {
                    layer_1: layer_1.to_string(),
                });
            }
        };

        Ok(Invoice {
            invoice_string,
            invoice_data: InvoiceData {
                recipient_id,
                asset_schema,
                asset_id,
                assignment,
                assignment_name,
                expiration_timestamp: decoded.expiry.map(|t| t as u64),
                transport_endpoints,
                network,
                public_key,
                encryption,
                unknown_query_params: decoded.unknown_query.into_iter().collect(),
            },
        })
    }

    /// Return the data associated with this [`Invoice`].
    pub fn invoice_data(&self) -> InvoiceData {
        self.invoice_data.clone()
    }

    /// Return the string associated with this [`Invoice`].
    pub fn invoice_string(&self) -> String {
        self.invoice_string.clone()
    }
}

/// The data of an RGB invoice.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct InvoiceData {
    /// ID of the receive operation (blinded UTXO or Bitcoin script)
    pub recipient_id: String,
    /// RGB schema
    pub asset_schema: Option<AssetSchema>,
    /// RGB asset ID
    pub asset_id: Option<String>,
    /// RGB assignment
    pub assignment: Assignment,
    /// RGB assignment name
    pub assignment_name: Option<String>,
    /// Bitcoin network
    pub network: BitcoinNetwork,
    /// Invoice expiration
    pub expiration_timestamp: Option<u64>,
    /// Transport endpoints
    pub transport_endpoints: Vec<String>,
    /// Recipient's Ed25519 public key (hex-encoded), from the invoice `pk` field (if present)
    pub public_key: Option<String>,
    /// Consignment/media encryption preference, from the invoice `enc` field
    pub encryption: Encryption,
    /// Unknown query parameters carried by the invoice
    pub unknown_query_params: HashMap<String, String>,
}

/// An RGB transport endpoint.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct TransportEndpoint {
    /// Endpoint address
    pub endpoint: String,
    /// Endpoint transport type
    pub transport_type: TransportType,
}

impl TransportEndpoint {
    /// Builds a new [`TransportEndpoint::endpoint`] from the provided string, checking that it is
    /// valid.
    pub fn new(transport_endpoint: String) -> Result<Self, Error> {
        let rgb_transport = RgbTransport::from_str(&transport_endpoint)?;
        TransportEndpoint::try_from(rgb_transport)
    }

    /// Return the transport type of this transport endpoint.
    pub fn transport_type(&self) -> TransportType {
        self.transport_type
    }
}

impl TryFrom<RgbTransport> for TransportEndpoint {
    type Error = Error;

    fn try_from(x: RgbTransport) -> Result<Self, Self::Error> {
        match x {
            RgbTransport::JsonRpc { tls, host } => Ok(TransportEndpoint {
                endpoint: format!("http{}://{host}", if tls { "s" } else { "" }),
                transport_type: TransportType::JsonRpc,
            }),
            _ => Err(Error::UnsupportedTransportType),
        }
    }
}

// ────────────────────────────────────────────────────────────
// Transfers
// ────────────────────────────────────────────────────────────

/// Which transfers to list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum TransferScope {
    /// All transfers
    All,
    /// Transfers committed by the on-chain transaction with the given TXID
    Txid(String),
    /// Transfers belonging to the batch transfer with the given index
    BatchTransfer(i32),
}

/// The type of an RGB transfer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum TransferKind {
    /// A transfer that issued the asset
    Issuance,
    /// An incoming transfer via blinded UTXO
    ReceiveBlind,
    /// An incoming transfer via a Bitcoin script (witness TX)
    ReceiveWitness,
    /// An outgoing transfer
    Send,
    /// An inflation transfer
    Inflation,
    /// A burn transfer
    Burn,
    /// A link transfer
    Link,
    /// A mint transfer, bringing in assets backed by tokens locked on the external chain
    Mint,
}

#[derive(Debug, Clone)]
pub struct TransferData {
    pub(crate) kind: TransferKind,
    pub(crate) status: TransferStatus,
    pub(crate) batch_transfer_idx: i32,
    pub(crate) assignments: Vec<Assignment>,
    pub(crate) txid: Option<String>,
    pub(crate) receive_utxo: Option<Outpoint>,
    pub(crate) change_utxos: Vec<Outpoint>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) expiration_timestamp: Option<i64>,
    pub(crate) consignment_path: Option<String>,
    pub(crate) psbt_path: Option<String>,
}

/// Data derived from an asset transfer's colorings, shared by every transfer connected to it.
pub struct AssetTransferColoringInfo<'a> {
    pub(crate) assignments: Vec<Assignment>,
    pub(crate) all_issue: bool,
    pub(crate) change_utxos: Vec<Outpoint>,
    pub(crate) receive_candidates: Vec<&'a DbTxo>,
}

impl<'a> AssetTransferColoringInfo<'a> {
    /// Build from `colorings` (all belonging to a single asset transfer) and their `txos`.
    pub(crate) fn new(colorings: &[DbColoring], txos: &'a HashMap<i32, DbTxo>) -> Self {
        let assignments = colorings
            .iter()
            .filter(|c| c.r#type != ColoringType::Input)
            .map(|c| c.assignment.clone())
            .collect();
        let all_issue =
            !colorings.is_empty() && colorings.iter().all(|c| c.r#type == ColoringType::Issue);
        let mut change_txos: Vec<&DbTxo> = colorings
            .iter()
            .filter(|c| c.r#type == ColoringType::Change)
            .filter_map(|c| txos.get(&c.txo_idx))
            .collect();
        change_txos.sort_by_key(|t| t.idx);
        change_txos.dedup_by_key(|t| t.idx);
        let change_utxos = change_txos.into_iter().map(|t| t.outpoint()).collect();
        let receive_candidates = colorings
            .iter()
            // issue coloring from inflation is considered as received
            .filter(|c| [ColoringType::Receive, ColoringType::Issue].contains(&c.r#type))
            .filter_map(|c| txos.get(&c.txo_idx))
            .collect();
        Self {
            assignments,
            all_issue,
            change_utxos,
            receive_candidates,
        }
    }
}

/// An RGB transfer.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Transfer {
    /// ID of the transfer
    pub idx: i32,
    /// ID of the batch transfer containing this transfer
    pub batch_transfer_idx: i32,
    /// Timestamp of the transfer creation
    pub created_at: i64,
    /// Timestamp of the transfer last update
    pub updated_at: i64,
    /// Status of the transfer
    pub status: TransferStatus,
    /// Requested RGB assignment
    pub requested_assignment: Option<Assignment>,
    /// RGB assignmnents
    pub assignments: Vec<Assignment>,
    /// Type of the transfer
    pub kind: TransferKind,
    /// ID of the Bitcoin transaction anchoring the transfer
    pub txid: Option<String>,
    /// Recipient ID (blinded UTXO or Bitcoin script) of an incoming transfer
    pub recipient_id: Option<String>,
    /// UTXO of an incoming transfer
    pub receive_utxo: Option<Outpoint>,
    /// UTXOs receiving the RGB change of an outgoing transfer, as it can be spread over several
    pub change_utxos: Vec<Outpoint>,
    /// Expiration of the transfer
    pub expiration_timestamp: Option<u64>,
    /// Recipient transport endpoints for the transfer
    pub transport_endpoints: Vec<TransferTransportEndpoint>,
    /// Sender transport endpoint for the transfer
    pub sender_transport_endpoint: Option<String>,
    /// Invoice string of the incoming transfer
    pub invoice_string: Option<String>,
    /// Path of the consignment file for the operation, when there's one on disk: the contract for
    /// issuances, the consignment sent for outgoing transfers, the one received for incoming
    /// transfers and the one generated for third-party verification of a burn or a link; `None`
    /// for inflations, which produce none, and for an incoming transfer that hasn't received its
    /// consignment (or never will, as one that failed while waiting for the counterparty)
    pub consignment_path: Option<String>,
    /// Path of the unsigned PSBT produced by the `_begin` step, when available
    pub psbt_path: Option<String>,
}

impl DbTransfer {
    pub(crate) fn to_transfer(
        &self,
        td: TransferData,
        transport_endpoints: Vec<TransferTransportEndpoint>,
        sender_transport_endpoint: Option<String>,
    ) -> Transfer {
        Transfer {
            idx: self.idx,
            batch_transfer_idx: td.batch_transfer_idx,
            created_at: td.created_at,
            updated_at: td.updated_at,
            status: td.status,
            requested_assignment: self.requested_assignment.clone(),
            assignments: td.assignments,
            kind: td.kind,
            txid: td.txid,
            recipient_id: self.recipient_id.clone(),
            receive_utxo: td.receive_utxo,
            change_utxos: td.change_utxos,
            expiration_timestamp: td.expiration_timestamp.map(|t| t as u64),
            transport_endpoints,
            sender_transport_endpoint,
            invoice_string: self.invoice_string.clone(),
            consignment_path: td.consignment_path,
            psbt_path: td.psbt_path,
        }
    }
}

/// An RGB transport endpoint for a transfer.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct TransferTransportEndpoint {
    /// Endpoint address
    pub endpoint: String,
    /// Endpoint transport type
    pub transport_type: TransportType,
    /// Whether the endpoint has been used
    pub used: bool,
}

impl DbTransportEndpoint {
    pub(crate) fn to_transfer_transport_endpoint(
        &self,
        x: &DbTransferTransportEndpoint,
    ) -> TransferTransportEndpoint {
        TransferTransportEndpoint {
            endpoint: self.endpoint.clone(),
            transport_type: self.transport_type,
            used: x.used,
        }
    }
}

/// Data to receive an RGB transfer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct ReceiveData {
    /// Invoice string
    pub invoice: String,
    /// ID of the receive operation (blinded UTXO or Bitcoin script)
    pub recipient_id: String,
    /// Expiration of the receive operation
    pub expiration_timestamp: u64,
    /// Batch transfer idx
    pub batch_transfer_idx: i32,
    /// Recipient's Ed25519 public key (hex-encoded), as embedded in the invoice
    pub public_key: String,
}

#[derive(Debug, Clone)]
pub struct ReceiveDataInternal {
    /// ID of the asset this receive is for, if any
    pub(crate) asset_id: Option<String>,
    /// Resolved assignment requested for this receive
    pub(crate) detected_assignment: Assignment,
    /// The RGB invoice
    pub(crate) invoice_string: String,
    /// Recipient ID (blind or witness) encoded in the invoice
    pub(crate) recipient_id: String,
    /// Transport endpoints, empty for the out-of-band exchange
    pub(crate) endpoints: Vec<String>,
    /// Creation timestamp (UTC unix seconds)
    pub(crate) created_at: i64,
    /// Invoice expiration timestamp (UTC unix seconds)
    pub(crate) expiration_timestamp: i64,
    /// Blind vs witness recipient, with its type-specific data
    pub(crate) recipient_type_full: RecipientTypeFull,
    /// Blind seal, set only for a blind receive
    pub(crate) blind_seal: Option<GraphSeal>,
    /// Script pubkey, set only for a witness receive
    pub(crate) script_pubkey: Option<ScriptBuf>,
    /// Recipient's per-operation Ed25519 secret seed (raw 32 bytes)
    pub(crate) operation_secret_key: Vec<u8>,
    /// Recipient's Ed25519 public key (raw 32 bytes), embedded in the invoice
    pub(crate) recipient_pubkey: Vec<u8>,
    /// Consignment/media encryption preference advertised in the invoice
    pub(crate) encryption: Encryption,
    /// For a blind receive reserved on proxies, the invoice expiration plus the longest retention
    /// grace among them, until which a transfer to the invoice can still be downloaded
    pub(crate) retention_deadline: Option<i64>,
}

// ────────────────────────────────────────────────────────────
// UTXOs & unspents
// ────────────────────────────────────────────────────────────

/// A Bitcoin unspent transaction output.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Utxo {
    /// UTXO outpoint
    pub outpoint: Outpoint,
    /// Amount (in sats)
    pub btc_amount: u64,
    /// Defines if the UTXO can have RGB allocations
    pub colorable: bool,
    /// Defines if the UTXO already exists (TX that creates it has been broadcasted)
    pub exists: bool,
    /// Derivation index of the UTXO's script pubkey. `None` if the transaction creating the UTXO
    /// is not known to the underlying BDK wallet, which is the case while the UTXO does not yet
    /// exist on-chain and for outputs that do not belong to this wallet.
    pub derivation_index: Option<u32>,
}

impl From<DbTxo> for Utxo {
    fn from(x: DbTxo) -> Utxo {
        Utxo {
            outpoint: x.outpoint(),
            btc_amount: x
                .btc_amount
                .parse::<u64>()
                .expect("DB should contain a valid u64 value"),
            colorable: true,
            exists: x.exists,
            derivation_index: None,
        }
    }
}

impl From<LocalOutput> for Utxo {
    fn from(x: LocalOutput) -> Utxo {
        Utxo {
            outpoint: Outpoint::from(x.outpoint),
            btc_amount: x.txout.value.to_sat(),
            colorable: false,
            exists: true,
            derivation_index: Some(x.derivation_index),
        }
    }
}

/// A wallet unspent.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Unspent {
    /// Bitcoin UTXO
    pub utxo: Utxo,
    /// RGB allocations on the UTXO
    pub rgb_allocations: Vec<RgbAllocation>,
    /// Number of pending blind receive operations
    pub pending_blinded: u32,
}

impl From<LocalUnspent> for Unspent {
    fn from(x: LocalUnspent) -> Unspent {
        Unspent {
            utxo: Utxo::from(x.utxo),
            rgb_allocations: x
                .rgb_allocations
                .into_iter()
                .map(RgbAllocation::from)
                .collect::<Vec<RgbAllocation>>(),
            pending_blinded: x.pending_blinded,
        }
    }
}

impl From<LocalOutput> for Unspent {
    fn from(x: LocalOutput) -> Unspent {
        Unspent {
            utxo: Utxo::from(x),
            rgb_allocations: vec![],
            pending_blinded: 0,
        }
    }
}

/// An RGB allocation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbAllocation {
    /// Asset ID
    pub asset_id: Option<String>,
    /// RGB assignment
    pub assignment: Assignment,
    /// Defines if the allocation is settled, meaning it refers to a transfer in the
    /// [`TransferStatus::Settled`] status
    pub settled: bool,
}

impl From<LocalRgbAllocation> for RgbAllocation {
    fn from(x: LocalRgbAllocation) -> RgbAllocation {
        RgbAllocation {
            asset_id: x.asset_id.clone(),
            assignment: x.assignment.clone(),
            settled: x.settled(),
        }
    }
}

// ────────────────────────────────────────────────────────────
// Transactions
// ────────────────────────────────────────────────────────────

/// The type of a transaction.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum TransactionType {
    /// Transaction used to perform an RGB send
    RgbSend,
    /// Transaction used to drain the RGB wallet
    Drain,
    /// Transaction used to create UTXOs
    CreateUtxos,
    /// Transaction used to perform a BTC send
    SendBtc,
    /// Incoming transaction
    Incoming,
}

/// A pending vanilla transaction that has reserved TXOs in the wallet.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct PendingVanillaTx {
    /// Transaction ID
    pub txid: String,
    /// Type of vanilla operation that reserved the TXOs
    pub r#type: WalletTransactionType,
}

/// A Bitcoin transaction.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Transaction {
    /// Type of transaction
    pub transaction_type: TransactionType,
    /// Transaction ID
    pub txid: String,
    /// Received value (in sats), computed as the sum of owned output amounts included in this
    /// transaction
    pub received: u64,
    /// Sent value (in sats), computed as the sum of owned input amounts included in this
    /// transaction
    pub sent: u64,
    /// Fee value (in sats)
    pub fee: u64,
    /// Height and Unix timestamp of the block containing the transaction if confirmed, `None` if
    /// unconfirmed
    pub confirmation_time: Option<BlockTime>,
}

// ────────────────────────────────────────────────────────────
// PSBT & RGB inspection
// ────────────────────────────────────────────────────────────

/// Information about a PSBT input for inspection.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct PsbtInputInfo {
    /// The outpoint being spent
    pub outpoint: Outpoint,
    /// The amount in satoshis
    pub amount_sat: u64,
    /// Whether this input belongs to the wallet's descriptors
    pub is_mine: bool,
}

/// Information about a PSBT output for inspection.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct PsbtOutputInfo {
    /// The output address (if parseable)
    pub address: Option<String>,
    /// The output script pubkey (hex encoded)
    pub script_pubkey_hex: String,
    /// The amount in satoshis
    pub amount_sat: u64,
    /// Whether this is an OP_RETURN output
    pub is_op_return: bool,
    /// Whether this is an output that will belong to the wallet
    pub is_mine: bool,
}

/// Inspection results for a PSBT.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct PsbtInspection {
    /// Transaction ID
    pub txid: String,
    /// List of inputs
    pub inputs: Vec<PsbtInputInfo>,
    /// List of outputs
    pub outputs: Vec<PsbtOutputInfo>,
    /// Total input amount in satoshis
    pub total_input_sat: u64,
    /// Total output amount in satoshis
    pub total_output_sat: u64,
    /// Calculated fee in satoshis
    pub fee_sat: u64,
    /// Number of signatures present
    pub signature_count: u16,
    /// Transaction size in virtual bytes
    pub size_vbytes: u64,
}

// ────────────────────────────────────────────────────────────
// Send, inflate, burn, mint & refresh operations
// ────────────────────────────────────────────────────────────

/// The result of a burn begin operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct BurnBeginResult {
    /// PSBT to inspect and sign
    pub psbt: String,
    /// Batch transfer idx, None when `dry_run: true`
    pub batch_transfer_idx: Option<i32>,
    /// Operation details
    pub details: BurnDetails,
}

/// Details for burn operations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct BurnDetails {
    /// Path to the strict-encoded fascia file, for inspection
    pub fascia_path: String,
    /// Minimum confirmations for the operation
    pub min_confirmations: u8,
    /// Entropy used for the merkle tree construction operation
    pub entropy: u64,
    /// Hex-encoded 32-byte reason committed in the burn transition, if any
    pub burn_reason: Option<String>,
}

/// The result of a link begin operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct LinkBeginResult {
    /// PSBT to inspect and sign
    pub psbt: String,
    /// Batch transfer idx, None when `dry_run: true`
    pub batch_transfer_idx: Option<i32>,
    /// Operation details
    pub details: LinkDetails,
}

/// Details for link operations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct LinkDetails {
    /// Path to the strict-encoded fascia file, for inspection
    pub fascia_path: String,
    /// Minimum confirmations for the operation
    pub min_confirmations: u8,
    /// Entropy used for the merkle tree construction operation
    pub entropy: u64,
}

/// The result of an inflate begin operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct InflateBeginResult {
    /// PSBT to inspect and sign
    pub psbt: String,
    /// Batch transfer idx, None when `dry_run: true`
    pub batch_transfer_idx: Option<i32>,
    /// Operation details
    pub details: InflateDetails,
}

/// Details for inflate operations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct InflateDetails {
    /// Path to the strict-encoded fascia file, for inspection
    pub fascia_path: String,
    /// Minimum confirmations for the operation
    pub min_confirmations: u8,
    /// Entropy used for the merkle tree construction operation
    pub entropy: u64,
}

/// The result of a mint begin operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct MintBeginResult {
    /// PSBT to inspect and sign
    pub psbt: String,
    /// Batch transfer idx
    pub batch_transfer_idx: i32,
    /// Operation details
    pub details: MintDetails,
}

/// Details for mint operations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct MintDetails {
    /// Path to fascia file for inspection
    pub fascia_path: String,
    /// Minimum confirmations for the operation
    pub min_confirmations: u8,
    /// Entropy used for the merkle tree construction operation
    pub entropy: u64,
    /// ID of the mint operation
    pub opid: String,
}

/// A mint received by the wallet, as checked by it: what its sender has to lock on the external
/// chain for the mint to complete.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct ReceivedMint {
    /// ID of the mint operation
    pub opid: String,
    /// ID the lock has to be made for, committing to the mint operation and to the TX including
    /// it, so that the lock confirms the mint only in that TX: the hex-encoded tagged SHA-256 of
    /// the operation ID followed by the TXID in internal byte order (reversed compared to how it's
    /// usually displayed), i.e. `SHA256(t || t || opid || txid)` where `t` is the SHA-256 of the
    /// tag `urn:lnp-bp:rgb:lock#2026-11-04`
    pub lock_id: String,
    /// Amount minted, which the lock has to match
    pub amount: u64,
    /// External chain block the lock has to be included before
    pub before_block: u64,
    /// EIP-155 chain ID of the EVM chain the asset's bridge contract lives on
    pub evm_chain_id: u64,
    /// Address of the asset's bridge contract, where the lock has to be made
    pub bridge_contract: String,
}

/// The result of a send begin operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct SendBeginResult {
    /// PSBT to inspect and sign
    pub psbt: String,
    /// Batch transfer idx, None when `dry_run: true`
    pub batch_transfer_idx: Option<i32>,
    /// Operation details
    pub details: SendDetails,
}

/// Details for send operations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct SendDetails {
    /// Path to the strict-encoded fascia file, for inspection
    pub fascia_path: String,
    /// Minimum confirmations for the operation
    pub min_confirmations: u8,
    /// Entropy used for the merkle tree construction operation
    pub entropy: u64,
    /// Whether this is a donation transfer
    pub is_donation: bool,
}

/// The result of an operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct OperationResult {
    /// ID of the transaction
    pub txid: String,
    /// Batch transfer idx
    pub batch_transfer_idx: i32,
    /// Entropy used for the merkle tree construction operation
    pub entropy: u64,
}

/// The pending status of a [`Transfer`] (eligible for refresh).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg(feature = "_online")]
pub enum RefreshTransferStatus {
    /// Waiting for the counterparty to take action
    WaitingCounterparty,
    /// Waiting for the safe height to be reached
    WaitingSafeHeight,
    /// Waiting for the transfer transaction to be broadcasted
    WaitingBroadcast,
    /// Waiting for the transfer transaction to reach the minimum number of confirmations
    WaitingConfirmations,
}

#[cfg(feature = "_online")]
impl TryFrom<TransferStatus> for RefreshTransferStatus {
    type Error = &'static str;

    fn try_from(x: TransferStatus) -> Result<Self, Self::Error> {
        match x {
            TransferStatus::WaitingCounterparty => Ok(RefreshTransferStatus::WaitingCounterparty),
            TransferStatus::WaitingSafeHeight => Ok(RefreshTransferStatus::WaitingSafeHeight),
            TransferStatus::WaitingBroadcast => Ok(RefreshTransferStatus::WaitingBroadcast),
            TransferStatus::WaitingConfirmations => Ok(RefreshTransferStatus::WaitingConfirmations),
            _ => Err("RefreshTransferStatus only accepts waiting statuses"),
        }
    }
}

/// A transfer refresh filter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RefreshFilter {
    /// Transfer status
    pub status: RefreshTransferStatus,
    /// Whether the transfer is incoming
    pub incoming: bool,
}

/// A refreshed transfer
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg(feature = "_online")]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RefreshedTransfer {
    /// The updated transfer status, if it has changed
    pub updated_status: Option<TransferStatus>,
    /// Optional failure
    pub failure: Option<Error>,
}

/// The result of a refresh operation
#[cfg(feature = "_online")]
pub type RefreshResult = HashMap<i32, RefreshedTransfer>;

#[cfg(feature = "_online")]
pub(crate) trait RefreshResultTrait {
    fn transfers_changed(&self) -> bool;
}

#[cfg(feature = "_online")]
impl RefreshResultTrait for RefreshResult {
    fn transfers_changed(&self) -> bool {
        self.values().any(|rt| rt.updated_status.is_some())
    }
}

// ────────────────────────────────────────────────────────────
// Internal objects
// ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalTransportEndpoint {
    pub transport_type: TransportType,
    pub endpoint: String,
    pub used: bool,
    pub usable: bool,
}

#[derive(Debug, Clone)]
pub struct LocalUnspent {
    /// Database UTXO
    pub utxo: DbTxo,
    /// RGB allocations on the UTXO
    pub rgb_allocations: Vec<LocalRgbAllocation>,
    /// Number of pending blind receive operations
    pub pending_blinded: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalWitnessData {
    pub amount_sat: u64,
    pub blinding: Option<u64>,
    pub vout: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum LocalRecipientData {
    Blind(SecretSeal),
    Witness(LocalWitnessData),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalRecipient {
    /// Recipient ID (blind or witness) encoded in the invoice
    pub recipient_id: String,
    /// Blind vs witness recipient, with its type-specific data
    pub local_recipient_data: LocalRecipientData,
    /// Assignment being sent to this recipient
    pub assignment: Assignment,
    /// Transport endpoints, empty for the out-of-band exchange
    pub transport_endpoints: Vec<LocalTransportEndpoint>,
    /// Recipient's Ed25519 public key (bytes) from the invoice, empty for the out-of-band exchange
    pub public_key: Vec<u8>,
    /// Recipient's encryption preference, from the invoice
    pub encryption: Encryption,
    /// Invoice expiration (UTC unix seconds), from the invoice, `None` for the out-of-band exchange
    #[serde(default)]
    pub invoice_expiration: Option<i64>,
}

#[cfg(feature = "_online")]
impl LocalRecipient {
    /// The IDs of the invoice and of the transfer to it on the proxy, given the hex-encoded pubkey
    /// of the sender operation.
    pub(crate) fn proxy_ids(&self, sender_pk_hex: &str) -> (String, String) {
        let invoice_id = invoice_id(&self.recipient_id, &hex::encode(&self.public_key));
        let transfer_id = transfer_id(&invoice_id, sender_pk_hex);
        (invoice_id, transfer_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalRgbAllocation {
    /// Asset ID
    pub asset_id: Option<String>,
    /// RGB assignment
    pub assignment: Assignment,
    /// The status of the transfer that produced the RGB allocation
    pub status: TransferStatus,
    /// Defines if the allocation is incoming
    pub incoming: bool,
    /// Defines if the allocation is on a spent TXO
    pub txo_spent: bool,
}

impl LocalRgbAllocation {
    pub(crate) fn settled(&self) -> bool {
        !self.status.failed()
            && ((!self.txo_spent && self.incoming && self.status.settled())
                || (self.txo_spent && !self.incoming && self.status.waiting_confirmations()))
    }

    pub(crate) fn future(&self) -> bool {
        !self.txo_spent && self.incoming && !self.status.failed() && !self.settled()
    }

    /// Whether the allocation is held by its UTXO, or due to be, so that spending the UTXO moves
    /// it: an incoming allocation on an unspent TXO, from a transfer that hasn't failed.
    pub(crate) fn held(&self) -> bool {
        self.incoming && !self.txo_spent && !self.status.failed()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetSpend {
    pub input_outpoints: Vec<Outpoint>,
    pub assignments_collected: AssignmentsCollection,
    pub input_btc_amt: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetInfo {
    pub contract_id: ContractId,
    pub schema: AssetSchema,
    pub reject_list_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BtcChange {
    pub vout: u32,
    pub amount: u64,
}

/// The main transition of an operation moving an asset, with the data it commits to beside its
/// assignments.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum MainTransition {
    /// A transfer
    Transfer,
    /// An inflation
    Inflate,
    /// A burn, committing to its reason, if any
    Burn {
        /// The burn reason
        burn_reason: Option<BurnReason>,
    },
    /// A link, committing to the contract it links to
    Link {
        /// The contract this is linked to
        linked_to_contract: ContractId,
    },
    /// A mint, committing to the external chain block the lock has to be included before
    Mint {
        /// The external chain block the lock has to be included before
        before_block: u64,
    },
}

#[cfg(feature = "_online")]
impl MainTransition {
    /// The type of the transition.
    pub(crate) fn transition_type(&self) -> TypeOfTransition {
        match self {
            Self::Transfer => TypeOfTransition::Transfer,
            Self::Inflate => TypeOfTransition::Inflate,
            Self::Burn { .. } => TypeOfTransition::Burn,
            Self::Link { .. } => TypeOfTransition::Link,
            Self::Mint { .. } => TypeOfTransition::Mint,
        }
    }

    /// The reason a burn commits to, if this is a burn with one.
    pub(crate) fn burn_reason(&self) -> Option<BurnReason> {
        match self {
            Self::Burn { burn_reason } => *burn_reason,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InfoAssetTransfer {
    pub asset_info: AssetInfo,
    pub recipients: Vec<LocalRecipient>,
    pub asset_spend: AssetSpend,
    pub change: AssignmentsCollection,
    pub original_assignments_needed: AssignmentsCollection,
    pub assignments_needed: AssignmentsCollection,
    pub assignments_spent: HashMap<OutPoint, Vec<Assignment>>,
    /// Where each allocation of the change of the asset goes
    pub change_destinations: Vec<(Assignment, ChangeDestination)>,
    pub main_transition: MainTransition,
    pub beneficiaries_blinded: Vec<SecretSeal>,
    pub beneficiaries_witness: Vec<ExplicitSeal<RgbTxid>>,
}

impl InfoAssetTransfer {
    /// OpIds of the transitions to explicitly request when consigning from `fascia`: a burn or a
    /// link has no beneficiary, so its transition is selected by OpId, while the others are
    /// selected by their beneficiaries
    pub(crate) fn requested_opids(&self, fascia: &Fascia) -> Vec<OpId> {
        let transition_type = match self.main_transition {
            MainTransition::Burn { .. } => TS_BURN,
            MainTransition::Link { .. } => TS_LINK,
            MainTransition::Transfer | MainTransition::Inflate | MainTransition::Mint { .. } => {
                return vec![];
            }
        };
        fascia
            .bundles()
            .get(&self.asset_info.contract_id)
            .map(|bundle| {
                bundle
                    .known_transitions
                    .iter()
                    .filter(|kt| kt.transition.transition_type == transition_type)
                    .map(|kt| kt.opid)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Where an allocation moved to the RGB change of an operation goes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum ChangeDestination {
    /// The new Bitcoin output created by the operation to hold RGB change
    BtcChange,
    /// A pre-existing UTXO
    Utxo(Outpoint),
}

/// The allocations moved along by an operation, by asset ID and outpoint they're moved from, each
/// with where it goes.
pub type ExtraAllocations =
    HashMap<String, HashMap<OutPoint, Vec<(Assignment, ChangeDestination)>>>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InfoBatchTransfer {
    /// New Bitcoin output created by this transfer to hold RGB change, if any
    pub btc_change: Option<BtcChange>,
    /// Allocations other than the ones being sent, moved as a side effect of spending a UTXO they
    /// shared with a sent asset, by asset ID and spent outpoint, each with where it goes
    pub extra_allocations: ExtraAllocations,
    /// Whether this is a donation (broadcast without waiting for the recipients' ACK)
    pub donation: bool,
    /// Number of confirmations required before the transfer is considered settled
    pub min_confirmations: u8,
    /// Expiration timestamp (UTC unix seconds), if any
    pub expiration_timestamp: Option<i64>,
    /// Creation timestamp (UTC unix seconds)
    pub created_at: i64,
    /// Entropy used for the committment
    pub entropy: u64,
    /// Per-asset transfer info, keyed by asset ID
    pub transfers: BTreeMap<String, InfoAssetTransfer>,
    /// Sender's proxy endpoint (where recipients post their ACK to and fetch the signed TX from);
    /// `None` for donations and non-counterparty operations
    pub sender_transport_endpoint: Option<String>,
    /// Sender's per-operation Ed25519 secret seed (raw 32 bytes); shared across all recipients
    pub operation_secret_key: Option<Vec<u8>>,
}

#[cfg(feature = "_online")]
impl InfoBatchTransfer {
    /// Whether this is the data of an operation of the kind identified by the given main
    /// transition, i.e. whether every transfer of the batch has it
    pub(crate) fn is_operation(&self, main_transition: &TypeOfTransition) -> bool {
        !self.transfers.is_empty()
            && self
                .transfers
                .values()
                .all(|t| &t.main_transition.transition_type() == main_transition)
    }
}

pub type TransferEndData = (String, PathBuf, InfoBatchTransfer, Fascia);

#[cfg(feature = "_online")]
pub struct BeginOperationData {
    pub psbt: Psbt,
    pub transfer_dir: PathBuf,
    pub info_batch_transfer: InfoBatchTransfer,
    pub batch_transfer_idx: Option<i32>,
    pub opid: Option<String>,
}

#[cfg(feature = "_online")]
pub enum PrepareRgbPsbtResult {
    Retry,
    Success(Box<BeginOperationData>),
}

#[cfg(feature = "_online")]
pub enum PrepareTransferPsbtResult {
    Retry,
    Success(Box<BeginOperationData>),
}

#[cfg(feature = "_online")]
#[derive(Debug, Deserialize, Serialize)]
pub struct ReceivedConsignmentMeta {
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub transfer_id: Option<String>,
    pub sender_transport_endpoint: String,
    pub sender_pubkey: String,
    pub consignment_hash: String,
    pub batch: bool,
}

/// A mint sent by the wallet: what its lock on the external chain has to match.
#[cfg(feature = "_online")]
pub struct SentMint {
    /// ID of the minted asset
    pub contract_id: ContractId,
    /// ID of the mint operation
    pub opid: OpId,
    /// ID the lock has to be made for, committing to the mint operation and to the TX including
    /// it
    pub lock_id: LockId,
    /// Amount minted, which the lock has to match
    pub amount: u64,
    /// External chain block the lock has to be included before
    pub before_block: u64,
    /// Location of the asset's bridge contract, where the lock has to be made
    pub bridge_location: BridgeLocation,
}

/// The mint locks a received consignment waits for, saved once it has been validated as far as it
/// can be without them, so that until they're finalized a refresh only looks them up.
#[cfg(feature = "_online")]
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct PendingMintLocks {
    /// SHA-256 digest (hex-encoded) of the consignment file waiting for the locks
    pub(crate) consignment_digest: String,
    /// EIP-155 chain ID of the EVM chain the bridge contract lives on
    pub(crate) evm_chain_id: u64,
    /// Address of the bridge contract recording the locks
    pub(crate) bridge_contract: EvmAddress,
    /// The claims of the mints whose locks aren't finalized yet
    pub(crate) claims: Vec<Claim>,
}

/// The sender proxy's response to `ack.clean`, requested before failing an outgoing operation.
#[cfg(feature = "_online")]
#[derive(Debug)]
pub enum CleanAckOutcome {
    /// the signed TXs were revoked: nothing can be released anymore
    Revoked,
    /// the TX was already released to a recipient, who may still broadcast it
    Released,
    /// the proxy doesn't know the operation: it was never registered (send_end not completed),
    /// or the proxy lost it (e.g. it dropped its data), in which case whether the TX was
    /// released cannot be known
    Unknown,
}

#[cfg(feature = "_online")]
pub enum TryFailBatchTransferOutcome {
    Failed,
    Refreshed,
    /// the transfer was not failed: the released signed TX could not be revoked (ack.clean
    /// denied), or the sender proxy no longer knows the operation and it has not expired yet
    Denied,
}

#[cfg(feature = "_online")]
pub struct FailTransfersOutcome {
    pub transfers_changed: bool,
    pub cannot_fail: bool,
}

#[cfg(feature = "_online")]
pub enum ReceiveMode {
    Proxy(ProxyReceiveData),
    OutOfBand { media_file_paths: Vec<String> },
}

/// Proxy-only context needed to receive (and decrypt) a consignment via a proxy server.
///
/// These fields only make sense for the proxy exchange: the sender's public key and the
/// recipient's encryption preference drive the ECDH decryption of the consignment/media/TX, and
/// the sender proxy endpoint + consignment hash are needed to sign and post the ACK. Out-of-band
/// exchange bypasses the proxy entirely (no key exchange, integrity covered by the plaintext hash),
/// so it carries none of this.
#[cfg(feature = "_online")]
#[derive(Clone)]
pub struct ProxyReceiveData {
    /// endpoint the consignment was downloaded from
    pub(crate) proxy_url: String,
    /// ID of the transfer on the proxies
    pub(crate) transfer_id: String,
    /// sender's proxy endpoint (HTTP URL), where the ACK is posted
    pub(crate) sender_proxy: String,
    /// sender's Ed25519 public key (hex), used to derive the shared secret for decryption
    pub(crate) sender_pubkey: String,
    /// hash of the (plaintext) consignment, signed into the ACK and used for blob verification
    pub(crate) consignment_hash: String,
    /// recipient's encryption preference for this operation
    pub(crate) recipient_encryption: Encryption,
    /// whether the operation is a batch (multiple recipients)
    pub(crate) batch: bool,
}

#[cfg(feature = "_online")]
pub enum ReceiveMatcher {
    Blind(SecretSeal),
    /// the script paid and, when known (proxy exchange), the blinding of the seal
    /// (`v1.0-changes-spec.md` A.11)
    Witness(ScriptBuf, Option<u64>),
}

/// The allocation slots taken on a UTXO receiving a consignment, besides the ones of the transfer
/// it pays.
#[cfg(feature = "_online")]
#[derive(Clone, Copy, Debug, Default)]
pub struct TakenSlots {
    /// allocations of the asset received held or due
    pub(crate) asset: usize,
    /// amount of the asset received held or due
    pub(crate) asset_amount: u128,
    /// allocations of any asset held or due, including the ones of the asset received
    pub(crate) all: usize,
    /// other unpaid invoices, each due to receive an allocation of any asset
    pub(crate) invoices: usize,
    /// mint rights of the asset received held or due, and other unpaid invoices requesting one
    pub(crate) mint_rights: usize,
}

/// The UTXOs selected as inputs of an operation, with the number of allocations of each asset they
/// hold, which spending them moves, as inputs of the bundle of the asset, and their total amount,
/// as a transition can't spend more than `u64::MAX` of an asset (which BFA assets can exceed).
#[cfg(feature = "_online")]
#[derive(Clone, Debug, Default)]
pub struct SelectedInputs {
    outpoints: HashSet<Outpoint>,
    // map of asset IDs to the number of their allocations and their total amount
    allocations: HashMap<String, (usize, u128)>,
}

#[cfg(feature = "_online")]
impl SelectedInputs {
    /// The number of allocations of each asset held by the given UTXO, with their total amount.
    fn held_allocations(unspent: &LocalUnspent) -> HashMap<&str, (usize, u128)> {
        let mut held: HashMap<&str, (usize, u128)> = HashMap::new();
        for allocation in unspent
            .rgb_allocations
            .iter()
            .filter(|a| a.held())
            // an allocation without an asset ID is due from a receive of an asset not known yet,
            // which makes its UTXO unspendable (see `get_input_unspents`); a UTXO used as change
            // gets checked when the receive gets its asset, counting the change (see
            // `count_utxo_taken_slots`)
            .filter(|a| a.asset_id.is_some())
        {
            let (num, amount) = held
                .entry(allocation.asset_id.as_deref().unwrap())
                .or_default();
            *num += 1;
            *amount += u128::from(allocation.assignment.main_amount());
        }
        held
    }

    /// Whether the given allocations of each asset, added to the ones of the UTXOs selected, don't
    /// exceed `max_allocations` nor an amount of `u64::MAX`.
    fn held_fit(&self, held: &HashMap<&str, (usize, u128)>, max_allocations: usize) -> bool {
        held.iter().all(|(asset_id, (num, amount))| {
            let (selected_num, selected_amount) =
                self.allocations.get(*asset_id).copied().unwrap_or_default();
            selected_num + num <= max_allocations
                && selected_amount + amount <= u128::from(u64::MAX)
        })
    }

    /// Select the given UTXO, unless the allocations of an asset it holds, added to the ones of the
    /// UTXOs selected, exceed `max_allocations` or an amount of `u64::MAX`, returning whether it's
    /// selected. A UTXO already selected (e.g. for another asset of the same operation) is skipped
    /// since it's already been accounted for (allocations would otherwise be counted twice).
    pub(crate) fn select(&mut self, unspent: &LocalUnspent, max_allocations: usize) -> bool {
        let outpoint = unspent.utxo.outpoint();
        if self.outpoints.contains(&outpoint) {
            return true;
        }
        // counted once, for both the check and the selection, as a UTXO can hold many
        let held = Self::held_allocations(unspent);
        if !self.held_fit(&held, max_allocations) {
            return false;
        }
        for (asset_id, (num, amount)) in held {
            let (selected_num, selected_amount) =
                self.allocations.entry(asset_id.to_string()).or_default();
            *selected_num += num;
            *selected_amount += amount;
        }
        self.outpoints.insert(outpoint);
        true
    }
}

/// The allocations of an asset a destination of the RGB change holds or is due to receive.
#[cfg(feature = "_online")]
#[derive(Clone, Debug, Default)]
struct ChangeSlotAsset {
    /// their number, of any kind (inflation rights included)
    num: usize,
    /// the total amount of the asset they hold, which can't go over `u64::MAX`, as a transition
    /// can't spend more than that of an asset (which BFA assets can exceed), so a destination
    /// holding more would be stuck: a right holds no amount of the asset, so it doesn't add to it,
    /// not even an inflation right (whose amount is an allowance to issue, a state of its own).
    /// A `u128` so that adding an allocation to the total cannot overflow before the check
    amount: u128,
    /// whether one of them is a mint right, or an unpaid invoice requests one
    mint_right: bool,
}

/// A destination of the RGB change of an operation, with the slots left on it.
#[cfg(feature = "_online")]
#[derive(Clone, Debug)]
struct ChangeSlot {
    destination: ChangeDestination,
    /// the outpoint of a pre-existing UTXO, parsed once, as a seal is built on it for each
    /// allocation it receives
    outpoint: Option<OutPoint>,
    /// the allocations it can still receive
    free: usize,
    /// the allocations of each asset it holds or is due to receive
    assets: HashMap<String, ChangeSlotAsset>,
}

/// The destinations the RGB change of an operation can go to, in order of preference: the new
/// Bitcoin output of its TX, if any, then pre-existing UTXOs. A destination receives allocations up
/// to the ones allowed per UTXO, counting the ones it holds or is due to receive and the slots
/// reserved by its unpaid invoices, as long as it doesn't hold more allocations of an asset than a
/// bundle can spend, nor more than `u64::MAX` of an asset (which a transition can't spend), nor
/// more than one mint right of an asset. Each allocation is placed on its own, so the change is
/// spread over as many destinations as needed.
#[cfg(feature = "_online")]
#[derive(Clone, Debug, Default)]
pub struct ChangeSlots {
    slots: Vec<ChangeSlot>,
    max_asset_allocations: usize,
    /// the assets whose unpaid invoices requesting a mint right have been accounted for
    mint_right_invoices: HashSet<String>,
}

#[cfg(feature = "_online")]
impl ChangeSlots {
    /// The destinations of the change of an operation, with or without a new Bitcoin output for
    /// it, then the given pre-existing UTXOs, in order of preference, each allowed to hold
    /// `max_allocations` allocations and `max_asset_allocations` of each asset.
    pub(crate) fn new(
        btc_change: bool,
        utxos: &[LocalUnspent],
        max_allocations: usize,
        max_asset_allocations: usize,
    ) -> Self {
        let mut slots = vec![];
        if btc_change {
            slots.push(ChangeSlot {
                destination: ChangeDestination::BtcChange,
                outpoint: None,
                free: max_allocations,
                assets: HashMap::new(),
            });
        }
        for utxo in utxos {
            let held: Vec<&LocalRgbAllocation> =
                utxo.rgb_allocations.iter().filter(|a| a.held()).collect();
            let taken = held.len() + utxo.pending_blinded as usize;
            let free = max_allocations.saturating_sub(taken);
            if free == 0 {
                continue;
            }
            let mut assets: HashMap<String, ChangeSlotAsset> = HashMap::new();
            // allocations of a receive whose asset isn't known yet only count towards the total;
            // the receive checks the per-asset limit once its asset is known, counting any
            // change too (see `count_utxo_taken_slots`)
            for allocation in held.iter().filter(|a| a.asset_id.is_some()) {
                let asset = assets
                    .entry(allocation.asset_id.clone().unwrap())
                    .or_default();
                asset.num += 1;
                asset.amount += u128::from(allocation.assignment.main_amount());
                asset.mint_right |= allocation.assignment == Assignment::MintRight;
            }
            slots.push(ChangeSlot {
                destination: ChangeDestination::Utxo(utxo.utxo.outpoint()),
                outpoint: Some(utxo.utxo.clone().into()),
                free,
                assets,
            });
        }
        Self {
            slots,
            max_asset_allocations,
            mint_right_invoices: HashSet::new(),
        }
    }

    /// Whether the unpaid invoices requesting a mint right of the asset with ID `asset_id` have
    /// been accounted for (see [`Self::add_mint_right_invoices`]).
    pub(crate) fn knows_mint_right_invoices(&self, asset_id: &str) -> bool {
        self.mint_right_invoices.contains(asset_id)
    }

    /// Account for the unpaid invoices requesting a mint right of the asset with ID `asset_id`,
    /// on the given UTXOs, which can't receive another one.
    pub(crate) fn add_mint_right_invoices(&mut self, asset_id: &str, utxos: &HashSet<Outpoint>) {
        for slot in &mut self.slots {
            if let ChangeDestination::Utxo(outpoint) = &slot.destination
                && utxos.contains(outpoint)
            {
                slot.assets
                    .entry(asset_id.to_string())
                    .or_default()
                    .mint_right = true;
            }
        }
        self.mint_right_invoices.insert(asset_id.to_string());
    }

    /// Take the slot for an allocation of the asset with ID `asset_id`, of the given amount and
    /// whether it's a mint right, on the first destination that can receive it.
    ///
    /// Returns `None` if no destination can receive it, otherwise the destination taken, with the
    /// outpoint of the pre-existing UTXO it is (`None` for the new Bitcoin change output).
    pub(crate) fn take(
        &mut self,
        asset_id: &str,
        amount: u128,
        mint_right: bool,
    ) -> Option<(ChangeDestination, Option<OutPoint>)> {
        let max_asset_allocations = self.max_asset_allocations;
        let slot = self.slots.iter_mut().find(|s| {
            let asset = s.assets.get(asset_id).cloned().unwrap_or_default();
            s.free > 0
                && asset.num < max_asset_allocations
                && asset.amount + amount <= u128::from(u64::MAX)
                && !(mint_right && asset.mint_right)
        })?;
        slot.free -= 1;
        let asset = slot.assets.entry(asset_id.to_string()).or_default();
        asset.num += 1;
        asset.amount += amount;
        asset.mint_right |= mint_right;
        Some((slot.destination.clone(), slot.outpoint))
    }
}

/// The seal at which a consignment pays this recipient: a concealed one (blind receives) or one
/// revealed on a vout of the bundle witness TX (witness receives), never both.
#[cfg(feature = "_online")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentSeal {
    Concealed(SecretSeal),
    WitnessVout(u32),
}

#[cfg(feature = "_online")]
impl PaymentSeal {
    /// the vout of the bundle witness TX being paid, for witness receives
    pub(crate) fn vout(&self) -> Option<u32> {
        match self {
            Self::Concealed(_) => None,
            Self::WitnessVout(vout) => Some(*vout),
        }
    }

    pub(crate) fn matches(&self, seal: &BuilderSeal<GraphSeal>) -> bool {
        match (self, seal) {
            (Self::Concealed(secret_seal), BuilderSeal::Concealed(seal)) => secret_seal == seal,
            (Self::WitnessVout(vout), BuilderSeal::Revealed(seal)) => {
                seal.txid == TxPtr::WitnessTx && seal.vout.into_u32() == *vout
            }
            _ => false,
        }
    }
}

/// A destination at which a consignment pays this recipient, see `find_payments`.
#[cfg(feature = "_online")]
pub struct ConsignmentPayment<'c> {
    /// the bundle paying the recipient
    pub(crate) bundle: &'c WitnessBundle,
    /// the seal at which the bundle pays the recipient
    pub(crate) payment_seal: PaymentSeal,
    /// the assignments received, unspent within the consignment
    pub(crate) receiving: HashMap<Opout, Assignment>,
}

impl DbTransfer {
    #[cfg(feature = "_online")]
    pub(crate) fn receive_matcher(&self, blinding: Option<u64>) -> Result<ReceiveMatcher, Error> {
        let recipient_id = self
            .recipient_id
            .clone()
            .expect("transfer should have a recipient ID");
        match self
            .recipient_type
            .as_ref()
            .expect("transfer should have a recipient type")
        {
            RecipientTypeFull::Blind { .. } => {
                let beneficiary = XChainNet::<Beneficiary>::from_str(&recipient_id)
                    .expect("saved recipient ID is invalid");
                match beneficiary.into_inner() {
                    Beneficiary::BlindedSeal(secret_seal) => Ok(ReceiveMatcher::Blind(secret_seal)),
                    _ => unreachable!("beneficiary is blinded"),
                }
            }
            RecipientTypeFull::Witness { .. } => {
                let script_pubkey = script_buf_from_recipient_id(recipient_id)?
                    .expect("witness recipient ID should yield a script");
                Ok(ReceiveMatcher::Witness(script_pubkey, blinding))
            }
        }
    }
}

#[cfg(test)]
#[cfg(feature = "_online")]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "_online")]
    fn display_indexer_protocol() {
        assert_eq!(IndexerProtocol::Bitcoind.to_string(), "Bitcoind");
        assert_eq!(IndexerProtocol::Electrum.to_string(), "Electrum");
        assert_eq!(IndexerProtocol::Esplora.to_string(), "Esplora");
    }

    #[test]
    #[cfg(feature = "_online")]
    fn refresh_transfer_status_matches_waiting() {
        for status in TransferStatus::iter() {
            assert_eq!(
                status.waiting(),
                RefreshTransferStatus::try_from(status).is_ok(),
            );
        }
    }
}
