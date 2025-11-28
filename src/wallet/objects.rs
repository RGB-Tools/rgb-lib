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

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub struct OnlineData {
    pub(crate) id: u64,
    pub(crate) indexer_url: String,
    pub(crate) indexer: Indexer,
    pub(crate) resolver: AnyResolver,
    pub(crate) hub_client: Option<MultisigHubClient>,
    pub(crate) user_role: Option<UserRole>,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
                wallet.database().iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = wallet.database().get_asset_balance(
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
                wallet.database().iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = wallet.database().get_asset_balance(
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
                wallet.database().iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = wallet.database().get_asset_balance(
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
                wallet.database().iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.media_dir()))
        };
        let balance = wallet.database().get_asset_balance(
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
}

pub(crate) trait IssuedAssetDetails: Sized {
    fn from_issuance(
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        issue_data: &IssueData,
    ) -> Result<Self, Error>;
}

impl IssuedAssetDetails for AssetNIA {
    fn from_issuance(
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        _issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(wallet, asset, None, None, None, None, None, None)
    }
}

impl IssuedAssetDetails for AssetUDA {
    fn from_issuance(
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(
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
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        _issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(wallet, asset, None, None, None, None, None, None)
    }
}

impl IssuedAssetDetails for AssetIFA {
    fn from_issuance(
        wallet: &(impl WalletOffline + ?Sized),
        asset: &DbAsset,
        _issue_data: &IssueData,
    ) -> Result<Self, Error> {
        Self::get_asset_details(wallet, asset, None, None, None, None, None, None)
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
    pub(crate) token: Option<Token>,
    pub(crate) timestamp: i64,
    pub(crate) added_at: i64,
}

#[derive(Debug, Clone)]
pub struct IssueData {
    pub(crate) asset_data: LocalAssetData,
    pub(crate) valid_contract: ValidContract,
    #[cfg(any(feature = "electrum", feature = "esplora"))]
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
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl AssignmentsCollection {
    fn add_fungible(&mut self, amt: u64) {
        self.fungible += amt;
    }

    fn add_non_fungible(&mut self) {
        self.non_fungible = true;
    }

    fn add_inflation(&mut self, amt: u64) {
        self.inflation += amt;
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
            _ => false,
        }
    }

    pub(crate) fn change(&self, needed: &Self) -> Self {
        Self {
            fungible: self.fungible - needed.fungible,
            non_fungible: false,
            inflation: self.inflation - needed.inflation,
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
        true
    }
}

/// Type of RGB transition
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum TypeOfTransition {
    /// Inflation transition (issuing new tokens)
    Inflate,
    /// Transfer transition (moving existing tokens)
    Transfer,
}

impl TypeOfTransition {
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::Inflate => "inflate",
            Self::Transfer => "transfer",
        }
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
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
}

#[derive(Debug, Clone)]
pub struct TransferData {
    pub(crate) kind: TransferKind,
    pub(crate) status: TransferStatus,
    pub(crate) batch_transfer_idx: i32,
    pub(crate) assignments: Vec<Assignment>,
    pub(crate) txid: Option<String>,
    pub(crate) receive_utxo: Option<Outpoint>,
    pub(crate) change_utxo: Option<Outpoint>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) expiration_timestamp: Option<i64>,
    pub(crate) consignment_path: Option<String>,
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
    /// Change UTXO of an outgoing transfer
    pub change_utxo: Option<Outpoint>,
    /// Expiration of the transfer
    pub expiration_timestamp: Option<u64>,
    /// Transport endpoints for the transfer
    pub transport_endpoints: Vec<TransferTransportEndpoint>,
    /// Invoice string of the incoming transfer
    pub invoice_string: Option<String>,
    /// Consignment path
    pub consignment_path: Option<String>,
}

impl DbTransfer {
    pub(crate) fn to_transfer(
        &self,
        td: TransferData,
        transport_endpoints: Vec<TransferTransportEndpoint>,
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
            change_utxo: td.change_utxo,
            expiration_timestamp: td.expiration_timestamp.map(|t| t as u64),
            transport_endpoints,
            invoice_string: self.invoice_string.clone(),
            consignment_path: td.consignment_path,
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
    pub expiration_timestamp: Option<u64>,
    /// Batch transfer idx
    pub batch_transfer_idx: i32,
}

#[derive(Debug, Clone)]
pub struct ReceiveDataInternal {
    pub(crate) asset_id: Option<String>,
    pub(crate) detected_assignment: Assignment,
    pub(crate) invoice_string: String,
    pub(crate) recipient_id: String,
    pub(crate) endpoints: Vec<String>,
    pub(crate) created_at: i64,
    pub(crate) expiration_timestamp: Option<i64>,
    pub(crate) recipient_type_full: RecipientTypeFull,
    pub(crate) blind_seal: Option<GraphSeal>,
    pub(crate) script_pubkey: Option<ScriptBuf>,
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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum TransactionType {
    /// Transaction used to perform an RGB send
    RgbSend,
    /// Transaction used to drain the RGB wallet
    Drain,
    /// Transaction used to create UTXOs
    CreateUtxos,
    /// Transaction not created by rgb-lib directly
    User,
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

/// Information about an RGB input being spent.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbInputInfo {
    /// The input index in the transaction
    pub vin: u32,
    /// The assignment in input
    pub assignment: Assignment,
}

/// Information about an RGB output destination.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbOutputInfo {
    /// The output vout (if allocated to a witness output)
    pub vout: Option<u32>,
    /// The assignment in output
    pub assignment: Assignment,
    /// Whether this output is allocated to a concealed seal
    pub is_concealed: bool,
    /// Whether this output belongs to our wallet
    pub is_ours: bool,
}

/// Information about an RGB transition.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbTransitionInfo {
    /// The transition type
    pub r#type: TypeOfTransition,
    /// Details of the transition inputs
    pub inputs: Vec<RgbInputInfo>,
    /// Details of the transition outputs
    pub outputs: Vec<RgbOutputInfo>,
}

/// Information about an RGB operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbOperationInfo {
    /// ID of the asset being operated on
    pub asset_id: String,
    /// Transitions
    pub transitions: Vec<RgbTransitionInfo>,
}

/// Result of inspecting RGB consignments against a PSBT.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbInspection {
    /// The close method used for the operation
    pub close_method: CloseMethod,
    /// The RGB commitment in hex encoded format
    pub commitment_hex: String,
    /// Details of each operation
    pub operations: Vec<RgbOperationInfo>,
}

