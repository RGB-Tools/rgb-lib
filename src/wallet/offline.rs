//! RGB wallet
//!
//! This module defines the offline methods of the [`Wallet`] structure and all its related data.

use super::*;

pub(crate) const RGB_LIB_DB_NAME: &str = "rgb_lib_db";
const BDK_DB_NAME: &str = "bdk_db";

pub(crate) const MEDIA_DIR: &str = "media_files";
const TRANSFERS_DIR: &str = "transfers";

const MIN_BTC_REQUIRED: u64 = 2000;

pub(crate) const NUM_KNOWN_SCHEMAS: usize = 4;

pub(crate) const UDA_FIXED_INDEX: u32 = 0;

pub(crate) const MAX_ATTACHMENTS: usize = 20;

pub(crate) const MAX_TRANSPORT_ENDPOINTS: usize = 3;

pub(crate) const DURATION_RCV_TRANSFER: u32 = 86400;

pub(crate) const ASSET_ID_PREFIX: &str = "rgb:";
pub(crate) const CONSIGNMENT_FILE: &str = "consignment_out";

pub(crate) const SCHEMA_ID_NIA: &str =
    "rgb:sch:RWhwUfTMpuP2Zfx1~j4nswCANGeJrYOqDcKelaMV4zU#remote-digital-pegasus";
pub(crate) const SCHEMA_ID_UDA: &str =
    "rgb:sch:~6rjymf3GTE840lb5JoXm2aFwE8eWCk3mCjOf_mUztE#spider-montana-fantasy";
pub(crate) const SCHEMA_ID_CFA: &str =
    "rgb:sch:JgqK5hJX9YBT4osCV7VcW_iLTcA5csUCnLzvaKTTrNY#mars-house-friend";
pub(crate) const SCHEMA_ID_IFA: &str =
    "rgb:sch:boBJfIhHYmFRFveNF5QvmyvgDVh3T5Gicqg6A~_czfY#virgo-koala-fire";

/// The bitcoin balances (in sats) for the vanilla and colored wallets.
///
/// The settled balances include the confirmed balance.
/// The future balances also include the immature balance and the untrusted and trusted pending
/// balances.
/// The spendable balances include the settled balance and also the untrusted and trusted pending
/// balances.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct BtcBalance {
    /// Funds that will never hold RGB assets
    pub vanilla: Balance,
    /// Funds that may hold RGB assets
    pub colored: Balance,
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

/// Metadata of an RGB asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Metadata {
    /// Asset schema type
    pub asset_schema: AssetSchema,
    /// Total issued amount
    pub issued_supply: u64,
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
}

/// A Non-Inflatable Asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
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
        wallet: &Wallet,
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
                wallet.database.iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.get_media_dir()))
        };
        let balance = wallet.database.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let issued_supply = asset.issued_supply.parse::<u64>().unwrap();
        Ok(AssetNIA {
            asset_id: asset.id.clone(),
            ticker: asset.ticker.clone().unwrap(),
            name: asset.name.clone(),
            details: asset.details.clone(),
            precision: asset.precision,
            issued_supply,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            media,
        })
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
    /// Total issued amount
    pub issued_supply: u64,
    /// Timestamp of asset genesis
    pub timestamp: i64,
    /// Timestamp of asset import
    pub added_at: i64,
    /// Current balance of the asset
    pub balance: Balance,
    /// Asset unique token
    pub token: Option<TokenLight>,
}

impl AssetUDA {
    pub(crate) fn get_asset_details(
        wallet: &Wallet,
        asset: &DbAsset,
        token: Option<TokenLight>,
        transfers: Option<Vec<DbTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
    ) -> Result<AssetUDA, Error> {
        let balance = wallet.database.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let issued_supply = asset.issued_supply.parse::<u64>().unwrap();
        Ok(AssetUDA {
            asset_id: asset.id.clone(),
            details: asset.details.clone(),
            ticker: asset.ticker.clone().unwrap(),
            name: asset.name.clone(),
            precision: asset.precision,
            issued_supply,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            token,
        })
    }
}

/// A Collectible Fungible Asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
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
        wallet: &Wallet,
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
                wallet.database.iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.get_media_dir()))
        };
        let balance = wallet.database.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let issued_supply = asset.issued_supply.parse::<u64>().unwrap();
        Ok(AssetCFA {
            asset_id: asset.id.clone(),
            name: asset.name.clone(),
            details: asset.details.clone(),
            precision: asset.precision,
            issued_supply,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            media,
        })
    }
}

/// An Inflatable Fungible Asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
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

impl AssetIFA {
    pub(crate) fn get_asset_details(
        wallet: &Wallet,
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
                wallet.database.iter_media()?
            };
            medias
                .iter()
                .find(|m| Some(m.idx) == asset.media_idx)
                .map(|m| Media::from_db_media(m, wallet.get_media_dir()))
        };
        let balance = wallet.database.get_asset_balance(
            asset.id.clone(),
            transfers,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let issued_supply = asset.issued_supply.parse::<u64>().unwrap();
        Ok(AssetIFA {
            asset_id: asset.id.clone(),
            ticker: asset.ticker.clone().unwrap(),
            name: asset.name.clone(),
            details: asset.details.clone(),
            precision: asset.precision,
            issued_supply,
            timestamp: asset.timestamp,
            added_at: asset.added_at,
            balance,
            media,
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

/// Data to receive an RGB transfer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct ReceiveData {
    /// Invoice string
    pub invoice: String,
    /// ID of the receive operation (blinded UTXO or Bitcoin script)
    pub recipient_id: String,
    /// Expiration of the receive operation
    pub expiration_timestamp: Option<i64>,
    /// Batch transfer idx
    pub batch_transfer_idx: i32,
}

/// The type of an RGB recipient
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum RecipientType {
    /// Receive via blinded UTXO
    Blind,
    /// Receive via witness TX
    Witness,
}

/// RGB recipient information used to be paid
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// An RGB transport endpoint.
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// Supported database types.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum DatabaseType {
    /// A SQLite database
    Sqlite,
}

/// A bitcoin address.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Deserialize, Serialize)]
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
            Some(<AssetSchema as std::convert::TryFrom<_>>::try_from(
                schema_id,
            )?)
        } else {
            None
        };
        let assignment_name = decoded.assignment_name.map(|a| a.to_string());
        let assignment = match asset_schema {
            None => match (decoded.assignment_state, assignment_name.as_deref()) {
                (Some(InvoiceState::Amount(v)), Some("assetOwner")) => {
                    Assignment::Fungible(v.value())
                }
                (Some(InvoiceState::Amount(v)), Some("inflationAllowance")) => {
                    Assignment::InflationRight(v.value())
                }
                (Some(InvoiceState::Amount(_)), _) => Assignment::Any,
                (Some(InvoiceState::Data(_)), Some("assetOwner") | None) => Assignment::NonFungible,
                (Some(InvoiceState::Void), Some("replaceRight") | None) => Assignment::ReplaceRight,
                (None, None) => Assignment::Any,
                (_, _) => {
                    return Err(Error::InvalidInvoice {
                        details: s!("unsupported assignment"),
                    });
                }
            },
            Some(AssetSchema::Nia) | Some(AssetSchema::Cfa) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Amount(v)), Some("assetOwner") | None) => {
                        Assignment::Fungible(v.value())
                    }
                    (None, Some("assetOwner") | None) => Assignment::Fungible(0),
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
            Some(AssetSchema::Uda) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Data(_)) | None, Some("assetOwner") | None) => {
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
                    (Some(InvoiceState::Amount(v)), Some("assetOwner")) => {
                        Assignment::Fungible(v.value())
                    }
                    (None, Some("assetOwner")) => Assignment::Fungible(0),
                    (Some(InvoiceState::Amount(v)), Some("inflationAllowance")) => {
                        Assignment::InflationRight(v.value())
                    }
                    (None, Some("inflationAllowance")) => Assignment::InflationRight(0),
                    (Some(InvoiceState::Amount(_)), None) => Assignment::Any,
                    (Some(InvoiceState::Void), Some("replaceRight") | None) => {
                        Assignment::ReplaceRight
                    }
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

        let invoice_data = InvoiceData {
            recipient_id,
            asset_schema,
            asset_id,
            assignment,
            assignment_name,
            expiration_timestamp: decoded.expiry,
            transport_endpoints,
            network,
        };

        Ok(Invoice {
            invoice_string,
            invoice_data,
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
    pub expiration_timestamp: Option<i64>,
    /// Transport endpoints
    pub transport_endpoints: Vec<String>,
}

/// Data for operations that require the wallet to be online.
///
/// Methods not requiring an `Online` object don't need network access and can be performed
/// offline. Methods taking an optional `Online` will operate offline when it's missing and will
/// use local data only.
///
/// <div class="warning">This should not be manually constructed but should be obtained from the
/// [`Wallet::go_online`] method.</div>
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Online {
    /// Unique ID for this object
    pub id: u64,
    /// URL of the indexer server to be used for online operations
    pub indexer_url: String,
}

/// Bitcoin transaction outpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
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

impl From<RgbOutpoint> for Outpoint {
    fn from(x: RgbOutpoint) -> Outpoint {
        Outpoint {
            txid: x.txid.to_string(),
            vout: x.vout.into_u32(),
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

impl From<DbTxo> for RgbOutpoint {
    fn from(x: DbTxo) -> RgbOutpoint {
        RgbOutpoint::new(RgbTxid::from_str(&x.txid).unwrap(), x.vout)
    }
}

impl From<Outpoint> for RgbOutpoint {
    fn from(x: Outpoint) -> RgbOutpoint {
        RgbOutpoint::new(RgbTxid::from_str(&x.txid).unwrap(), x.vout)
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

/// An RGB allocation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
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

/// A Bitcoin transaction.
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// Block height and timestamp of a block.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockTime {
    /// Confirmation block height
    pub height: u32,
    /// Confirmation block timestamp
    pub timestamp: u64,
}

/// The type of a transaction.
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// An RGB transfer.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
    pub expiration: Option<i64>,
    /// Transport endpoints for the transfer
    pub transport_endpoints: Vec<TransferTransportEndpoint>,
    /// Invoice string of the incoming transfer
    pub invoice_string: Option<String>,
}

impl Transfer {
    fn from_db_transfer(
        x: &DbTransfer,
        td: TransferData,
        transport_endpoints: Vec<TransferTransportEndpoint>,
    ) -> Transfer {
        Transfer {
            idx: x.idx,
            batch_transfer_idx: td.batch_transfer_idx,
            created_at: td.created_at,
            updated_at: td.updated_at,
            status: td.status,
            requested_assignment: x.requested_assignment.clone(),
            assignments: td.assignments,
            kind: td.kind,
            txid: td.txid,
            recipient_id: x.recipient_id.clone(),
            receive_utxo: td.receive_utxo,
            change_utxo: td.change_utxo,
            expiration: td.expiration,
            transport_endpoints,
            invoice_string: x.invoice_string.clone(),
        }
    }
}

/// An RGB transport endpoint for a transfer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct TransferTransportEndpoint {
    /// Endpoint address
    pub endpoint: String,
    /// Endpoint transport type
    pub transport_type: TransportType,
    /// Whether the endpoint has been used
    pub used: bool,
}

impl TransferTransportEndpoint {
    fn from_db_transfer_transport_endpoint(
        x: &DbTransferTransportEndpoint,
        ce: &DbTransportEndpoint,
    ) -> TransferTransportEndpoint {
        TransferTransportEndpoint {
            endpoint: ce.endpoint.clone(),
            transport_type: ce.transport_type,
            used: x.used,
        }
    }
}

/// The type of an RGB transfer.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum TransferKind {
    /// A transfer that issued the asset
    Issuance,
    /// An incoming transfer via blinded UTXO
    ReceiveBlind,
    /// An incoming transfer via a Bitcoin script (witness TX)
    ReceiveWitness,
    /// An outgoing transfer
    Send,
}

/// A wallet unspent.
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// A Bitcoin unspent transaction output.
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// Data that defines a [`Wallet`].
#[derive(Clone, Debug, Deserialize, Serialize)]
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
    /// Wallet account-level xPub for the vanilla-side of the wallet
    pub account_xpub_vanilla: String,
    /// Wallet account-level xPub for the colored-side of the wallet
    pub account_xpub_colored: String,
    /// Wallet mnemonic phrase
    pub mnemonic: Option<String>,
    /// Wallet master fingerprint
    pub master_fingerprint: String,
    /// Keychain index for the vanilla-side of the wallet (default: 0)
    #[serde(deserialize_with = "from_str_or_number_optional")]
    pub vanilla_keychain: Option<u8>,
    /// List of schemas the wallet should support (when issuing, sending and receiving). Empty list
    /// means the wallet should support all the schemas rgb-lib supports.
    pub supported_schemas: Vec<AssetSchema>,
}

/// An RGB wallet.
///
/// This should not be manually constructed but should be obtained from the [`Wallet::new`]
/// method.
pub struct Wallet {
    pub(crate) wallet_data: WalletData,
    pub(crate) logger: Logger,
    pub(crate) _logger_guard: AsyncGuard,
    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) watch_only: bool,
    pub(crate) database: Arc<RgbLibDatabase>,
    pub(crate) wallet_dir: PathBuf,
    pub(crate) bdk_wallet: PersistedWallet<Store<ChangeSet>>,
    pub(crate) bdk_database: Store<ChangeSet>,
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) rest_client: RestClient,
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) online_data: Option<OnlineData>,
}

impl Wallet {
    /// Create a new RGB wallet based on the provided [`WalletData`].
    pub fn new(wallet_data: WalletData) -> Result<Self, Error> {
        let wdata = wallet_data.clone();

        // wallet account xPubs
        let bdk_network = BdkNetwork::from(wdata.bitcoin_network);
        let xpub_rgb = str_to_xpub(&wdata.account_xpub_colored, bdk_network)?;
        let xpub_btc = str_to_xpub(&wdata.account_xpub_vanilla, bdk_network)?;

        // wallet directory and file logging setup
        let data_dir_path = Path::new(&wdata.data_dir);
        if !data_dir_path.exists() {
            return Err(Error::InexistentDataDir);
        }
        let data_dir_path = fs::canonicalize(data_dir_path)?;
        if let Some(mnemonic) = &wdata.mnemonic {
            // check master fingerprint derived from mnemonic matches provided one
            let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)?;
            let master_xprv =
                Xpriv::new_master(wdata.bitcoin_network, &mnemonic.to_seed("")).unwrap();
            let master_xpub = Xpub::from_priv(&Secp256k1::new(), &master_xprv);
            let master_fingerprint = master_xpub.fingerprint();
            if master_fingerprint
                != Fingerprint::from_str(&wdata.master_fingerprint)
                    .map_err(|_| Error::InvalidFingerprint)?
            {
                return Err(Error::FingerprintMismatch);
            }
        }
        let wallet_dir = data_dir_path.join(&wdata.master_fingerprint);
        if !wallet_dir.exists() {
            fs::create_dir(&wallet_dir)?;
            fs::create_dir(wallet_dir.join(MEDIA_DIR))?;
        }
        let (logger, _logger_guard) = setup_logger(&wallet_dir, None)?;
        info!(logger.clone(), "New wallet in '{:?}'", wallet_dir);
        let panic_logger = logger.clone();
        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            error!(panic_logger.clone(), "PANIC: {:?}", info);
            prev_hook(info);
        }));

        // BDK setup
        let (desc_colored, desc_vanilla, watch_only) = if let Some(mnemonic) = wdata.mnemonic {
            let (desc_colored, desc_vanilla) = get_descriptors(
                wdata.bitcoin_network,
                &mnemonic,
                wdata.vanilla_keychain,
                xpub_btc,
                xpub_rgb,
            )?;
            (desc_colored, desc_vanilla, false)
        } else {
            let (desc_colored, desc_vanilla) = get_descriptors_from_xpubs(
                wdata.bitcoin_network,
                &wdata.master_fingerprint,
                xpub_rgb,
                xpub_btc,
                wdata.vanilla_keychain,
            )?;
            (desc_colored, desc_vanilla, true)
        };
        let mut wallet_params = BdkWallet::load()
            .descriptor(KeychainKind::External, Some(desc_colored.clone()))
            .descriptor(KeychainKind::Internal, Some(desc_vanilla.clone()))
            .check_genesis_hash(
                BlockHash::from_str(get_genesis_hash(&wdata.bitcoin_network)).unwrap(),
            );
        let bdk_db_name = if watch_only {
            format!("{BDK_DB_NAME}_watch_only")
        } else {
            wallet_params = wallet_params.extract_keys();
            BDK_DB_NAME.to_string()
        };
        let bdk_db_path = wallet_dir.join(bdk_db_name);
        let (mut bdk_database, _) =
            Store::<ChangeSet>::load_or_create(BDK_DB_NAME.as_bytes(), bdk_db_path)?;
        let bdk_wallet = match wallet_params.load_wallet(&mut bdk_database)? {
            Some(wallet) => wallet,
            None => BdkWallet::create(desc_colored, desc_vanilla)
                .network(bdk_network)
                .create_wallet(&mut bdk_database)?,
        };

        // RGB setup
        let supported_schemas = wdata.supported_schemas;
        if supported_schemas.is_empty() {
            return Err(Error::NoSupportedSchemas);
        }
        if wdata.bitcoin_network == BitcoinNetwork::Mainnet
            && supported_schemas.contains(&AssetSchema::Ifa)
        {
            return Err(Error::CannotUseIfaOnMainnet);
        }
        let mut runtime = load_rgb_runtime(wallet_dir.clone())?;
        let known_schemas = runtime.schemata()?;
        if known_schemas.len() < NUM_KNOWN_SCHEMAS {
            let known: HashSet<_> = known_schemas.iter().map(|s| s.id).collect();
            for schema in supported_schemas {
                if !known.contains(&SchemaId::from(schema)) {
                    schema.import_kit(&mut runtime)?;
                }
            }
        }

        // RGB-LIB setup
        let db_path = wallet_dir.join(RGB_LIB_DB_NAME);
        let display_db_path = adjust_canonicalization(db_path);
        let connection_string = format!("sqlite:{display_db_path}?mode=rwc");
        let mut opt = ConnectOptions::new(connection_string);
        opt.max_connections(1)
            .min_connections(0)
            .connect_timeout(Duration::from_secs(8))
            .idle_timeout(Duration::from_secs(8))
            .max_lifetime(Duration::from_secs(8));
        let db_cnn = block_on(Database::connect(opt));
        let connection = db_cnn.map_err(InternalError::from)?;
        block_on(Migrator::up(&connection, None)).map_err(InternalError::from)?;
        let database = RgbLibDatabase::new(connection);
        #[cfg(any(feature = "electrum", feature = "esplora"))]
        let rest_client = get_proxy_client()?;

        info!(logger, "New wallet completed");
        Ok(Wallet {
            wallet_data,
            logger,
            _logger_guard,
            watch_only,
            database: Arc::new(database),
            wallet_dir,
            bdk_wallet,
            bdk_database,
            #[cfg(any(feature = "electrum", feature = "esplora"))]
            rest_client,
            #[cfg(any(feature = "electrum", feature = "esplora"))]
            online_data: None,
        })
    }

    pub(crate) fn bitcoin_network(&self) -> BitcoinNetwork {
        self.wallet_data.bitcoin_network
    }

    pub(crate) fn chain_net(&self) -> ChainNet {
        self.bitcoin_network().into()
    }

    pub(crate) fn rgb_runtime(&self) -> Result<RgbRuntime, Error> {
        load_rgb_runtime(self.wallet_dir.clone())
    }

    /// Return the data that defines the wallet.
    pub fn get_wallet_data(&self) -> WalletData {
        self.wallet_data.clone()
    }

    /// Return the wallet directory.
    pub fn get_wallet_dir(&self) -> PathBuf {
        self.wallet_dir.clone()
    }

    /// Return the media directory.
    pub fn get_media_dir(&self) -> PathBuf {
        self.wallet_dir.join(MEDIA_DIR)
    }

    pub(crate) fn get_transfers_dir(&self) -> PathBuf {
        self.wallet_dir.join(TRANSFERS_DIR)
    }

    pub(crate) fn max_allocations_per_utxo(&self) -> u32 {
        self.wallet_data.max_allocations_per_utxo
    }

    pub(crate) fn supports_schema(&self, asset_schema: &AssetSchema) -> bool {
        self.wallet_data.supported_schemas.contains(asset_schema)
    }

    pub(crate) fn check_schema_support(&self, asset_schema: &AssetSchema) -> Result<(), Error> {
        if !self.supports_schema(asset_schema) {
            return Err(Error::UnsupportedSchema {
                asset_schema: *asset_schema,
            });
        }
        Ok(())
    }

    pub(crate) fn check_transport_endpoints(
        &self,
        transport_endpoints: &[String],
    ) -> Result<(), Error> {
        if transport_endpoints.is_empty() {
            return Err(Error::InvalidTransportEndpoints {
                details: s!("must provide at least a transport endpoint"),
            });
        }
        if transport_endpoints.len() > MAX_TRANSPORT_ENDPOINTS {
            return Err(Error::InvalidTransportEndpoints {
                details: format!(
                    "library supports at max {MAX_TRANSPORT_ENDPOINTS} transport endpoints"
                ),
            });
        }

        Ok(())
    }

    pub(crate) fn filter_unspents(
        &self,
        keychain: KeychainKind,
    ) -> impl Iterator<Item = LocalOutput> + '_ {
        self.bdk_wallet
            .list_unspent()
            .filter(move |u| u.keychain == keychain)
    }

    pub(crate) fn internal_unspents(&self) -> impl Iterator<Item = LocalOutput> + '_ {
        self.filter_unspents(KeychainKind::Internal)
    }

    pub(crate) fn get_uncolorable_btc_sum(&self) -> Result<u64, Error> {
        Ok(self
            .internal_unspents()
            .map(|u| u.txout.value.to_sat())
            .sum())
    }

    pub(crate) fn get_available_allocations<T>(
        &self,
        unspents: T,
        exclude_utxos: &[Outpoint],
        max_allocations: Option<u32>,
    ) -> Result<Vec<LocalUnspent>, Error>
    where
        T: Into<Vec<LocalUnspent>>,
    {
        let mut mut_unspents = unspents.into();
        mut_unspents
            .iter_mut()
            .for_each(|u| u.rgb_allocations.retain(|a| !a.status.failed()));
        let max_allocs = max_allocations.unwrap_or(self.max_allocations_per_utxo() - 1);
        Ok(mut_unspents
            .iter()
            .filter(|u| u.utxo.exists)
            .filter(|u| !u.utxo.pending_witness)
            .filter(|u| !exclude_utxos.contains(&u.utxo.outpoint()))
            .filter(|u| {
                (u.rgb_allocations.len() as u32) + u.pending_blinded <= max_allocs
                    && !u
                        .rgb_allocations
                        .iter()
                        .any(|a| !a.incoming && a.status.waiting_counterparty())
            })
            .cloned()
            .collect())
    }

    pub(crate) fn detect_btc_unspendable_err(&self) -> Result<Error, Error> {
        let available = self.get_uncolorable_btc_sum()?;
        Ok(if available < MIN_BTC_REQUIRED {
            Error::InsufficientBitcoins {
                needed: MIN_BTC_REQUIRED,
                available,
            }
        } else {
            Error::InsufficientAllocationSlots
        })
    }

    pub(crate) fn get_utxo(
        &self,
        exclude_utxos: &[Outpoint],
        unspents: Option<&[LocalUnspent]>,
        pending_operation: bool,
        max_allocations: Option<u32>,
    ) -> Result<DbTxo, Error> {
        let rgb_allocations = if unspents.is_none() {
            let unspent_txos = self.database.get_unspent_txos(vec![])?;
            Some(
                self.database
                    .get_rgb_allocations(unspent_txos, None, None, None, None)?,
            )
        } else {
            None
        };
        let unspents: &[LocalUnspent] = match unspents {
            Some(u) => u,
            None => rgb_allocations.as_deref().unwrap(),
        };

        let mut allocatable =
            self.get_available_allocations(unspents, exclude_utxos, max_allocations)?;
        allocatable.sort_by_key(|t| t.rgb_allocations.len() + t.pending_blinded as usize);
        match allocatable.first() {
            Some(mut selected) => {
                if allocatable.len() > 1 && !selected.rgb_allocations.is_empty() {
                    let filtered_allocatable: Vec<&LocalUnspent> = if pending_operation {
                        allocatable
                            .iter()
                            .filter(|t| t.rgb_allocations.iter().any(|a| a.future()))
                            .collect()
                    } else {
                        allocatable
                            .iter()
                            .filter(|t| t.rgb_allocations.iter().all(|a| !a.future()))
                            .collect()
                    };
                    if let Some(other) = filtered_allocatable.first() {
                        selected = other;
                    }
                }
                Ok(selected.clone().utxo)
            }
            None => Err(self.detect_btc_unspendable_err()?),
        }
    }

    pub(crate) fn save_transfer_transport_endpoint(
        &self,
        transfer_idx: i32,
        transport_endpoint: &LocalTransportEndpoint,
    ) -> Result<(), Error> {
        let transport_endpoint_idx = match self
            .database
            .get_transport_endpoint(transport_endpoint.endpoint.clone())?
        {
            Some(ce) => ce.idx,
            None => self
                .database
                .set_transport_endpoint(DbTransportEndpointActMod {
                    transport_type: ActiveValue::Set(transport_endpoint.transport_type),
                    endpoint: ActiveValue::Set(transport_endpoint.endpoint.clone()),
                    ..Default::default()
                })?,
        };

        self.database
            .set_transfer_transport_endpoint(DbTransferTransportEndpointActMod {
                transfer_idx: ActiveValue::Set(transfer_idx),
                transport_endpoint_idx: ActiveValue::Set(transport_endpoint_idx),
                used: ActiveValue::Set(transport_endpoint.used),
                ..Default::default()
            })?;

        Ok(())
    }

    pub(crate) fn check_details(&self, details: String) -> Result<Details, Error> {
        if details.is_empty() {
            return Err(Error::InvalidDetails {
                details: s!("ident must contain at least one character"),
            });
        }
        Details::from_str(&details).map_err(|e| Error::InvalidDetails {
            details: e.to_string(),
        })
    }

    fn _check_name(&self, name: String) -> Result<Name, Error> {
        Name::try_from(name).map_err(|e| Error::InvalidName {
            details: e.to_string(),
        })
    }

    fn _check_precision(&self, precision: u8) -> Result<Precision, Error> {
        Precision::try_from(precision).map_err(|_| Error::InvalidPrecision {
            details: s!("precision is too high"),
        })
    }

    fn _check_ticker(&self, ticker: String) -> Result<Ticker, Error> {
        if ticker.to_ascii_uppercase() != *ticker {
            return Err(Error::InvalidTicker {
                details: s!("ticker needs to be all uppercase"),
            });
        }
        Ticker::try_from(ticker).map_err(|e| Error::InvalidTicker {
            details: e.to_string(),
        })
    }

    fn _get_total_issue_amount(&self, amounts: &[u64]) -> Result<u64, Error> {
        if amounts.is_empty() {
            return Err(Error::NoIssuanceAmounts);
        }
        amounts.iter().try_fold(0u64, |acc, x| {
            acc.checked_add(*x).ok_or(Error::TooHighIssuanceAmounts)
        })
    }

    fn _get_total_inflation_amount(
        &self,
        inflation_amounts: &[u64],
        issued_supply: u64,
    ) -> Result<u64, Error> {
        if inflation_amounts.is_empty() {
            return Ok(0);
        }
        let total_inflation = inflation_amounts.iter().try_fold(0u64, |acc, x| {
            acc.checked_add(*x).ok_or(Error::TooHighInflationAmounts)
        })?;
        issued_supply
            .checked_add(total_inflation)
            .ok_or(Error::TooHighInflationAmounts)?;

        Ok(total_inflation)
    }

    fn _file_details<P: AsRef<Path>>(
        &self,
        original_file_path: P,
    ) -> Result<(Attachment, Media), Error> {
        if !original_file_path.as_ref().exists() {
            return Err(Error::InvalidFilePath {
                file_path: original_file_path.as_ref().to_string_lossy().to_string(),
            });
        }
        let file_bytes = fs::read(&original_file_path)?;
        if file_bytes.is_empty() {
            return Err(Error::EmptyFile {
                file_path: original_file_path.as_ref().to_string_lossy().to_string(),
            });
        }
        let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
        let digest_bytes = file_hash.to_byte_array();
        let mime = FileFormat::from_file(original_file_path.as_ref())?
            .media_type()
            .to_string();
        let media_ty: &'static str = Box::leak(mime.clone().into_boxed_str());
        let media_type = MediaType::with(media_ty);
        let digest = file_hash.to_string();
        let file_path = self
            .get_media_dir()
            .join(&digest)
            .to_string_lossy()
            .to_string();
        Ok((
            Attachment {
                ty: media_type,
                digest: digest_bytes.into(),
            },
            Media {
                digest,
                mime,
                file_path,
            },
        ))
    }

    pub(crate) fn copy_media_and_save<P: AsRef<Path>>(
        &self,
        original_file_path: P,
        media: &Media,
    ) -> Result<i32, Error> {
        let src = original_file_path.as_ref().to_string_lossy().to_string();
        let dst = media.clone().file_path;
        if src != dst {
            fs::copy(src, dst)?;
        }
        self.get_or_insert_media(media.get_digest(), media.mime.clone())
    }

    pub(crate) fn new_asset_terms(
        &self,
        text: RicardianContract,
        media: Option<Attachment>,
    ) -> ContractTerms {
        ContractTerms { text, media }
    }

    pub(crate) fn get_blind_seal(&self, outpoint: impl Into<RgbOutpoint>) -> BlindSeal<RgbTxid> {
        let outpoint = outpoint.into();
        BlindSeal::new_random(outpoint.txid, outpoint.vout)
    }

    pub(crate) fn get_builder_seal(
        &self,
        outpoint: impl Into<RgbOutpoint>,
    ) -> BuilderSeal<BlindSeal<RgbTxid>> {
        BuilderSeal::from(self.get_blind_seal(outpoint))
    }

    /// Issue a new RGB NIA asset with the provided `ticker`, `name`, `precision` and `amounts`,
    /// then return it.
    ///
    /// At least 1 amount needs to be provided and the sum of all amounts cannot exceed the maximum
    /// `u64` value.
    ///
    /// If `amounts` contains more than 1 element, each one will be issued as a separate allocation
    /// for the same asset (on a separate UTXO that needs to be already available).
    pub fn issue_asset_nia(
        &self,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetNIA, Error> {
        info!(
            self.logger,
            "Issuing NIA asset with ticker '{}' name '{}' precision '{}' amounts '{:?}'...",
            ticker,
            name,
            precision,
            amounts
        );

        let asset_schema = &AssetSchema::Nia;

        self.check_schema_support(asset_schema)?;

        let settled = self._get_total_issue_amount(&amounts)?;

        let db_data = self.database.get_db_data(false)?;

        let mut unspents: Vec<LocalUnspent> = self.database.get_rgb_allocations(
            self.database.get_unspent_txos(db_data.txos.clone())?,
            None,
            None,
            None,
            None,
        )?;
        unspents.retain(|u| {
            !(u.rgb_allocations
                .iter()
                .any(|a| !a.incoming && a.status.waiting_counterparty()))
        });

        let created_at = now().unix_timestamp();
        let text = RicardianContract::default();
        #[cfg(test)]
        let terms = mock_asset_terms(self, text, None);
        #[cfg(not(test))]
        let terms = self.new_asset_terms(text, None);
        #[cfg(test)]
        let details = mock_contract_details(self);
        #[cfg(not(test))]
        let details = None;
        let spec = AssetSpec {
            ticker: self._check_ticker(ticker.clone())?,
            name: self._check_name(name.clone())?,
            details,
            precision: self._check_precision(precision)?,
        };

        let mut runtime = self.rgb_runtime()?;
        let mut builder = ContractBuilder::with(
            Identity::default(),
            NonInflatableAsset::schema(),
            NonInflatableAsset::types(),
            NonInflatableAsset::scripts(),
            self.chain_net(),
        )
        .add_global_state("spec", spec.clone())
        .expect("invalid spec")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_global_state("issuedSupply", Amount::from(settled))
        .expect("invalid issuedSupply");

        let mut issue_utxos: HashMap<DbTxo, u64> = HashMap::new();
        let mut exclude_outpoints = vec![];
        for amount in &amounts {
            let utxo = self.get_utxo(&exclude_outpoints, Some(&unspents), false, None)?;
            exclude_outpoints.push(utxo.outpoint());
            issue_utxos.insert(utxo.clone(), *amount);

            builder = builder
                .add_fungible_state("assetOwner", self.get_builder_seal(utxo), *amount)
                .expect("invalid global state data");
        }
        debug!(self.logger, "Issuing on UTXOs: {issue_utxos:?}");

        let validated_contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = validated_contract.contract_id().to_string();
        runtime
            .import_contract(validated_contract, &DumbResolver)
            .expect("failure importing issued contract");

        let asset = self.add_asset_to_db(
            asset_id.clone(),
            asset_schema,
            Some(created_at),
            spec.details().map(|d| d.to_string()),
            settled,
            name,
            precision,
            Some(ticker),
            created_at,
            None,
        )?;
        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::Settled),
            expiration: ActiveValue::Set(None),
            created_at: ActiveValue::Set(created_at),
            min_confirmations: ActiveValue::Set(0),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_id: ActiveValue::Set(Some(asset_id)),
            ..Default::default()
        };
        let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        for (utxo, amount) in issue_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                r#type: ActiveValue::Set(ColoringType::Issue),
                assignment: ActiveValue::Set(Assignment::Fungible(amount)),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }

        let asset = AssetNIA::get_asset_details(self, &asset, None, None, None, None, None, None)?;

        self.update_backup_info(false)?;

        info!(self.logger, "Issue asset NIA completed");
        Ok(asset)
    }

    pub(crate) fn new_token_data(
        &self,
        index: TokenIndex,
        media_data: &Option<(Attachment, Media)>,
        attachments: BTreeMap<u8, Attachment>,
    ) -> TokenData {
        TokenData {
            index,
            media: media_data
                .as_ref()
                .map(|(attachment, _)| attachment.clone()),
            attachments: Confined::try_from(attachments.clone()).unwrap(),
            ..Default::default()
        }
    }

    /// Issue a new RGB UDA asset with the provided `ticker`, `name`, optional `details` and
    /// `precision`, then return it.
    ///
    /// An optional `media_file_path` containing the path to a media file can be provided. Its hash
    /// and mime type will be encoded in the contract.
    ///
    /// An optional `attachments_file_paths` containing paths to extra media files can be provided.
    /// Their hash and mime type will be encoded in the contract.
    pub fn issue_asset_uda(
        &self,
        ticker: String,
        name: String,
        details: Option<String>,
        precision: u8,
        media_file_path: Option<String>,
        attachments_file_paths: Vec<String>,
    ) -> Result<AssetUDA, Error> {
        info!(
            self.logger,
            "Issuing UDA asset with ticker '{}' name '{}' precision '{}'...",
            ticker,
            name,
            precision,
        );

        let asset_schema = &AssetSchema::Uda;

        self.check_schema_support(asset_schema)?;

        if attachments_file_paths.len() > MAX_ATTACHMENTS {
            return Err(Error::InvalidAttachments {
                details: format!("no more than {MAX_ATTACHMENTS} attachments are supported"),
            });
        }

        let db_data = self.database.get_db_data(false)?;

        let mut unspents: Vec<LocalUnspent> = self.database.get_rgb_allocations(
            self.database.get_unspent_txos(db_data.txos.clone())?,
            None,
            None,
            None,
            None,
        )?;
        unspents.retain(|u| {
            !(u.rgb_allocations
                .iter()
                .any(|a| !a.incoming && a.status.waiting_counterparty()))
        });

        let created_at = now().unix_timestamp();
        let text = RicardianContract::default();
        let terms = ContractTerms { text, media: None };

        let details_obj = if let Some(details) = &details {
            Some(self.check_details(details.clone())?)
        } else {
            None
        };
        let ticker_obj = self._check_ticker(ticker.clone())?;
        let spec = AssetSpec {
            ticker: ticker_obj.clone(),
            name: self._check_name(name.clone())?,
            details: details_obj,
            precision: self._check_precision(precision)?,
        };

        let issue_utxo = self.get_utxo(&[], Some(&unspents), false, None)?;
        debug!(self.logger, "Issuing on UTXO: {issue_utxo:?}");

        let index = TokenIndex::from_inner(UDA_FIXED_INDEX);

        let media_data = if let Some(media_file_path) = &media_file_path {
            Some(self._file_details(media_file_path)?)
        } else {
            None
        };

        let mut attachments = BTreeMap::new();
        let mut media_attachments = HashMap::new();
        for (idx, attachment_file_path) in attachments_file_paths.iter().enumerate() {
            let (attachment, media) = self._file_details(attachment_file_path)?;
            attachments.insert(idx as u8, attachment);
            media_attachments.insert(idx as u8, media);
        }

        #[cfg(test)]
        let token_data = mock_token_data(self, index, &media_data, attachments);
        #[cfg(not(test))]
        let token_data = self.new_token_data(index, &media_data, attachments);

        let fraction = OwnedFraction::from_inner(1);
        let allocation = Allocation::with(token_data.index, fraction);

        let token = TokenLight {
            index: token_data.index.into(),
            media: media_data.as_ref().map(|(_, media)| media.clone()),
            attachments: media_attachments.clone(),
            ..Default::default()
        };

        let mut runtime = self.rgb_runtime()?;
        let builder = ContractBuilder::with(
            Identity::default(),
            UniqueDigitalAsset::schema(),
            UniqueDigitalAsset::types(),
            UniqueDigitalAsset::scripts(),
            self.chain_net(),
        )
        .add_global_state("spec", spec)
        .expect("invalid spec")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_data(
            "assetOwner",
            self.get_builder_seal(issue_utxo.clone()),
            allocation,
        )
        .expect("invalid global state data")
        .add_global_state("tokens", token_data.clone())
        .expect("invalid tokens");

        let validated_contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = validated_contract.contract_id().to_string();
        runtime
            .import_contract(validated_contract, &DumbResolver)
            .expect("failure importing issued contract");

        if let Some((_, media)) = &media_data {
            self.copy_media_and_save(media_file_path.unwrap(), media)?;
        }
        for (idx, attachment_file_path) in attachments_file_paths.into_iter().enumerate() {
            let media = media_attachments.get(&(idx as u8)).unwrap();
            self.copy_media_and_save(attachment_file_path, media)?;
        }

        let asset = self.add_asset_to_db(
            asset_id.clone(),
            asset_schema,
            Some(created_at),
            details.clone(),
            1,
            name.clone(),
            precision,
            Some(ticker.clone()),
            created_at,
            None,
        )?;
        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::Settled),
            expiration: ActiveValue::Set(None),
            created_at: ActiveValue::Set(created_at),
            min_confirmations: ActiveValue::Set(0),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_id: ActiveValue::Set(Some(asset_id.clone())),
            ..Default::default()
        };
        let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        let db_coloring = DbColoringActMod {
            txo_idx: ActiveValue::Set(issue_utxo.idx),
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            r#type: ActiveValue::Set(ColoringType::Issue),
            assignment: ActiveValue::Set(Assignment::NonFungible),
            ..Default::default()
        };
        self.database.set_coloring(db_coloring)?;
        let db_token = DbTokenActMod {
            asset_idx: ActiveValue::Set(asset.idx),
            index: ActiveValue::Set(token_data.index.into()),
            embedded_media: ActiveValue::Set(false),
            reserves: ActiveValue::Set(false),
            ..Default::default()
        };
        let token_idx = self.database.set_token(db_token)?;
        if let Some((_, media)) = &media_data {
            self.save_token_media(token_idx, media.get_digest(), media.mime.clone(), None)?;
        }
        for (attachment_id, media) in media_attachments {
            self.save_token_media(
                token_idx,
                media.get_digest(),
                media.mime.clone(),
                Some(attachment_id),
            )?;
        }

        let asset =
            AssetUDA::get_asset_details(self, &asset, Some(token), None, None, None, None, None)?;

        self.update_backup_info(false)?;

        info!(self.logger, "Issue asset UDA completed");
        Ok(asset)
    }

    /// Issue a new RGB CFA asset with the provided `name`, optional `details`, `precision` and
    /// `amounts`, then return it.
    ///
    /// An optional `file_path` containing the path to a media file can be provided. Its hash and
    /// mime type will be encoded in the contract.
    ///
    /// At least 1 amount needs to be provided and the sum of all amounts cannot exceed the maximum
    /// `u64` value.
    ///
    /// If `amounts` contains more than 1 element, each one will be issued as a separate allocation
    /// for the same asset (on a separate UTXO that needs to be already available).
    pub fn issue_asset_cfa(
        &self,
        name: String,
        details: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, Error> {
        info!(
            self.logger,
            "Issuing CFA asset with name '{}' precision '{}' amounts '{:?}'...",
            name,
            precision,
            amounts
        );

        let asset_schema = &AssetSchema::Cfa;

        self.check_schema_support(asset_schema)?;

        let settled = self._get_total_issue_amount(&amounts)?;

        let db_data = self.database.get_db_data(false)?;

        let mut unspents: Vec<LocalUnspent> = self.database.get_rgb_allocations(
            self.database.get_unspent_txos(db_data.txos.clone())?,
            None,
            None,
            None,
            None,
        )?;
        unspents.retain(|u| {
            !(u.rgb_allocations
                .iter()
                .any(|a| !a.incoming && a.status.waiting_counterparty()))
        });

        let created_at = now().unix_timestamp();
        let text = RicardianContract::default();
        let media_data = if let Some(file_path) = &file_path {
            Some(self._file_details(file_path)?)
        } else {
            None
        };
        let terms = ContractTerms {
            text,
            media: media_data
                .as_ref()
                .map(|(attachment, _)| attachment.clone()),
        };
        let precision_state = self._check_precision(precision)?;
        let name_state = self._check_name(name.clone())?;

        let mut runtime = self.rgb_runtime()?;
        let mut builder = ContractBuilder::with(
            Identity::default(),
            CollectibleFungibleAsset::schema(),
            CollectibleFungibleAsset::types(),
            CollectibleFungibleAsset::scripts(),
            self.chain_net(),
        )
        .add_global_state("name", name_state)
        .expect("invalid name")
        .add_global_state("precision", precision_state)
        .expect("invalid precision")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_global_state("issuedSupply", Amount::from(settled))
        .expect("invalid issuedSupply");

        if let Some(details) = &details {
            builder = builder
                .add_global_state("details", self.check_details(details.clone())?)
                .expect("invalid details");
        };

        let mut issue_utxos: HashMap<DbTxo, u64> = HashMap::new();
        let mut exclude_outpoints = vec![];
        for amount in &amounts {
            let utxo = self.get_utxo(&exclude_outpoints, Some(&unspents), false, None)?;
            exclude_outpoints.push(utxo.outpoint());
            issue_utxos.insert(utxo.clone(), *amount);

            builder = builder
                .add_fungible_state("assetOwner", self.get_builder_seal(utxo), *amount)
                .expect("invalid global state data");
        }
        debug!(self.logger, "Issuing on UTXOs: {issue_utxos:?}");

        let validated_contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = validated_contract.contract_id().to_string();
        runtime
            .import_contract(validated_contract, &DumbResolver)
            .expect("failure importing issued contract");

        let media_idx = if let Some(file_path) = file_path {
            let (_, media) = media_data.unwrap();
            Some(self.copy_media_and_save(file_path, &media)?)
        } else {
            None
        };

        let asset = self.add_asset_to_db(
            asset_id.clone(),
            asset_schema,
            Some(created_at),
            details,
            settled,
            name,
            precision,
            None,
            created_at,
            media_idx,
        )?;
        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::Settled),
            expiration: ActiveValue::Set(None),
            created_at: ActiveValue::Set(created_at),
            min_confirmations: ActiveValue::Set(0),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_id: ActiveValue::Set(Some(asset_id)),
            ..Default::default()
        };
        let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        for (utxo, amount) in issue_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                r#type: ActiveValue::Set(ColoringType::Issue),
                assignment: ActiveValue::Set(Assignment::Fungible(amount)),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }

        let asset = AssetCFA::get_asset_details(self, &asset, None, None, None, None, None, None)?;

        self.update_backup_info(false)?;

        info!(self.logger, "Issue asset CFA completed");
        Ok(asset)
    }

    /// Issue a new RGB IFA asset with the provided `ticker`, `name`, `precision`, `amounts`,
    /// `inflation_amounts` and `replace_rights_num`, then return it.
    ///
    /// At least 1 amount needs to be provided and the sum of all amounts cannot exceed the maximum
    /// `u64` value.
    ///
    /// If `amounts` contains more than 1 element, each one will be issued as a separate allocation
    /// for the same asset (on a separate UTXO that needs to be already available).
    ///
    /// The `inflation_amounts` can be empty. If provided the sum of its elements plus the sum of
    /// `amounts` cannot exceed the maximum `u64` value.
    ///
    /// The `replace_rights_num` can be set to 0. If provided it represents the number of replace
    /// rights to create.
    pub fn issue_asset_ifa(
        &self,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
        inflation_amounts: Vec<u64>,
        replace_rights_num: u8,
    ) -> Result<AssetIFA, Error> {
        info!(
            self.logger,
            "Issuing IFA asset with ticker '{}' name '{}' precision '{}' amounts '{:?}' inflation amounts {:?} replace rights num {}...",
            ticker,
            name,
            precision,
            amounts,
            inflation_amounts,
            replace_rights_num,
        );

        let asset_schema = &AssetSchema::Ifa;

        self.check_schema_support(asset_schema)?;

        let settled = self._get_total_issue_amount(&amounts)?;
        let inflation_amt = self._get_total_inflation_amount(&inflation_amounts, settled)?;

        let db_data = self.database.get_db_data(false)?;

        let mut unspents: Vec<LocalUnspent> = self.database.get_rgb_allocations(
            self.database.get_unspent_txos(db_data.txos.clone())?,
            None,
            None,
            None,
            None,
        )?;
        unspents.retain(|u| {
            !(u.rgb_allocations
                .iter()
                .any(|a| !a.incoming && a.status.waiting_counterparty()))
        });

        let created_at = now().unix_timestamp();
        let text = RicardianContract::default();
        #[cfg(test)]
        let terms = mock_asset_terms(self, text, None);
        #[cfg(not(test))]
        let terms = self.new_asset_terms(text, None);
        #[cfg(test)]
        let details = mock_contract_details(self);
        #[cfg(not(test))]
        let details = None;
        let spec = AssetSpec {
            ticker: self._check_ticker(ticker.clone())?,
            name: self._check_name(name.clone())?,
            details,
            precision: self._check_precision(precision)?,
        };

        let mut runtime = self.rgb_runtime()?;
        let mut builder = ContractBuilder::with(
            Identity::default(),
            InflatableFungibleAsset::schema(),
            InflatableFungibleAsset::types(),
            InflatableFungibleAsset::scripts(),
            self.chain_net(),
        )
        .add_global_state("spec", spec.clone())
        .expect("invalid spec")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_global_state("issuedSupply", Amount::from(settled))
        .expect("invalid issuedSupply")
        .add_global_state("maxSupply", Amount::from(settled + inflation_amt))
        .expect("invalid maxSupply");

        let mut issue_utxos: HashMap<DbTxo, u64> = HashMap::new();
        let mut exclude_outpoints: Vec<Outpoint> = vec![];
        for amount in &amounts {
            let utxo = self.get_utxo(&exclude_outpoints, Some(&unspents), false, None)?;
            exclude_outpoints.push(utxo.outpoint());
            issue_utxos.insert(utxo.clone(), *amount);

            builder = builder
                .add_fungible_state("assetOwner", self.get_builder_seal(utxo), *amount)
                .expect("invalid global state data");
        }
        debug!(self.logger, "Issuing on UTXOs: {issue_utxos:?}");

        let mut inflation_utxos: HashMap<DbTxo, u64> = HashMap::new();
        for amount in &inflation_amounts {
            let utxo = self.get_utxo(&exclude_outpoints, Some(&unspents), false, Some(0))?;
            exclude_outpoints.push(utxo.outpoint());
            inflation_utxos.insert(utxo.clone(), *amount);

            builder = builder
                .add_fungible_state("inflationAllowance", self.get_builder_seal(utxo), *amount)
                .expect("invalid global state data");
        }
        debug!(
            self.logger,
            "Assigning inflation rights: {inflation_utxos:?}"
        );

        let mut replace_utxos: HashSet<DbTxo> = HashSet::new();
        for _ in 0..replace_rights_num {
            let utxo = self.get_utxo(&exclude_outpoints, Some(&unspents), false, Some(0))?;
            exclude_outpoints.push(utxo.outpoint());
            replace_utxos.insert(utxo.clone());

            builder = builder
                .add_rights("replaceRight", self.get_builder_seal(utxo))
                .expect("invalid global state data");
        }
        debug!(self.logger, "Assigning replace rights: {replace_utxos:?}");

        let validated_contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = validated_contract.contract_id().to_string();
        runtime
            .import_contract(validated_contract, &DumbResolver)
            .expect("failure importing issued contract");

        let asset = self.add_asset_to_db(
            asset_id.clone(),
            asset_schema,
            Some(created_at),
            spec.details().map(|d| d.to_string()),
            settled,
            name,
            precision,
            Some(ticker),
            created_at,
            None,
        )?;
        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::Settled),
            expiration: ActiveValue::Set(None),
            created_at: ActiveValue::Set(created_at),
            min_confirmations: ActiveValue::Set(0),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_id: ActiveValue::Set(Some(asset_id)),
            ..Default::default()
        };
        let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        for (utxo, amount) in issue_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                r#type: ActiveValue::Set(ColoringType::Issue),
                assignment: ActiveValue::Set(Assignment::Fungible(amount)),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }
        for (utxo, amount) in inflation_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                r#type: ActiveValue::Set(ColoringType::Issue),
                assignment: ActiveValue::Set(Assignment::InflationRight(amount)),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }
        for utxo in replace_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                r#type: ActiveValue::Set(ColoringType::Issue),
                assignment: ActiveValue::Set(Assignment::ReplaceRight),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }

        let asset = AssetIFA::get_asset_details(self, &asset, None, None, None, None, None, None)?;

        self.update_backup_info(false)?;

        info!(self.logger, "Issue asset IFA completed");
        Ok(asset)
    }

    fn _receive(
        &self,
        asset_id: Option<String>,
        assignment: Assignment,
        duration_seconds: Option<u32>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
        beneficiary: Beneficiary,
        recipient_type: RecipientTypeFull,
    ) -> Result<(String, String, Option<i64>, i32), Error> {
        #[cfg(test)]
        let network = mock_chain_net(self);
        #[cfg(not(test))]
        let network: ChainNet = self.bitcoin_network().into();

        let beneficiary = XChainNet::with(network, beneficiary);
        let recipient_id = beneficiary.to_string();
        debug!(self.logger, "Recipient ID: {recipient_id}");
        let (schema, contract_id) = if let Some(aid) = asset_id.clone() {
            let asset = self.database.check_asset_exists(aid.clone())?;
            let contract_id = ContractId::from_str(&aid).expect("invalid contract ID");
            (Some(asset.schema), Some(contract_id))
        } else {
            (None, None)
        };

        self.check_transport_endpoints(&transport_endpoints)?;
        let mut transport_endpoints_dedup = transport_endpoints.clone();
        transport_endpoints_dedup.sort();
        transport_endpoints_dedup.dedup();
        if transport_endpoints_dedup.len() != transport_endpoints.len() {
            return Err(Error::InvalidTransportEndpoints {
                details: s!("no duplicate transport endpoints allowed"),
            });
        }
        let mut endpoints: Vec<String> = vec![];
        for endpoint_str in &transport_endpoints {
            let rgb_transport = RgbTransport::from_str(endpoint_str)?;
            match &rgb_transport {
                RgbTransport::JsonRpc { .. } => {
                    endpoints.push(
                        TransportEndpoint::try_from(rgb_transport)
                            .unwrap()
                            .endpoint
                            .clone(),
                    );
                }
                _ => {
                    return Err(Error::UnsupportedTransportType);
                }
            }
        }

        let mut invoice_builder = RgbInvoiceBuilder::new(beneficiary);
        if let Some(schema) = schema {
            invoice_builder = invoice_builder.set_schema(schema.into());
        }
        if let Some(contract_id) = contract_id {
            invoice_builder = invoice_builder.set_contract(contract_id);
        }
        let transports: Vec<&str> = transport_endpoints.iter().map(AsRef::as_ref).collect();
        invoice_builder = invoice_builder.add_transports(transports).unwrap();
        let detected_assignment = match (&assignment, schema) {
            (
                Assignment::Fungible(amt),
                Some(AssetSchema::Nia) | Some(AssetSchema::Cfa) | Some(AssetSchema::Ifa) | None,
            ) => {
                invoice_builder = invoice_builder.set_amount_raw(*amt);
                invoice_builder = invoice_builder.set_assignment_name("assetOwner");
                assignment
            }
            (Assignment::Any, Some(AssetSchema::Nia) | Some(AssetSchema::Cfa)) => {
                invoice_builder = invoice_builder.set_assignment_name("assetOwner");
                Assignment::Fungible(0)
            }
            (Assignment::NonFungible | Assignment::Any, Some(AssetSchema::Uda)) => {
                invoice_builder = invoice_builder.set_assignment_name("assetOwner");
                Assignment::NonFungible
            }
            (Assignment::ReplaceRight, Some(AssetSchema::Ifa)) => {
                invoice_builder = invoice_builder.set_void();
                invoice_builder = invoice_builder.set_assignment_name("replaceRight");
                Assignment::ReplaceRight
            }
            (Assignment::InflationRight(amt), Some(AssetSchema::Ifa)) => {
                invoice_builder = invoice_builder.set_amount_raw(*amt);
                invoice_builder = invoice_builder.set_assignment_name("inflationAllowance");
                assignment
            }
            (Assignment::Any, _) => Assignment::Any,
            _ => return Err(Error::InvalidAssignment),
        };
        let created_at = now().unix_timestamp();
        let expiry = if duration_seconds == Some(0) {
            None
        } else {
            let duration_seconds = duration_seconds.unwrap_or(DURATION_RCV_TRANSFER) as i64;
            let expiry = created_at + duration_seconds;
            invoice_builder = invoice_builder.set_expiry_timestamp(expiry);
            Some(expiry)
        };

        let invoice = invoice_builder.finish();
        let invoice_string = invoice.to_string();

        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::WaitingCounterparty),
            expiration: ActiveValue::Set(expiry),
            created_at: ActiveValue::Set(created_at),
            min_confirmations: ActiveValue::Set(min_confirmations),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_id: ActiveValue::Set(asset_id),
            ..Default::default()
        };
        let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            requested_assignment: ActiveValue::Set(Some(detected_assignment)),
            incoming: ActiveValue::Set(true),
            recipient_id: ActiveValue::Set(Some(recipient_id.clone())),
            recipient_type: ActiveValue::Set(Some(recipient_type)),
            invoice_string: ActiveValue::Set(Some(invoice_string.clone())),
            ..Default::default()
        };
        let transfer_idx = self.database.set_transfer(transfer)?;
        for endpoint in endpoints {
            self.save_transfer_transport_endpoint(
                transfer_idx,
                &LocalTransportEndpoint {
                    endpoint,
                    transport_type: TransportType::JsonRpc,
                    used: false,
                    usable: true,
                },
            )?;
        }

        Ok((recipient_id, invoice_string, expiry, batch_transfer_idx))
    }

    /// Blind an UTXO to receive RGB assets and return the resulting [`ReceiveData`].
    ///
    /// An optional asset ID can be specified, which will be embedded in the invoice, resulting in
    /// the refusal of the transfer is the asset doesn't match.
    ///
    /// An optional amount can be specified, which will be embedded in the invoice. It will not be
    /// checked when accepting the transfer.
    ///
    /// An optional duration (in seconds) can be specified, which will set the expiration of the
    /// invoice. A duration of 0 seconds means no expiration.
    ///
    /// Each endpoint in the provided `transport_endpoints` list will be used as RGB data exchange
    /// medium. The list needs to contain at least 1 endpoint and a maximum of 3. Strings
    /// specifying invalid endpoints and duplicate ones will cause an error to be raised. A valid
    /// endpoint string encodes an
    /// [`RgbTransport`](https://docs.rs/rgb-wallet/latest/rgbwallet/enum.RgbTransport.html). At
    /// the moment the only supported variant is JsonRpc (e.g. `rpc://127.0.0.1` or
    /// `rpcs://example.com`).
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    pub fn blind_receive(
        &self,
        asset_id: Option<String>,
        assignment: Assignment,
        duration_seconds: Option<u32>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, Error> {
        info!(
            self.logger,
            "Receiving via blinded UTXO for asset '{:?}' with duration '{:?}'...",
            asset_id,
            duration_seconds
        );

        let mut unspents: Vec<LocalUnspent> = self.database.get_rgb_allocations(
            self.database.get_unspent_txos(vec![])?,
            None,
            None,
            None,
            None,
        )?;
        unspents.retain(|u| {
            !(u.rgb_allocations
                .iter()
                .any(|a| !a.incoming && a.status.waiting_counterparty()))
        });
        let utxo = self.get_utxo(&[], Some(&unspents), true, None)?;
        let unblinded_utxo = utxo.outpoint();
        debug!(
            self.logger,
            "Blinding outpoint '{}'",
            unblinded_utxo.to_string()
        );
        let blind_seal = self.get_blind_seal(utxo.clone()).transmutate();
        let beneficiary = Beneficiary::BlindedSeal(blind_seal.conceal());

        let (recipient_id, invoice, expiration_timestamp, batch_transfer_idx) = self._receive(
            asset_id,
            assignment,
            duration_seconds,
            transport_endpoints,
            min_confirmations,
            beneficiary,
            RecipientTypeFull::Blind { unblinded_utxo },
        )?;

        let mut runtime = self.rgb_runtime()?;
        runtime.store_secret_seal(blind_seal)?;

        self.update_backup_info(false)?;

        info!(self.logger, "Blind receive completed");
        Ok(ReceiveData {
            invoice,
            recipient_id,
            expiration_timestamp,
            batch_transfer_idx,
        })
    }

    /// Create an address to receive RGB assets and return the resulting [`ReceiveData`].
    ///
    /// An optional asset ID can be specified, which will be embedded in the invoice, resulting in
    /// the refusal of the transfer is the asset doesn't match.
    ///
    /// An optional amount can be specified, which will be embedded in the invoice. It will not be
    /// checked when accepting the transfer.
    ///
    /// An optional duration (in seconds) can be specified, which will set the expiration of the
    /// invoice. A duration of 0 seconds means no expiration.
    ///
    /// Each endpoint in the provided `transport_endpoints` list will be used as RGB data exchange
    /// medium. The list needs to contain at least 1 endpoint and a maximum of 3. Strings
    /// specifying invalid endpoints and duplicate ones will cause an error to be raised. A valid
    /// endpoint string encodes an
    /// [`RgbTransport`](https://docs.rs/rgb-wallet/latest/rgbwallet/enum.RgbTransport.html). At
    /// the moment the only supported variant is JsonRpc (e.g. `rpc://127.0.0.1` or
    /// `rpcs://example.com`).
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    pub fn witness_receive(
        &mut self,
        asset_id: Option<String>,
        assignment: Assignment,
        duration_seconds: Option<u32>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, Error> {
        info!(
            self.logger,
            "Receiving via witness TX for asset '{:?}' with duration '{:?}'...",
            asset_id,
            duration_seconds
        );

        let script_pubkey = self.get_new_address()?.script_pubkey();
        let beneficiary = beneficiary_from_script_buf(script_pubkey.clone());

        let (recipient_id, invoice, expiration_timestamp, batch_transfer_idx) = self._receive(
            asset_id,
            assignment,
            duration_seconds,
            transport_endpoints,
            min_confirmations,
            beneficiary,
            RecipientTypeFull::Witness { vout: None },
        )?;

        self.database
            .set_pending_witness_script(DbPendingWitnessScriptActMod {
                script: ActiveValue::Set(script_pubkey.to_hex_string()),
                ..Default::default()
            })?;

        self.update_backup_info(false)?;

        info!(self.logger, "Witness receive completed");
        Ok(ReceiveData {
            invoice,
            recipient_id,
            expiration_timestamp,
            batch_transfer_idx,
        })
    }

    /// Finalize a PSBT, optionally providing BDK sign options to tweak the behavior of the
    /// finalizer.
    pub fn finalize_psbt(
        &self,
        signed_psbt: String,
        sign_options: Option<SignOptions>,
    ) -> Result<String, Error> {
        info!(self.logger, "Finalizing PSBT...");
        let mut psbt = Psbt::from_str(&signed_psbt)?;
        let sign_options = sign_options.unwrap_or_default();
        if !self
            .bdk_wallet
            .finalize_psbt(&mut psbt, sign_options)
            .map_err(InternalError::from)?
        {
            return Err(Error::CannotFinalizePsbt);
        }
        info!(self.logger, "Finalize PSBT completed");
        Ok(psbt.to_string())
    }

    fn _sign_psbt(&self, psbt: &mut Psbt, sign_options: Option<SignOptions>) -> Result<(), Error> {
        let sign_options = sign_options.unwrap_or_default();
        self.bdk_wallet
            .sign(psbt, sign_options)
            .map_err(InternalError::from)?;
        Ok(())
    }

    /// Sign a PSBT, optionally providing BDK sign options.
    pub fn sign_psbt(
        &self,
        unsigned_psbt: String,
        sign_options: Option<SignOptions>,
    ) -> Result<String, Error> {
        info!(self.logger, "Signing PSBT...");
        let mut psbt = Psbt::from_str(&unsigned_psbt)?;
        self._sign_psbt(&mut psbt, sign_options)?;
        info!(self.logger, "Sign PSBT completed");
        Ok(psbt.to_string())
    }

    fn _delete_batch_transfer(
        &self,
        batch_transfer: &DbBatchTransfer,
        asset_transfers: &Vec<DbAssetTransfer>,
        colorings: &[DbColoring],
        txos: &[DbTxo],
    ) -> Result<(), Error> {
        let mut txos_to_delete = HashSet::new();
        for asset_transfer in asset_transfers {
            self.database.del_coloring(asset_transfer.idx)?;
            colorings
                .iter()
                .filter(|c| c.asset_transfer_idx == asset_transfer.idx)
                .for_each(|c| {
                    if let Some(txo) = txos.iter().find(|t| !t.exists && t.idx == c.txo_idx) {
                        txos_to_delete.insert(txo.idx);
                    }
                });
        }
        for txo in txos_to_delete {
            self.database.del_txo(txo)?;
        }
        Ok(self.database.del_batch_transfer(batch_transfer)?)
    }

    /// Delete eligible transfers from the database and return true if any transfer has been
    /// deleted.
    ///
    /// An optional `batch_transfer_idx` can be provided to operate on a single batch transfer.
    ///
    /// If a `batch_transfer_idx` is provided and `no_asset_only` is true, transfers with an
    /// associated asset ID will not be deleted and instead return an error.
    ///
    /// If no `batch_transfer_idx` is provided, all failed transfers will be deleted, and if
    /// `no_asset_only` is true transfers with an associated asset ID will be skipped.
    ///
    /// Eligible transfers are the ones in status [`TransferStatus::Failed`].
    pub fn delete_transfers(
        &self,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        info!(
            self.logger,
            "Deleting batch transfer with idx {:?}...", batch_transfer_idx
        );

        let db_data = self.database.get_db_data(false)?;
        let mut transfers_changed = false;

        if let Some(batch_transfer_idx) = batch_transfer_idx {
            let batch_transfer = &self
                .database
                .get_batch_transfer_or_fail(batch_transfer_idx, &db_data.batch_transfers)?;

            if !batch_transfer.failed() {
                return Err(Error::CannotDeleteBatchTransfer);
            }

            let asset_transfers = batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;

            if no_asset_only {
                let connected_assets = asset_transfers.iter().any(|t| t.asset_id.is_some());
                if connected_assets {
                    return Err(Error::CannotDeleteBatchTransfer);
                }
            }

            transfers_changed = true;
            self._delete_batch_transfer(
                batch_transfer,
                &asset_transfers,
                &db_data.colorings,
                &db_data.txos,
            )?
        } else {
            // delete all failed transfers
            let mut batch_transfers: Vec<DbBatchTransfer> = db_data
                .batch_transfers
                .clone()
                .into_iter()
                .filter(|t| t.failed())
                .collect();
            for batch_transfer in batch_transfers.iter_mut() {
                let asset_transfers =
                    batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;
                if no_asset_only {
                    let connected_assets = asset_transfers.iter().any(|t| t.asset_id.is_some());
                    if connected_assets {
                        continue;
                    }
                }
                transfers_changed = true;
                self._delete_batch_transfer(
                    batch_transfer,
                    &asset_transfers,
                    &db_data.colorings,
                    &db_data.txos,
                )?
            }
        }

        if transfers_changed {
            self.update_backup_info(false)?;
        }

        info!(self.logger, "Delete transfer completed");
        Ok(transfers_changed)
    }

    pub(crate) fn _get_new_address(&mut self, keychain: KeychainKind) -> Result<BdkAddress, Error> {
        let address = self.bdk_wallet.reveal_next_address(keychain).address;
        self.bdk_wallet.persist(&mut self.bdk_database)?;
        Ok(address)
    }

    pub(crate) fn get_new_address(&mut self) -> Result<BdkAddress, Error> {
        self._get_new_address(KeychainKind::External)
    }

    /// Return a new Bitcoin address from the vanilla wallet.
    pub fn get_address(&mut self) -> Result<String, Error> {
        info!(self.logger, "Getting address...");
        let address = self._get_new_address(KeychainKind::Internal)?;

        self.update_backup_info(false)?;

        info!(self.logger, "Get address completed");
        Ok(address.to_string())
    }

    /// Return the [`Balance`] for the RGB asset with the provided ID.
    pub fn get_asset_balance(&self, asset_id: String) -> Result<Balance, Error> {
        info!(self.logger, "Getting balance for asset '{}'...", asset_id);
        self.database.check_asset_exists(asset_id.clone())?;
        let balance = self
            .database
            .get_asset_balance(asset_id, None, None, None, None, None);
        info!(self.logger, "Get asset balance completed");
        balance
    }

    /// Return the [`Metadata`] for the RGB asset with the provided ID.
    pub fn get_asset_metadata(&self, asset_id: String) -> Result<Metadata, Error> {
        info!(self.logger, "Getting metadata for asset '{}'...", asset_id);
        let asset = self.database.check_asset_exists(asset_id.clone())?;

        let issued_supply = asset.issued_supply.parse::<u64>().unwrap();
        let token = if matches!(asset.schema, AssetSchema::Uda) {
            let medias = self.database.iter_media()?;
            let tokens = self.database.iter_tokens()?;
            let token_medias = self.database.iter_token_medias()?;
            if let Some(token_light) =
                self.get_asset_token(asset.idx, &medias, &tokens, &token_medias)
            {
                let mut token = Token {
                    index: token_light.index,
                    ticker: token_light.ticker,
                    name: token_light.name,
                    details: token_light.details,
                    embedded_media: None,
                    media: token_light.media,
                    attachments: token_light.attachments,
                    reserves: None,
                };
                if token_light.embedded_media || token_light.reserves {
                    let runtime = self.rgb_runtime()?;
                    let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
                    let contract = runtime.contract_wrapper::<UniqueDigitalAsset>(contract_id)?;
                    let uda_token =
                        Token::from_token_data(&contract.token_data(), self.get_media_dir());
                    token.embedded_media = uda_token.embedded_media;
                    token.reserves = uda_token.reserves;
                }
                Some(token)
            } else {
                None
            }
        } else {
            None
        };

        info!(self.logger, "Get asset metadata completed");
        Ok(Metadata {
            asset_schema: asset.schema,
            issued_supply,
            timestamp: asset.timestamp,
            name: asset.name,
            precision: asset.precision,
            ticker: asset.ticker,
            details: asset.details,
            token,
        })
    }

    pub(crate) fn get_or_insert_media(&self, digest: String, mime: String) -> Result<i32, Error> {
        Ok(match self.database.get_media_by_digest(digest.clone())? {
            Some(media) => media.idx,
            None => self.database.set_media(DbMediaActMod {
                digest: ActiveValue::Set(digest),
                mime: ActiveValue::Set(mime),
                ..Default::default()
            })?,
        })
    }

    pub(crate) fn save_token_media(
        &self,
        token_idx: i32,
        digest: String,
        mime: String,
        attachment_id: Option<u8>,
    ) -> Result<(), Error> {
        let media_idx = self.get_or_insert_media(digest, mime)?;

        self.database.set_token_media(DbTokenMediaActMod {
            token_idx: ActiveValue::Set(token_idx),
            media_idx: ActiveValue::Set(media_idx),
            attachment_id: ActiveValue::Set(attachment_id),
            ..Default::default()
        })?;

        Ok(())
    }

    pub(crate) fn add_asset_to_db(
        &self,
        asset_id: String,
        schema: &AssetSchema,
        added_at: Option<i64>,
        details: Option<String>,
        issued_supply: u64,
        name: String,
        precision: u8,
        ticker: Option<String>,
        timestamp: i64,
        media_idx: Option<i32>,
    ) -> Result<DbAsset, Error> {
        let added_at = added_at.unwrap_or_else(|| now().unix_timestamp());
        let mut db_asset = DbAssetActMod {
            idx: ActiveValue::NotSet,
            media_idx: ActiveValue::Set(media_idx),
            id: ActiveValue::Set(asset_id),
            schema: ActiveValue::Set(*schema),
            added_at: ActiveValue::Set(added_at),
            details: ActiveValue::Set(details),
            issued_supply: ActiveValue::Set(issued_supply.to_string()),
            name: ActiveValue::Set(name),
            precision: ActiveValue::Set(precision),
            ticker: ActiveValue::Set(ticker),
            timestamp: ActiveValue::Set(timestamp),
        };
        let idx = self.database.set_asset(db_asset.clone())?;
        db_asset.idx = ActiveValue::Set(idx);
        Ok(db_asset.try_into_model().unwrap())
    }

    pub(crate) fn get_asset_token(
        &self,
        asset_idx: i32,
        medias: &[DbMedia],
        tokens: &[DbToken],
        token_medias: &[DbTokenMedia],
    ) -> Option<TokenLight> {
        if let Some(db_token) = tokens.iter().find(|t| t.asset_idx == asset_idx) {
            let mut media = None;
            let mut attachments = HashMap::new();
            let media_dir = self.get_media_dir();
            token_medias
                .iter()
                .filter(|tm| tm.token_idx == db_token.idx)
                .for_each(|tm| {
                    let db_media = medias.iter().find(|m| m.idx == tm.media_idx).unwrap();
                    let media_tkn = Media::from_db_media(db_media, &media_dir);
                    if let Some(attachment_id) = tm.attachment_id {
                        attachments.insert(attachment_id, media_tkn);
                    } else {
                        media = Some(media_tkn);
                    }
                });

            Some(TokenLight {
                index: db_token.index,
                ticker: db_token.ticker.clone(),
                name: db_token.name.clone(),
                details: db_token.details.clone(),
                embedded_media: db_token.embedded_media,
                media,
                attachments,
                reserves: db_token.reserves,
            })
        } else {
            None
        }
    }

    fn _get_btc_balance(&self, keychain: KeychainKind) -> Result<Balance, Error> {
        let chain = self.bdk_wallet.local_chain();
        let chain_tip = self.bdk_wallet.latest_checkpoint().block_id();
        let outpoints = self.filter_unspents(keychain).map(|lo| ((), lo.outpoint));
        let balance = self.bdk_wallet.as_ref().balance(
            chain,
            chain_tip,
            CanonicalizationParams::default(),
            outpoints,
            |_, _| false,
        );

        let future = balance.total();
        Ok(Balance {
            settled: balance.confirmed.to_sat(),
            future: future.to_sat(),
            spendable: future.to_sat() - balance.immature.to_sat(),
        })
    }

    /// Return the [`BtcBalance`] of the internal Bitcoin wallets.
    pub fn get_btc_balance(
        &mut self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<BtcBalance, Error> {
        info!(self.logger, "Getting BTC balance...");

        self.sync_if_requested(online, skip_sync)?;

        let vanilla = self._get_btc_balance(KeychainKind::Internal)?;
        let colored = self._get_btc_balance(KeychainKind::External)?;

        let balance = BtcBalance { vanilla, colored };

        info!(self.logger, "Get BTC balance completed");
        Ok(balance)
    }

    /// List the known RGB assets.
    ///
    /// Providing an empty `filter_asset_schemas` will list assets for all schemas, otherwise only
    /// assets for the provided schemas will be listed.
    ///
    /// The returned `Assets` will have fields set to `None` for schemas that have not been
    /// requested.
    pub fn list_assets(&self, mut filter_asset_schemas: Vec<AssetSchema>) -> Result<Assets, Error> {
        info!(self.logger, "Listing assets...");
        if filter_asset_schemas.is_empty() {
            filter_asset_schemas = AssetSchema::VALUES.to_vec()
        }

        let batch_transfers = Some(self.database.iter_batch_transfers()?);
        let colorings = Some(self.database.iter_colorings()?);
        let txos = Some(self.database.iter_txos()?);
        let asset_transfers = Some(self.database.iter_asset_transfers()?);
        let transfers = Some(self.database.iter_transfers()?);
        let medias = Some(self.database.iter_media()?);

        let assets = self.database.iter_assets()?;
        let mut nia = None;
        let mut uda = None;
        let mut cfa = None;
        let mut ifa = None;
        for schema in filter_asset_schemas {
            match schema {
                AssetSchema::Nia => {
                    nia = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetNIA::get_asset_details(
                                    self,
                                    a,
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                    medias.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetNIA>, Error>>()?,
                    );
                }
                AssetSchema::Uda => {
                    let tokens = self.database.iter_tokens()?;
                    let token_medias = self.database.iter_token_medias()?;
                    uda = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetUDA::get_asset_details(
                                    self,
                                    a,
                                    self.get_asset_token(
                                        a.idx,
                                        &medias.clone().unwrap(),
                                        &tokens,
                                        &token_medias,
                                    ),
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetUDA>, Error>>()?,
                    );
                }
                AssetSchema::Cfa => {
                    cfa = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetCFA::get_asset_details(
                                    self,
                                    a,
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                    medias.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetCFA>, Error>>()?,
                    );
                }
                AssetSchema::Ifa => {
                    ifa = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetIFA::get_asset_details(
                                    self,
                                    a,
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                    medias.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetIFA>, Error>>()?,
                    );
                }
            }
        }

        info!(self.logger, "List assets completed");
        Ok(Assets { nia, uda, cfa, ifa })
    }

    pub(crate) fn sync_if_requested(
        &mut self,
        #[cfg_attr(
            not(any(feature = "electrum", feature = "esplora")),
            allow(unused_variables)
        )]
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<(), Error> {
        if !skip_sync {
            #[cfg(not(any(feature = "electrum", feature = "esplora")))]
            return Err(Error::Offline);
            #[cfg(any(feature = "electrum", feature = "esplora"))]
            {
                if let Some(online) = online {
                    self.check_online(online)?;
                } else {
                    return Err(Error::OnlineNeeded);
                }
                self.sync_db_txos(false)?;
            }
        }
        Ok(())
    }

    /// List the Bitcoin [`Transaction`]s known to the wallet.
    pub fn list_transactions(
        &mut self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<Vec<Transaction>, Error> {
        info!(self.logger, "Listing transactions...");

        self.sync_if_requested(online, skip_sync)?;

        let mut create_utxos_txids = vec![];
        let mut drain_txids = vec![];
        let wallet_transactions = self.database.iter_wallet_transactions()?;
        for tx in wallet_transactions {
            match tx.r#type {
                WalletTransactionType::CreateUtxos => create_utxos_txids.push(tx.txid),
                WalletTransactionType::Drain => drain_txids.push(tx.txid),
            }
        }
        let rgb_send_txids: Vec<String> = self
            .database
            .iter_batch_transfers()?
            .into_iter()
            .filter_map(|t| t.txid)
            .collect();
        let transactions = self
            .bdk_wallet
            .transactions()
            .map(|t| {
                let txid = t.tx_node.txid.to_string();
                let transaction_type = if drain_txids.contains(&txid) {
                    TransactionType::Drain
                } else if create_utxos_txids.contains(&txid) {
                    TransactionType::CreateUtxos
                } else if rgb_send_txids.contains(&txid) {
                    TransactionType::RgbSend
                } else {
                    TransactionType::User
                };
                let confirmation_time = match t.chain_position {
                    ChainPosition::Confirmed { anchor, .. } => Some(BlockTime {
                        height: anchor.block_id.height,
                        timestamp: anchor.confirmation_time,
                    }),
                    _ => None,
                };
                let (sent, received) = self.bdk_wallet.sent_and_received(&t.tx_node);
                let fee = self.bdk_wallet.calculate_fee(&t.tx_node).unwrap();
                Transaction {
                    transaction_type,
                    txid,
                    received: received.to_sat(),
                    sent: sent.to_sat(),
                    fee: fee.to_sat(),
                    confirmation_time,
                }
            })
            .collect();
        info!(self.logger, "List transactions completed");
        Ok(transactions)
    }

    /// List the RGB [`Transfer`]s known to the wallet.
    ///
    /// When an `asset_id` is not provided, return transfers that are not connected to a specific
    /// asset.
    pub fn list_transfers(&self, asset_id: Option<String>) -> Result<Vec<Transfer>, Error> {
        if let Some(asset_id) = &asset_id {
            info!(self.logger, "Listing transfers for asset '{}'...", asset_id);
            self.database.check_asset_exists(asset_id.clone())?;
        } else {
            info!(self.logger, "Listing transfers...");
        }
        let db_data = self.database.get_db_data(false)?;
        let asset_transfer_ids: Vec<i32> = db_data
            .asset_transfers
            .iter()
            .filter(|t| t.asset_id == asset_id)
            .filter(|t| t.user_driven)
            .map(|t| t.idx)
            .collect();
        let transfers: Vec<Transfer> = db_data
            .transfers
            .into_iter()
            .filter(|t| asset_transfer_ids.contains(&t.asset_transfer_idx))
            .map(|t| {
                let (asset_transfer, batch_transfer) =
                    t.related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)?;
                let tte_data = self.database.get_transfer_transport_endpoints_data(t.idx)?;
                Ok(Transfer::from_db_transfer(
                    &t,
                    self.database.get_transfer_data(
                        &t,
                        &asset_transfer,
                        &batch_transfer,
                        &db_data.txos,
                        &db_data.colorings,
                    )?,
                    tte_data
                        .iter()
                        .map(|(tte, ce)| {
                            TransferTransportEndpoint::from_db_transfer_transport_endpoint(tte, ce)
                        })
                        .collect(),
                ))
            })
            .collect::<Result<Vec<Transfer>, Error>>()?;

        info!(self.logger, "List transfers completed");
        Ok(transfers)
    }

    /// List the [`Unspent`]s known to the wallet.
    ///
    /// If `settled` is true only show settled RGB allocations, if false also show pending RGB
    /// allocations.
    pub fn list_unspents(
        &mut self,
        online: Option<Online>,
        settled_only: bool,
        skip_sync: bool,
    ) -> Result<Vec<Unspent>, Error> {
        info!(self.logger, "Listing unspents...");

        self.sync_if_requested(online, skip_sync)?;

        let db_data = self.database.get_db_data(false)?;

        let mut allocation_txos = self.database.get_unspent_txos(db_data.txos.clone())?;
        let spent_txos_ids: Vec<i32> = db_data
            .txos
            .clone()
            .into_iter()
            .filter(|t| t.spent)
            .map(|u| u.idx)
            .collect();
        let waiting_confs_batch_transfer_ids: Vec<i32> = db_data
            .batch_transfers
            .clone()
            .into_iter()
            .filter(|t| t.waiting_confirmations())
            .map(|t| t.idx)
            .collect();
        let waiting_confs_transfer_ids: Vec<i32> = db_data
            .asset_transfers
            .clone()
            .into_iter()
            .filter(|t| waiting_confs_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let almost_spent_txos_ids: Vec<i32> = db_data
            .colorings
            .clone()
            .into_iter()
            .filter(|c| {
                waiting_confs_transfer_ids.contains(&c.asset_transfer_idx)
                    && spent_txos_ids.contains(&c.txo_idx)
            })
            .map(|c| c.txo_idx)
            .collect();
        let mut spent_txos = db_data
            .txos
            .into_iter()
            .filter(|t| almost_spent_txos_ids.contains(&t.idx))
            .collect();
        allocation_txos.append(&mut spent_txos);

        let mut txos_allocations = self.database.get_rgb_allocations(
            allocation_txos,
            Some(db_data.colorings),
            Some(db_data.batch_transfers),
            Some(db_data.asset_transfers),
            Some(db_data.transfers),
        )?;

        txos_allocations
            .iter_mut()
            .for_each(|t| t.rgb_allocations.retain(|a| a.settled() || a.future()));

        txos_allocations.retain(|t| !(t.rgb_allocations.is_empty() && t.utxo.spent));

        let mut unspents: Vec<Unspent> = txos_allocations.into_iter().map(Unspent::from).collect();

        if settled_only {
            unspents
                .iter_mut()
                .for_each(|u| u.rgb_allocations.retain(|a| a.settled));
        }

        let mut internal_unspents: Vec<Unspent> =
            self.internal_unspents().map(Unspent::from).collect();

        unspents.append(&mut internal_unspents);

        info!(self.logger, "List unspents completed");
        Ok(unspents)
    }
}