// ────────────────────────────────────────────────────────────
// Send, inflate & refresh operations
// ────────────────────────────────────────────────────────────

/// The result of an inflate begin operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct InflateBeginResult {
    /// PSBT to inspect and sign
    pub psbt: String,
    /// Operation details
    pub details: InflateDetails,
}

/// Details for inflate operations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct InflateDetails {
    /// Path to fascia file for inspection
    pub fascia_path: String,
    /// Minimum confirmations for the operation
    pub min_confirmations: u8,
    /// Entropy used for the merkle tree construction operation
    pub entropy: u64,
}

/// The result of a send begin operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct SendBeginResult {
    /// PSBT to inspect and sign
    pub psbt: String,
    /// Operation details
    pub details: SendDetails,
}

/// Details for send operations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct SendDetails {
    /// Path to fascia file for inspection
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
#[cfg(any(feature = "electrum", feature = "esplora"))]
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
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub enum RefreshTransferStatus {
    /// Waiting for the counterparty to take action
    WaitingCounterparty = 1,
    /// Waiting for the transfer transaction to reach the minimum number of confirmations
    WaitingConfirmations = 2,
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl TryFrom<TransferStatus> for RefreshTransferStatus {
    type Error = &'static str;

    fn try_from(x: TransferStatus) -> Result<Self, Self::Error> {
        match x {
            TransferStatus::WaitingCounterparty => Ok(RefreshTransferStatus::WaitingCounterparty),
            TransferStatus::WaitingConfirmations => Ok(RefreshTransferStatus::WaitingConfirmations),
            _ => Err("ResfreshStatus only accepts pending statuses"),
        }
    }
}

/// A transfer refresh filter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RefreshFilter {
    /// Transfer status
    pub status: RefreshTransferStatus,
    /// Whether the transfer is incoming
    pub incoming: bool,
}

/// A refreshed transfer
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RefreshedTransfer {
    /// The updated transfer status, if it has changed
    pub updated_status: Option<TransferStatus>,
    /// Optional failure
    pub failure: Option<Error>,
}

/// The result of a refresh operation
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub type RefreshResult = HashMap<i32, RefreshedTransfer>;

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) trait RefreshResultTrait {
    fn transfers_changed(&self) -> bool;
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
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

impl LocalRecipientData {
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn vout(&self) -> Option<u32> {
        match &self {
            LocalRecipientData::Blind(_) => None,
            LocalRecipientData::Witness(d) => Some(d.vout),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalRecipient {
    pub recipient_id: String,
    pub local_recipient_data: LocalRecipientData,
    pub assignment: Assignment,
    pub transport_endpoints: Vec<LocalTransportEndpoint>,
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
    pub reject_list_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BtcChange {
    pub vout: u32,
    pub amount: u64,
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
    pub main_transition: TypeOfTransition,
    pub beneficiaries_blinded: Vec<SecretSeal>,
    pub beneficiaries_witness: Vec<ExplicitSeal<RgbTxid>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InfoBatchTransfer {
    pub btc_change: Option<BtcChange>,
    pub change_utxo_outpoint: Option<Outpoint>,
    pub extra_allocations: HashMap<String, HashMap<OutPoint, Vec<Assignment>>>,
    pub donation: bool,
    pub min_confirmations: u8,
    pub expiration_timestamp: Option<i64>,
    pub created_at: i64,
    pub entropy: u64,
    pub transfers: BTreeMap<String, InfoAssetTransfer>,
}

pub type TransferEndData = (String, PathBuf, InfoBatchTransfer, Fascia);

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub struct BeginOperationData {
    pub psbt: Psbt,
    pub transfer_dir: PathBuf,
    pub info_batch_transfer: InfoBatchTransfer,
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub enum PrepareRgbPsbtResult {
    Retry,
    Success(Box<BeginOperationData>),
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub enum PrepareTransferPsbtResult {
    Retry,
    Success(Box<BeginOperationData>),
}
