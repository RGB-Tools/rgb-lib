//! RGB wallet
//!
//! This module defines the [`Wallet`] structure and all its related data.

use amplify::{bmap, none, s, RawArray};
use base64::{engine::general_purpose, Engine as _};
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::util::bip32::ExtendedPubKey;
use bdk::bitcoin::{
    psbt::Psbt as BdkPsbt, Address as BdkAddress, Network as BdkNetwork, OutPoint as BdkOutPoint,
    Script as BdkScript, Transaction as BdkTransaction,
};
use bdk::blockchain::{
    Blockchain, ConfigurableBlockchain, ElectrumBlockchain, ElectrumBlockchainConfig,
};
use bdk::database::any::SledDbConfiguration;
use bdk::database::{
    AnyDatabase, BatchDatabase, ConfigurableDatabase as BdkConfigurableDatabase, MemoryDatabase,
};
use bdk::descriptor::IntoWalletDescriptor;
use bdk::keys::bip39::{Language, Mnemonic};
use bdk::keys::{DerivableKey, ExtendedKey};
use bdk::wallet::AddressIndex;
pub use bdk::BlockTime;
use bdk::{FeeRate, KeychainKind, LocalUtxo, SignOptions, SyncOptions, Wallet as BdkWallet};
use bitcoin::hashes::{sha256, Hash as Sha256Hash};
use bitcoin::psbt::PartiallySignedTransaction;
use bitcoin::{Address, OutPoint};
use bitcoin::{ScriptBuf, Txid};
use bp::seals::txout::blind::ChainBlindSeal;
use bp::seals::txout::{CloseMethod, ExplicitSeal};
use bp::Outpoint as RgbOutpoint;
use bp::Txid as BpTxid;
use electrum_client::{Client as ElectrumClient, ConfigBuilder, ElectrumApi, Param};
use futures::executor::block_on;
use reqwest::blocking::Client as RestClient;
use rgb::BlockchainResolver;
use rgb_core::validation::Validity;
use rgb_core::{Assign, Operation, Opout, SecretSeal, Transition};
use rgb_lib_migration::{Migrator, MigratorTrait};
use rgb_schemata::{cfa_rgb25, cfa_schema, nia_rgb20, nia_schema};
use rgbstd::containers::{Bindle, BuilderSeal, Transfer as RgbTransfer};
use rgbstd::contract::{ContractId, GenesisSeal, GraphSeal};
use rgbstd::interface::{rgb20, rgb25, ContractBuilder, ContractIface, Rgb20, Rgb25, TypedState};
use rgbstd::stl::{
    Amount, AssetNaming, Attachment, ContractData, Details, DivisibleAssetSpec, MediaType, Name,
    Precision, RicardianContract, Ticker, Timestamp,
};
use rgbstd::validation::ConsignmentApi;
use rgbstd::Txid as RgbTxid;
use rgbwallet::psbt::opret::OutputOpret;
use rgbwallet::psbt::{PsbtDbc, RgbExt, RgbInExt};
use rgbwallet::{Beneficiary, RgbInvoice, RgbTransport};
use sea_orm::{ActiveValue, ConnectOptions, Database, TryIntoModel};
use serde::{Deserialize, Serialize};
use slog::{debug, error, info, Logger};
use std::cmp::min;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::panic;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use strict_encoding::{tn, FieldName, TypeName};
use strict_types::value::StrictNum;
use strict_types::StrictVal;

use crate::api::Proxy;
use crate::database::entities::asset::{ActiveModel as DbAssetActMod, Model as DbAsset};
use crate::database::entities::asset_transfer::{
    ActiveModel as DbAssetTransferActMod, Model as DbAssetTransfer,
};
use crate::database::entities::batch_transfer::{
    ActiveModel as DbBatchTransferActMod, Model as DbBatchTransfer,
};
use crate::database::entities::coloring::{ActiveModel as DbColoringActMod, Model as DbColoring};
use crate::database::entities::transfer::{ActiveModel as DbTransferActMod, Model as DbTransfer};
use crate::database::entities::transfer_transport_endpoint::{
    ActiveModel as DbTransferTransportEndpointActMod, Model as DbTransferTransportEndpoint,
};
use crate::database::entities::transport_endpoint::{
    ActiveModel as DbTransportEndpointActMod, Model as DbTransportEndpoint,
};
use crate::database::entities::txo::{ActiveModel as DbTxoActMod, Model as DbTxo};
use crate::database::entities::wallet_transaction::ActiveModel as DbWalletTransactionActMod;
use crate::database::enums::{
    AssetSchema, ColoringType, RecipientType, TransferStatus, TransportType, WalletTransactionType,
};
use crate::database::{
    DbData, LocalRecipient, LocalRgbAllocation, LocalTransportEndpoint, LocalUnspent,
    RgbLibDatabase, TransferData,
};
use crate::error::{Error, InternalError};
use crate::utils::{
    calculate_descriptor_from_xprv, calculate_descriptor_from_xpub, get_txid, load_rgb_runtime,
    now, setup_logger, BitcoinNetwork, RgbRuntime, LOG_FILE,
};

const RGB_DB_NAME: &str = "rgb_db";
const BDK_DB_NAME: &str = "bdk_db";

pub(crate) const KEYCHAIN_RGB_OPRET: u8 = 9;
pub(crate) const KEYCHAIN_RGB_TAPRET: u8 = 10;
pub(crate) const KEYCHAIN_BTC: u8 = 1;

const ASSETS_DIR: &str = "assets";
const TRANSFER_DIR: &str = "transfers";
const TRANSFER_DATA_FILE: &str = "transfer_data.txt";
const SIGNED_PSBT_FILE: &str = "signed.psbt";
const CONSIGNMENT_FILE: &str = "consignment_out";
const CONSIGNMENT_RCV_FILE: &str = "rcv_compose.rgbc";
const MEDIA_FNAME: &str = "media";
const MIME_FNAME: &str = "mime";

const MIN_BTC_REQUIRED: u64 = 2000;

const OPRET_VBYTES: f32 = 43.0;

const NUM_KNOWN_SCHEMAS: usize = 2;

const UTXO_SIZE: u32 = 1000;
const UTXO_NUM: u8 = 5;

const MAX_TRANSPORT_ENDPOINTS: u8 = 3;

const MIN_FEE_RATE: f32 = 1.0;
const MAX_FEE_RATE: f32 = 1000.0;

const DURATION_SEND_TRANSFER: i64 = 3600;
const DURATION_RCV_TRANSFER: u32 = 86400;

const ELECTRUM_TIMEOUT: u8 = 4;
const PROXY_TIMEOUT: u8 = 90;

const PROXY_PROTOCOL_VERSION: &str = "0.2";

pub(crate) const SCHEMA_ID_NIA: &str =
    "urn:lnp-bp:sc:BEiLYE-am9WhTW1-oK8cpvw4-FEMtzMrf-mKocuGZn-qWK6YF#ginger-parking-nirvana";
pub(crate) const SCHEMA_ID_CFA: &str =
    "urn:lnp-bp:sc:4nfgJ2-jkeTRQuG-uTet6NSW-Fy1sFTU8-qqrN2uY2-j6S5rv#ravioli-justin-brave";

/// The interface of an asset
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum AssetIface {
    /// RGB20 interface
    RGB20,
    /// RGB25 interface
    RGB25,
}

impl AssetIface {
    fn to_typename(&self) -> TypeName {
        tn!(format!("{self:?}"))
    }

    fn get_asset_details(
        &self,
        wallet: &Wallet,
        asset: &DbAsset,
        assets_dir: PathBuf,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
    ) -> Result<AssetType, Error> {
        let mut data_paths = vec![];
        let asset_dir = assets_dir.join(asset.asset_id.clone());
        if asset_dir.is_dir() {
            for fp in fs::read_dir(asset_dir)? {
                let fpath = fp?.path();
                let file_path = fpath.join(MEDIA_FNAME).to_string_lossy().to_string();
                let mime = fs::read_to_string(fpath.join(MIME_FNAME))?;
                data_paths.push(Media { file_path, mime });
            }
        }
        let balance = wallet.database.get_asset_balance(
            asset.asset_id.clone(),
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )?;
        let issued_supply = asset.issued_supply.parse::<u64>().unwrap();
        Ok(match &self {
            AssetIface::RGB20 => AssetType::AssetNIA(AssetNIA {
                asset_id: asset.asset_id.clone(),
                asset_iface: self.clone(),
                ticker: asset.ticker.clone().unwrap(),
                name: asset.name.clone(),
                precision: asset.precision,
                issued_supply,
                timestamp: asset.timestamp,
                added_at: asset.added_at,
                balance,
                data_paths,
            }),
            AssetIface::RGB25 => AssetType::AssetCFA(AssetCFA {
                asset_id: asset.asset_id.clone(),
                asset_iface: self.clone(),
                description: asset.description.clone(),
                name: asset.name.clone(),
                precision: asset.precision,
                issued_supply,
                timestamp: asset.timestamp,
                added_at: asset.added_at,
                balance,
                data_paths,
            }),
        })
    }
}

impl From<AssetSchema> for AssetIface {
    fn from(x: AssetSchema) -> AssetIface {
        match x {
            AssetSchema::Nia => AssetIface::RGB20,
            AssetSchema::Cfa => AssetIface::RGB25,
        }
    }
}

impl TryFrom<TypeName> for AssetIface {
    type Error = Error;

    fn try_from(value: TypeName) -> Result<Self, Self::Error> {
        match value.to_string().as_str() {
            "RGB20" => Ok(AssetIface::RGB20),
            "RGB25" => Ok(AssetIface::RGB25),
            _ => Err(Error::UnknownRgbInterface {
                interface: value.to_string(),
            }),
        }
    }
}

/// An RGB20 fungible asset
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct AssetNIA {
    /// ID of the asset
    pub asset_id: String,
    /// Asset interface type
    pub asset_iface: AssetIface,
    /// Ticker of the asset
    pub ticker: String,
    /// Name of the asset
    pub name: String,
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
    /// List of asset data file paths
    pub data_paths: Vec<Media>,
}

impl AssetNIA {
    fn get_asset_details(
        wallet: &Wallet,
        asset: &DbAsset,
        assets_dir: PathBuf,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
    ) -> Result<AssetNIA, Error> {
        match AssetIface::RGB20.get_asset_details(
            wallet,
            asset,
            assets_dir,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )? {
            AssetType::AssetNIA(asset) => Ok(asset),
            _ => unreachable!("impossible"),
        }
    }
}

/// An asset media file
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Media {
    /// Path of the media file
    pub file_path: String,
    /// Mime of the media file
    pub mime: String,
}

/// Metadata of an asset
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Metadata {
    /// Asset interface type
    pub asset_iface: AssetIface,
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
    /// Asset description
    pub description: Option<String>,
}

/// An RGB25 collectible asset
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct AssetCFA {
    /// ID of the asset
    pub asset_id: String,
    /// Asset interface type
    pub asset_iface: AssetIface,
    /// Name of the asset
    pub name: String,
    /// Description of the asset
    pub description: Option<String>,
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
    /// List of asset data file paths
    pub data_paths: Vec<Media>,
}

impl AssetCFA {
    fn get_asset_details(
        wallet: &Wallet,
        asset: &DbAsset,
        assets_dir: PathBuf,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
    ) -> Result<AssetCFA, Error> {
        match AssetIface::RGB25.get_asset_details(
            wallet,
            asset,
            assets_dir,
            asset_transfers,
            batch_transfers,
            colorings,
            txos,
        )? {
            AssetType::AssetCFA(asset) => Ok(asset),
            _ => unreachable!("impossible"),
        }
    }
}

enum AssetType {
    AssetNIA(AssetNIA),
    AssetCFA(AssetCFA),
}

/// List of known assets, grouped by asset interface
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Assets {
    /// List of RGB20 assets
    pub nia: Option<Vec<AssetNIA>>,
    /// List of RGB25 assets
    pub cfa: Option<Vec<AssetCFA>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AssetSpend {
    txo_map: HashMap<i32, u64>,
    input_outpoints: Vec<BdkOutPoint>,
    change_amount: u64,
}

/// A balance
///
/// The settled balance for the vanilla wallet includes the confirmed balance.
/// The settled balance for the colored wallet includes allocations created by operations that have
/// completed and are in a final status.
/// The future balance for the vanilla wallet also includes the immature balance and the untrusted
/// and trusted pending balances.
/// The future balance for the colored wallet also includes operations that have not yet completed
/// or are not yet final.
/// The spendable balance for the vanilla wallet includes the settled balance and also the
/// untrusted and trusted pending balances.
/// The spendable balance for the colored wallet is a subset of the settled balance, excluding
/// allocations on UTXOs that are supporting any pending operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Balance {
    /// Settled balance
    pub settled: u64,
    /// Future balance
    pub future: u64,
    /// Spendable balance
    pub spendable: u64,
}

/// The bitcoin balances for the vanilla and colored wallets
#[derive(Debug)]
pub struct BtcBalance {
    /// Extra funds that will never hold RGB assets
    pub vanilla: Balance,
    /// Funds that may hold RGB assets
    pub colored: Balance,
}

/// Data to receive an asset
#[derive(Debug, Deserialize, Serialize)]
pub struct ReceiveData {
    /// Invoice string
    pub invoice: String,
    /// ID of the receive operation (blinded UTXO or script)
    pub recipient_id: String,
    /// Expiration of the receive operation
    pub expiration_timestamp: Option<i64>,
}

/// An RGB blinded UTXO
#[derive(Debug)]
pub struct BlindedUTXO {
    /// Blinded UTXO
    pub blinded_utxo: String,
}

impl BlindedUTXO {
    /// Check that the provided [`BlindedUTXO::blinded_utxo`] is valid
    pub fn new(blinded_utxo: String) -> Result<Self, Error> {
        SecretSeal::from_str(&blinded_utxo).map_err(|e| Error::InvalidBlindedUTXO {
            details: e.to_string(),
        })?;
        Ok(BlindedUTXO { blinded_utxo })
    }
}

/// An RGB transport endpoint
#[derive(Debug)]
pub struct TransportEndpoint {
    /// Endpoint address
    pub endpoint: String,
    /// Endpoint transport type
    pub transport_type: TransportType,
}

impl TransportEndpoint {
    /// Check that the provided [`TransportEndpoint::endpoint`] is valid
    pub fn new(transport_endpoint: String) -> Result<Self, Error> {
        let rgb_transport = RgbTransport::from_str(&transport_endpoint)?;
        TransportEndpoint::try_from(rgb_transport)
    }

    /// Return the transport type of this transport endpoint
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

/// Supported database types
#[derive(Clone, Deserialize, Serialize)]
pub enum DatabaseType {
    /// A SQLite database
    Sqlite,
}

#[derive(Debug, Deserialize, Serialize)]
struct InfoBatchTransfer {
    change_utxo_idx: Option<i32>,
    blank_allocations: HashMap<String, u64>,
    donation: bool,
    min_confirmations: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct InfoAssetTransfer {
    recipients: Vec<LocalRecipient>,
    asset_spend: AssetSpend,
    asset_iface: AssetIface,
}

/// An RGB invoice
#[derive(Debug)]
pub struct Invoice {
    /// The RGB invoice string
    invoice_string: String,
    /// The decoded RGB invoice
    invoice_data: InvoiceData,
}

impl Invoice {
    /// Parse the provided [`Invoice::invoice_string`].
    /// Throws an error if the provided string is not a valid RGB invoice.
    pub fn new(invoice_string: String) -> Result<Self, Error> {
        let decoded = RgbInvoice::from_str(&invoice_string).map_err(|e| Error::InvalidInvoice {
            details: e.to_string(),
        })?;
        let asset_id = decoded.contract.map(|cid| cid.to_string());
        let amount = match decoded.owned_state {
            TypedState::Amount(v) => Some(v),
            _ => None,
        };
        let recipient_id = match decoded.beneficiary {
            Beneficiary::BlindedSeal(concealed_seal) => concealed_seal.to_string(),
            Beneficiary::WitnessUtxo(address) => address.script_pubkey().to_hex_string(),
        };
        let asset_iface = if let Some(iface) = decoded.iface {
            Some(AssetIface::try_from(iface)?)
        } else {
            None
        };
        let transport_endpoints: Vec<String> =
            decoded.transports.iter().map(|t| t.to_string()).collect();
        let invoice_data = InvoiceData {
            recipient_id,
            asset_iface,
            asset_id,
            amount,
            expiration_timestamp: decoded.expiry,
            transport_endpoints,
            network: decoded.chain.map(|c| c.into()),
        };

        Ok(Invoice {
            invoice_string,
            invoice_data,
        })
    }

    /// Parse the provided [`Invoice::invoice_data`].
    /// Throws an error if the provided data is invalid.
    pub fn from_invoice_data(invoice_data: InvoiceData) -> Result<Self, Error> {
        let concealed_seal = SecretSeal::from_str(&invoice_data.recipient_id);
        let script_buf = ScriptBuf::from_hex(&invoice_data.recipient_id);
        if concealed_seal.is_err() && script_buf.is_err() {
            return Err(Error::InvalidRecipientID);
        }
        let beneficiary = if let Ok(concealed_seal) = concealed_seal {
            concealed_seal.into()
        } else if let Some(network) = invoice_data.network {
            let address =
                Address::from_script(script_buf.unwrap().as_script(), network.into()).unwrap();
            Beneficiary::WitnessUtxo(address)
        } else {
            return Err(Error::InvalidInvoiceData {
                details: s!("cannot provide a script recipient without a network"),
            });
        };

        let contract = if let Some(cid) = invoice_data.asset_id.clone() {
            let contract_id =
                ContractId::from_str(&cid).map_err(|_| Error::InvalidAssetID { asset_id: cid })?;
            Some(contract_id)
        } else {
            None
        };
        let mut transports = vec![];
        for endpoint in invoice_data.transport_endpoints.clone() {
            transports.push(RgbTransport::from_str(&endpoint)?);
        }
        let owned_state = if let Some(value) = invoice_data.amount {
            TypedState::Amount(value)
        } else {
            TypedState::Void
        };
        let invoice = RgbInvoice {
            transports,
            contract,
            iface: invoice_data.asset_iface.as_ref().map(|i| i.to_typename()),
            operation: None,
            assignment: None,
            beneficiary,
            owned_state,
            chain: invoice_data.network.map(|n| n.into()),
            expiry: invoice_data.expiration_timestamp,
            unknown_query: none!(),
        };

        let invoice_string = invoice.to_string();

        Ok(Invoice {
            invoice_string,
            invoice_data,
        })
    }

    /// Return the data associated with this [`Invoice`]
    pub fn invoice_data(&self) -> InvoiceData {
        self.invoice_data.clone()
    }

    /// Return the string associated with this [`Invoice`]
    pub fn invoice_string(&self) -> String {
        self.invoice_string.clone()
    }
}

/// A decoded RGB invoice
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct InvoiceData {
    /// Recipient ID
    pub recipient_id: String,
    /// RGB interface
    pub asset_iface: Option<AssetIface>,
    /// RGB asset ID
    pub asset_id: Option<String>,
    /// RGB amount
    pub amount: Option<u64>,
    /// Bitcoin network
    pub network: Option<BitcoinNetwork>,
    /// Invoice expiration
    pub expiration_timestamp: Option<i64>,
    /// Transport endpoints
    pub transport_endpoints: Vec<String>,
}

/// Data for operations that require the wallet to be online
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Online {
    /// ID to tell different Online structs apart
    pub id: u64,
    /// URL of the electrum server to be used for online operations
    pub electrum_url: String,
}

struct OnlineData {
    id: u64,
    bdk_blockchain: ElectrumBlockchain,
    electrum_url: String,
    electrum_client: ElectrumClient,
}

/// Bitcoin transaction outpoint
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
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

impl From<BdkOutPoint> for Outpoint {
    fn from(x: BdkOutPoint) -> Outpoint {
        Outpoint {
            txid: x.txid.to_string(),
            vout: x.vout,
        }
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

impl From<Outpoint> for BdkOutPoint {
    fn from(x: Outpoint) -> BdkOutPoint {
        BdkOutPoint::from_str(&x.to_string()).expect("outpoint should be parsable")
    }
}

impl From<Outpoint> for OutPoint {
    fn from(x: Outpoint) -> OutPoint {
        OutPoint::from_str(&x.to_string()).expect("outpoint should be parsable")
    }
}

impl From<Outpoint> for RgbOutpoint {
    fn from(x: Outpoint) -> RgbOutpoint {
        RgbOutpoint::new(RgbTxid::from_str(&x.txid).unwrap(), x.vout)
    }
}

/// An RGB recipient
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Recipient {
    /// Recipient data
    pub recipient_data: RecipientData,
    /// RGB amount
    pub amount: u64,
    /// Transport endpoints
    pub transport_endpoints: Vec<String>,
}

impl Recipient {
    fn recipient_id(&self) -> String {
        self.recipient_data.recipient_id()
    }
}

/// The information needed to receive RGB assets
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum RecipientData {
    /// A blinded UTXO
    BlindedUTXO(SecretSeal),
    /// Witness data
    WitnessData {
        /// The script
        script_buf: ScriptBuf,
        /// The Bitcoin amount
        amount_sat: u64,
        /// An optional blinding
        blinding: Option<u64>,
    },
}

impl RecipientData {
    pub(crate) fn recipient_id(&self) -> String {
        match &self {
            RecipientData::BlindedUTXO(secret_seal) => secret_seal.to_string(),
            RecipientData::WitnessData { script_buf, .. } => script_buf.to_hex_string(),
        }
    }
}

/// A transfer refresh filter
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct RefreshFilter {
    /// Transfer status
    pub status: RefreshTransferStatus,
    /// Whether the transfer is incoming
    pub incoming: bool,
}

/// The pending status of a [`Transfer`] (eligible for refresh)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum RefreshTransferStatus {
    /// Waiting for the counterparty to take action
    WaitingCounterparty = 1,
    /// Waiting for the transfer transaction to be confirmed
    WaitingConfirmations = 2,
}

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

/// An RGB allocation
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub struct RgbAllocation {
    /// Asset ID
    pub asset_id: Option<String>,
    /// RGB amount
    pub amount: u64,
    /// Defines if the allocation is settled
    pub settled: bool,
}

impl From<LocalRgbAllocation> for RgbAllocation {
    fn from(x: LocalRgbAllocation) -> RgbAllocation {
        RgbAllocation {
            asset_id: x.asset_id.clone(),
            amount: x.amount,
            settled: x.settled(),
        }
    }
}

/// A bitcoin transaction
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Transaction {
    /// Type of transaction
    pub transaction_type: TransactionType,
    /// Transaction id
    pub txid: String,
    /// Received value (sats)
    /// Sum of owned outputs of this transaction.
    pub received: u64,
    /// Sent value (sats)
    /// Sum of owned inputs of this transaction.
    pub sent: u64,
    /// Fee value (sats) if confirmed.
    pub fee: Option<u64>,
    /// If the transaction is confirmed, contains height and Unix timestamp of the block containing the
    /// transaction, unconfirmed transaction contains `None`.
    pub confirmation_time: Option<BlockTime>,
}

/// The type of a transaction
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

/// An RGB transfer
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Transfer {
    /// ID of the transfer
    pub idx: i32,
    /// Timestamp of the transfer creation
    pub created_at: i64,
    /// Timestamp of the transfer last update
    pub updated_at: i64,
    /// Status of the transfer
    pub status: TransferStatus,
    /// Amount
    pub amount: u64,
    /// Type of the transfer
    pub kind: TransferKind,
    /// Txid of the transfer
    pub txid: Option<String>,
    /// Recipient ID (blinded UTXO or script) of an incoming transfer
    pub recipient_id: Option<String>,
    /// UTXO of an incoming transfer
    pub receive_utxo: Option<Outpoint>,
    /// Change UTXO of an outgoing transfer
    pub change_utxo: Option<Outpoint>,
    /// Expiration of the transfer
    pub expiration: Option<i64>,
    /// Transport endpoints of the transfer
    pub transport_endpoints: Vec<TransferTransportEndpoint>,
}

impl Transfer {
    fn from_db_transfer(
        x: &DbTransfer,
        td: TransferData,
        transport_endpoints: Vec<TransferTransportEndpoint>,
    ) -> Transfer {
        Transfer {
            idx: x.idx,
            created_at: td.created_at,
            updated_at: td.updated_at,
            status: td.status,
            amount: x
                .amount
                .parse::<u64>()
                .expect("DB should contain a valid u64 value"),
            kind: td.kind,
            txid: td.txid,
            recipient_id: x.recipient_id.clone(),
            receive_utxo: td.receive_utxo,
            change_utxo: td.change_utxo,
            expiration: td.expiration,
            transport_endpoints,
        }
    }
}

/// An RGB transfer transport endpoint
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// The type of an RGB transfer
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum TransferKind {
    /// A transfer that issued the asset
    Issuance,
    /// An incoming transfer via blinded UTXO
    ReceiveBlind,
    /// An incoming transfer via witness TX
    ReceiveWitness,
    /// An outgoing transfer
    Send,
}

/// A wallet unspent
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Unspent {
    /// Bitcoin UTXO
    pub utxo: Utxo,
    /// RGB allocations on the UTXO
    pub rgb_allocations: Vec<RgbAllocation>,
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
        }
    }
}

impl From<LocalUtxo> for Unspent {
    fn from(x: LocalUtxo) -> Unspent {
        Unspent {
            utxo: Utxo::from(x),
            rgb_allocations: vec![],
        }
    }
}

/// An unspent transaction output
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Utxo {
    /// UTXO outpoint
    pub outpoint: Outpoint,
    /// Amount held in satoshi
    pub btc_amount: u64,
    /// Defines if the UTXO can have RGB allocations
    pub colorable: bool,
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
        }
    }
}

impl From<LocalUtxo> for Utxo {
    fn from(x: LocalUtxo) -> Utxo {
        Utxo {
            outpoint: Outpoint::from(x.outpoint),
            btc_amount: x.txout.value,
            colorable: false,
        }
    }
}

/// Wallet data provided by the user
#[derive(Clone, Deserialize, Serialize)]
pub struct WalletData {
    /// Directory where the wallet directory is to be created
    pub data_dir: String,
    /// Bitcoin network for the wallet
    pub bitcoin_network: BitcoinNetwork,
    /// Database type for the wallet
    pub database_type: DatabaseType,
    /// The max number of RGB allocations allowed per UTXO
    pub max_allocations_per_utxo: u32,
    /// Wallet xPub
    pub pubkey: String,
    /// Wallet mnemonic phrase
    pub mnemonic: Option<String>,
    /// Optional keychain index for the vanilla wallet (default: 1)
    pub vanilla_keychain: Option<u8>,
}

/// An RGB wallet
///
/// A `Wallet` struct holds all the data required to operate it
pub struct Wallet {
    wallet_data: WalletData,
    logger: Logger,
    watch_only: bool,
    database: Arc<RgbLibDatabase>,
    wallet_dir: PathBuf,
    bdk_wallet: BdkWallet<AnyDatabase>,
    rest_client: RestClient,
    max_allocations_per_utxo: u32,
    online_data: Option<OnlineData>,
}

impl Wallet {
    /// Create a new RGB wallet based on the provided [`WalletData`]
    pub fn new(wallet_data: WalletData) -> Result<Self, Error> {
        let wdata = wallet_data.clone();

        // wallet directory and file logging setup
        let pubkey = ExtendedPubKey::from_str(&wdata.pubkey)?;
        let extended_key: ExtendedKey = ExtendedKey::from(pubkey);
        let bdk_network = BdkNetwork::from(wdata.bitcoin_network);
        let xpub = extended_key.into_xpub(bdk_network, &Secp256k1::new());
        let fingerprint = xpub.fingerprint().to_string();
        let absolute_data_dir = fs::canonicalize(wdata.data_dir)?;
        let data_dir_path = Path::new(&absolute_data_dir);
        let wallet_dir = data_dir_path.join(fingerprint);
        if !data_dir_path.exists() {
            return Err(Error::InexistentDataDir)?;
        }
        if !wallet_dir.exists() {
            fs::create_dir(wallet_dir.clone())?;
        }
        let logger = setup_logger(wallet_dir.clone(), None)?;
        info!(logger.clone(), "New wallet in '{:?}'", wallet_dir);
        let panic_logger = logger.clone();
        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            error!(panic_logger.clone(), "PANIC: {:?}", info);
            prev_hook(info);
        }));

        // BDK setup
        let vanilla_keychain = wdata.vanilla_keychain.unwrap_or(KEYCHAIN_BTC);
        if [KEYCHAIN_RGB_OPRET, KEYCHAIN_RGB_TAPRET].contains(&vanilla_keychain) {
            return Err(Error::InvalidVanillaKeychain);
        }
        let bdk_db = wallet_dir.join(BDK_DB_NAME);
        let bdk_config = SledDbConfiguration {
            path: bdk_db
                .into_os_string()
                .into_string()
                .expect("should be possible to convert path to a string"),
            tree_name: BDK_DB_NAME.to_string(),
        };
        let bdk_database =
            AnyDatabase::from_config(&bdk_config.into()).map_err(InternalError::from)?;
        let watch_only = wdata.mnemonic.is_none();
        let bdk_wallet = if let Some(mnemonic) = wdata.mnemonic {
            let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)?;
            let xkey: ExtendedKey = mnemonic
                .clone()
                .into_extended_key()
                .expect("a valid key should have been provided");
            let xpub_from_mnemonic = &xkey.into_xpub(bdk_network, &Secp256k1::new());
            if *xpub_from_mnemonic != xpub {
                return Err(Error::InvalidBitcoinKeys);
            }
            let xkey: ExtendedKey = mnemonic
                .into_extended_key()
                .expect("a valid key should have been provided");
            let xprv = xkey
                .into_xprv(bdk_network)
                .expect("should be possible to get an extended private key");
            let descriptor =
                calculate_descriptor_from_xprv(xprv, wdata.bitcoin_network, KEYCHAIN_RGB_OPRET);
            let change_descriptor =
                calculate_descriptor_from_xprv(xprv, wdata.bitcoin_network, vanilla_keychain);
            BdkWallet::new(
                &descriptor,
                Some(&change_descriptor),
                bdk_network,
                bdk_database,
            )
            .map_err(InternalError::from)?
        } else {
            let descriptor_pub =
                calculate_descriptor_from_xpub(xpub, wdata.bitcoin_network, KEYCHAIN_RGB_OPRET)?;
            let change_descriptor_pub =
                calculate_descriptor_from_xpub(xpub, wdata.bitcoin_network, vanilla_keychain)?;
            BdkWallet::new(
                &descriptor_pub,
                Some(&change_descriptor_pub),
                bdk_network,
                bdk_database,
            )
            .map_err(InternalError::from)?
        };

        // RGB setup
        let mut runtime = load_rgb_runtime(wallet_dir.clone(), wdata.bitcoin_network)?;
        if runtime.schema_ids()?.len() < NUM_KNOWN_SCHEMAS {
            runtime.import_iface(rgb20())?;
            runtime.import_schema(nia_schema())?;
            runtime.import_iface_impl(nia_rgb20())?;

            runtime.import_iface(rgb25())?;
            runtime.import_schema(cfa_schema())?;
            runtime.import_iface_impl(cfa_rgb25())?;
        }

        // RGB-LIB setup
        let db_path = wallet_dir.join(RGB_DB_NAME);
        let connection_string = format!("sqlite://{}?mode=rwc", db_path.as_path().display());
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
        let rest_client = RestClient::builder()
            .timeout(Duration::from_secs(PROXY_TIMEOUT as u64))
            .build()?;

        info!(logger, "New wallet completed");
        Ok(Wallet {
            wallet_data,
            logger,
            watch_only,
            database: Arc::new(database),
            wallet_dir,
            bdk_wallet,
            rest_client,
            max_allocations_per_utxo: wdata.max_allocations_per_utxo,
            online_data: None,
        })
    }

    fn _bdk_blockchain(&self) -> Result<&ElectrumBlockchain, InternalError> {
        match self.online_data {
            Some(ref x) => Ok(&x.bdk_blockchain),
            None => Err(InternalError::Unexpected),
        }
    }

    fn _bitcoin_network(&self) -> BitcoinNetwork {
        self.wallet_data.bitcoin_network
    }

    fn _electrum_client(&self) -> Result<&ElectrumClient, InternalError> {
        match self.online_data {
            Some(ref x) => Ok(&x.electrum_client),
            None => Err(InternalError::Unexpected),
        }
    }

    fn _blockchain_resolver(&self) -> Result<BlockchainResolver, Error> {
        Ok(BlockchainResolver::with(
            &self.online_data.as_ref().unwrap().electrum_url,
        )?)
    }

    fn _rgb_runtime(&self) -> Result<RgbRuntime, Error> {
        load_rgb_runtime(self.wallet_dir.clone(), self._bitcoin_network())
    }

    fn _get_tx_details(
        &self,
        txid: String,
        electrum_client: Option<&ElectrumClient>,
    ) -> Result<serde_json::Value, Error> {
        let electrum_client = if let Some(client) = electrum_client {
            client
        } else {
            self._electrum_client()?
        };
        electrum_client
            .raw_call(
                "blockchain.transaction.get",
                vec![Param::String(txid), Param::Bool(true)],
            )
            .map_err(|e| Error::InvalidElectrum {
                details: e.to_string(),
            })
    }

    fn _check_transport_endpoints(&self, transport_endpoints: &Vec<String>) -> Result<(), Error> {
        if transport_endpoints.is_empty() {
            return Err(Error::InvalidTransportEndpoints {
                details: s!("must provide at least a transport endpoint"),
            });
        }
        if transport_endpoints.len() > MAX_TRANSPORT_ENDPOINTS as usize {
            return Err(Error::InvalidTransportEndpoints {
                details: format!(
                    "library supports at max {MAX_TRANSPORT_ENDPOINTS} transport endpoints"
                ),
            });
        }

        Ok(())
    }

    fn _check_fee_rate(&self, fee_rate: f32) -> Result<(), Error> {
        if fee_rate < MIN_FEE_RATE {
            return Err(Error::InvalidFeeRate {
                details: format!("value under minimum {MIN_FEE_RATE}"),
            });
        } else if fee_rate > MAX_FEE_RATE {
            return Err(Error::InvalidFeeRate {
                details: format!("value above maximum {MAX_FEE_RATE}"),
            });
        }
        Ok(())
    }

    fn _sync_wallet<D>(&self, wallet: &BdkWallet<D>) -> Result<(), Error>
    where
        D: BatchDatabase,
    {
        self._sync_wallet_with_blockchain(wallet, self._bdk_blockchain()?)?;
        Ok(())
    }

    fn _sync_wallet_with_blockchain<D>(
        &self,
        wallet: &BdkWallet<D>,
        bdk_blockchain: &ElectrumBlockchain,
    ) -> Result<(), Error>
    where
        D: BatchDatabase,
    {
        wallet
            .sync(bdk_blockchain, SyncOptions { progress: None })
            .map_err(|e| Error::FailedBdkSync {
                details: e.to_string(),
            })?;
        Ok(())
    }

    fn _sync_db_txos_with_blockchain(
        &self,
        bdk_blockchain: &ElectrumBlockchain,
    ) -> Result<(), Error> {
        debug!(self.logger, "Syncing TXOs...");
        self._sync_wallet_with_blockchain(&self.bdk_wallet, bdk_blockchain)?;

        let db_outpoints: Vec<String> = self
            .database
            .iter_txos()?
            .into_iter()
            .filter(|t| !t.spent)
            .map(|u| u.outpoint().to_string())
            .collect();
        let bdk_utxos: Vec<LocalUtxo> = self
            .bdk_wallet
            .list_unspent()
            .map_err(InternalError::from)?;
        let new_utxos: Vec<DbTxoActMod> = bdk_utxos
            .into_iter()
            .filter(|u| u.keychain == KeychainKind::External)
            .filter(|u| !db_outpoints.contains(&u.outpoint.to_string()))
            .map(DbTxoActMod::from)
            .collect();
        for new_utxo in new_utxos.iter().cloned() {
            self.database.set_txo(new_utxo)?;
        }

        Ok(())
    }

    fn _sync_db_txos(&self) -> Result<(), Error> {
        self._sync_db_txos_with_blockchain(self._bdk_blockchain()?)?;
        Ok(())
    }

    fn _internal_unspents(&self) -> Result<impl Iterator<Item = LocalUtxo>, Error> {
        Ok(self
            .bdk_wallet
            .list_unspent()
            .map_err(InternalError::from)?
            .into_iter()
            .filter(|u| u.keychain == KeychainKind::Internal))
    }

    fn _broadcast_psbt(&self, signed_psbt: BdkPsbt) -> Result<BdkTransaction, Error> {
        let tx = signed_psbt.extract_tx();
        self._bdk_blockchain()?
            .broadcast(&tx)
            .map_err(|e| Error::FailedBroadcast {
                details: e.to_string(),
            })?;
        debug!(self.logger, "Broadcasted TX with ID '{}'", tx.txid());

        let internal_unspents_outpoints: Vec<(String, u32)> = self
            ._internal_unspents()?
            .map(|u| (u.outpoint.txid.to_string(), u.outpoint.vout))
            .collect();

        for input in tx.clone().input {
            let txid = input.previous_output.txid.to_string();
            let vout = input.previous_output.vout;
            if internal_unspents_outpoints.contains(&(txid.clone(), vout)) {
                continue;
            }
            let mut db_txo: DbTxoActMod = self
                .database
                .get_txo(Outpoint { txid, vout })?
                .expect("outpoint should be in the DB")
                .into();
            db_txo.spent = ActiveValue::Set(true);
            self.database.update_txo(db_txo)?;
        }

        self._sync_db_txos()?;

        Ok(tx)
    }

    fn _check_online(&self, online: Online) -> Result<(), Error> {
        if let Some(online_data) = &self.online_data {
            if online_data.id != online.id || online_data.electrum_url != online.electrum_url {
                error!(self.logger, "Cannot change online object");
                return Err(Error::CannotChangeOnline);
            }
        } else {
            error!(self.logger, "Wallet is offline");
            return Err(Error::Offline);
        }
        Ok(())
    }

    fn _check_xprv(&self) -> Result<(), Error> {
        if self.watch_only {
            error!(self.logger, "Invalid operation for a watch only wallet");
            return Err(Error::WatchOnly);
        }
        Ok(())
    }

    fn _get_uncolorable_btc_sum(&self) -> Result<u64, Error> {
        Ok(self._internal_unspents()?.map(|u| u.txout.value).sum())
    }

    fn _handle_expired_transfers(&mut self, db_data: &mut DbData) -> Result<(), Error> {
        self._sync_db_txos()?;
        let now = now().unix_timestamp();
        let expired_transfers: Vec<DbBatchTransfer> = db_data
            .batch_transfers
            .clone()
            .into_iter()
            .filter(|t| t.waiting_counterparty() && t.expiration.unwrap_or(now) < now)
            .collect();
        for transfer in expired_transfers.iter() {
            let updated_batch_transfer = self._refresh_transfer(transfer, db_data, &vec![])?;
            if updated_batch_transfer.is_none() {
                let mut updated_batch_transfer: DbBatchTransferActMod = transfer.clone().into();
                updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
                self.database
                    .update_batch_transfer(&mut updated_batch_transfer)?;
            }
        }
        Ok(())
    }

    fn _get_available_allocations(
        &self,
        unspents: Vec<LocalUnspent>,
        exclude_utxos: Vec<Outpoint>,
        max_allocations: Option<u32>,
    ) -> Result<Vec<LocalUnspent>, Error> {
        let mut mut_unspents = unspents;
        mut_unspents
            .iter_mut()
            .for_each(|u| u.rgb_allocations.retain(|a| !a.status.failed()));
        let max_allocs = max_allocations.unwrap_or(self.max_allocations_per_utxo - 1);
        Ok(mut_unspents
            .iter()
            .filter(|u| !exclude_utxos.contains(&u.utxo.outpoint()))
            .filter(|u| {
                (u.rgb_allocations.len() as u32) <= max_allocs
                    && !u
                        .rgb_allocations
                        .iter()
                        .any(|a| !a.incoming && a.status.waiting_counterparty())
            })
            .cloned()
            .collect())
    }

    fn _detect_btc_unspendable_err(&self) -> Result<Error, Error> {
        let available = self._get_uncolorable_btc_sum()?;
        Ok(if available < MIN_BTC_REQUIRED {
            Error::InsufficientBitcoins {
                needed: MIN_BTC_REQUIRED,
                available,
            }
        } else {
            Error::InsufficientAllocationSlots
        })
    }

    fn _get_utxo(
        &self,
        exclude_utxos: Vec<Outpoint>,
        unspents: Option<Vec<LocalUnspent>>,
        pending_operation: bool,
    ) -> Result<DbTxo, Error> {
        let unspents = if let Some(u) = unspents {
            u
        } else {
            self.database.get_rgb_allocations(
                self.database.get_unspent_txos(vec![])?,
                None,
                None,
                None,
            )?
        };
        let mut allocatable = self._get_available_allocations(unspents, exclude_utxos, None)?;
        allocatable.sort_by_key(|t| t.rgb_allocations.len());
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
            None => Err(self._detect_btc_unspendable_err()?),
        }
    }

    fn _save_transfer_transport_endpoint(
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

    pub(crate) fn _get_asset_iface(
        &self,
        contract_id: ContractId,
        runtime: &RgbRuntime,
    ) -> Result<AssetIface, Error> {
        let genesis = runtime.genesis(contract_id)?;
        let schema_id = genesis.schema_id.to_string();
        Ok(match &schema_id[..] {
            SCHEMA_ID_NIA => AssetIface::RGB20,
            SCHEMA_ID_CFA => AssetIface::RGB25,
            _ => return Err(Error::UnknownRgbSchema { schema_id }),
        })
    }

    fn _receive(
        &self,
        asset_id: Option<String>,
        amount: Option<u64>,
        duration_seconds: Option<u32>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
        beneficiary: Beneficiary,
        recipient_type: RecipientType,
        recipient_id: String,
    ) -> Result<(String, Option<i64>, i32), Error> {
        let (iface, contract_id) = if let Some(aid) = asset_id.clone() {
            let asset = self.database.check_asset_exists(aid.clone())?;
            let contract_id = ContractId::from_str(&aid).expect("invalid contract ID");
            let asset_iface = AssetIface::from(asset.schema);
            let iface = asset_iface.to_typename();
            (Some(iface), Some(contract_id))
        } else {
            (None, None)
        };

        let created_at = now().unix_timestamp();
        let expiry = if duration_seconds == Some(0) {
            None
        } else {
            let duration_seconds = duration_seconds.unwrap_or(DURATION_RCV_TRANSFER) as i64;
            let expiry = created_at + duration_seconds;
            Some(expiry)
        };

        self._check_transport_endpoints(&transport_endpoints)?;
        let mut transport_endpoints_dedup = transport_endpoints.clone();
        transport_endpoints_dedup.sort();
        transport_endpoints_dedup.dedup();
        if transport_endpoints_dedup.len() != transport_endpoints.len() {
            return Err(Error::InvalidTransportEndpoints {
                details: s!("no duplicate transport endpoints allowed"),
            });
        }
        let mut endpoints: Vec<String> = vec![];
        let mut transports = vec![];
        for endpoint_str in transport_endpoints {
            let rgb_transport = RgbTransport::from_str(&endpoint_str)?;
            transports.push(rgb_transport.clone());
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

        let owned_state = if let Some(value) = amount {
            TypedState::Amount(value)
        } else {
            TypedState::Void
        };

        let invoice = RgbInvoice {
            transports,
            contract: contract_id,
            iface,
            operation: None,
            assignment: None,
            beneficiary,
            owned_state,
            chain: Some(self._bitcoin_network().into()),
            expiry,
            unknown_query: none!(),
        };

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
            amount: ActiveValue::Set(s!("0")),
            incoming: ActiveValue::Set(true),
            recipient_id: ActiveValue::Set(Some(recipient_id)),
            recipient_type: ActiveValue::Set(Some(recipient_type)),
            ..Default::default()
        };
        let transfer_idx = self.database.set_transfer(transfer)?;
        for endpoint in endpoints {
            self._save_transfer_transport_endpoint(
                transfer_idx,
                &LocalTransportEndpoint {
                    endpoint,
                    transport_type: TransportType::JsonRpc,
                    used: false,
                    usable: true,
                },
            )?;
        }

        Ok((invoice.to_string(), expiry, asset_transfer_idx))
    }

    /// Blind an UTXO to receive RGB assets and return the resulting [`ReceiveData`]
    ///
    /// Optional Asset ID and duration (in seconds) can be specified
    pub fn blind_receive(
        &mut self,
        asset_id: Option<String>,
        amount: Option<u64>,
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
        )?;
        unspents.retain(|u| {
            !(u.rgb_allocations
                .iter()
                .any(|a| !a.incoming && a.status.waiting_counterparty()))
        });
        let utxo = self._get_utxo(vec![], Some(unspents), true)?;
        debug!(
            self.logger,
            "Blinding outpoint '{}'",
            utxo.outpoint().to_string()
        );
        let seal = ExplicitSeal::with(
            CloseMethod::OpretFirst,
            RgbTxid::from_str(&utxo.txid).unwrap().into(),
            utxo.vout,
        );
        let seal = GraphSeal::from(seal);
        let concealed_seal = seal.to_concealed_seal();
        let blinded_utxo = concealed_seal.to_string();
        debug!(self.logger, "Recipient ID '{}'", blinded_utxo);

        let (invoice, expiration_timestamp, asset_transfer_idx) = self._receive(
            asset_id,
            amount,
            duration_seconds,
            transport_endpoints,
            min_confirmations,
            concealed_seal.into(),
            RecipientType::Blind,
            blinded_utxo.clone(),
        )?;

        let mut runtime = self._rgb_runtime()?;
        runtime.store_seal_secret(seal)?;

        let db_coloring = DbColoringActMod {
            txo_idx: ActiveValue::Set(utxo.idx),
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            coloring_type: ActiveValue::Set(ColoringType::Receive),
            amount: ActiveValue::Set(s!("0")),
            ..Default::default()
        };
        self.database.set_coloring(db_coloring)?;

        self.update_backup_info(false)?;

        info!(self.logger, "Blind receive completed");
        Ok(ReceiveData {
            invoice,
            recipient_id: blinded_utxo,
            expiration_timestamp,
        })
    }

    /// Create an address to receive RGB assets and return the resulting [`ReceiveData`]
    ///
    /// Optional Asset ID and duration (in seconds) can be specified
    pub fn witness_receive(
        &mut self,
        asset_id: Option<String>,
        amount: Option<u64>,
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

        let address_str = self._get_new_address().to_string();
        let address = Address::from_str(&address_str).unwrap().assume_checked();
        let script_buf_str = address.script_pubkey().to_hex_string();
        debug!(self.logger, "Recipient ID '{}'", script_buf_str);

        let (invoice, expiration_timestamp, _) = self._receive(
            asset_id,
            amount,
            duration_seconds,
            transport_endpoints,
            min_confirmations,
            Beneficiary::WitnessUtxo(address),
            RecipientType::Witness,
            script_buf_str.clone(),
        )?;

        self.update_backup_info(false)?;

        info!(self.logger, "Witness receive completed");
        Ok(ReceiveData {
            invoice,
            recipient_id: script_buf_str,
            expiration_timestamp,
        })
    }

    fn _sign_psbt(&self, psbt: &mut BdkPsbt) -> Result<(), Error> {
        self.bdk_wallet
            .sign(psbt, SignOptions::default())
            .map_err(InternalError::from)?;
        Ok(())
    }

    /// Sign a PSBT
    pub fn sign_psbt(&self, unsigned_psbt: String) -> Result<String, Error> {
        let mut psbt = BdkPsbt::from_str(&unsigned_psbt).map_err(InternalError::from)?;
        self._sign_psbt(&mut psbt)?;
        Ok(psbt.to_string())
    }

    fn _create_split_tx(
        &self,
        inputs: &[BdkOutPoint],
        num_utxos_to_create: u8,
        size: u32,
        fee_rate: f32,
    ) -> Result<BdkPsbt, bdk::Error> {
        let mut tx_builder = self.bdk_wallet.build_tx();
        tx_builder
            .add_utxos(inputs)?
            .manually_selected_only()
            .fee_rate(FeeRate::from_sat_per_vb(fee_rate));
        for _i in 0..num_utxos_to_create {
            tx_builder.add_recipient(self._get_new_address().script_pubkey(), size as u64);
        }
        Ok(tx_builder.finish()?.0)
    }

    /// Create new UTXOs. See the [`create_utxos_begin`](Wallet::create_utxos_begin) function for
    /// details.
    ///
    /// This is the full version, requiring a wallet with private keys and [`Online`] data
    pub fn create_utxos(
        &mut self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: f32,
    ) -> Result<u8, Error> {
        info!(self.logger, "Creating UTXOs...");
        self._check_xprv()?;

        let unsigned_psbt = self.create_utxos_begin(online.clone(), up_to, num, size, fee_rate)?;

        let psbt = self.sign_psbt(unsigned_psbt)?;

        self.create_utxos_end(online, psbt)
    }

    /// Prepare the PSBT to create new UTXOs to hold RGB allocations.
    ///
    /// If `up_to` is false, just create the required UTXOs.
    /// If `up_to` is true, create as many UTXOs as needed to reach the requested number or return
    /// an error if none need to be created.
    ///
    /// Providing the optional `num` parameter requests that many UTXOs, if it's not specified the
    /// default number is used.
    ///
    /// Providing the optional `size` parameter requests that UTXOs be created of that size, if it's
    /// not specified the default one is used.
    ///
    /// If not enough bitcoin funds are available to create the requested (or default) number of
    /// UTXOs, the number is decremented by one until it is possible to complete the operation. If
    /// the number reaches zero, an error is returned.
    ///
    /// This is the first half of the partial version, requiring no private keys nor [`Online`] data.
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`create_utxos_end`](Wallet::create_utxos_end) function.
    ///
    /// Returns a PSBT ready to be signed
    pub fn create_utxos_begin(
        &mut self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: f32,
    ) -> Result<String, Error> {
        info!(self.logger, "Creating UTXOs (begin)...");
        self._check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        self._sync_db_txos()?;

        let unspent_txos = self.database.get_unspent_txos(vec![])?;
        let unspents = self
            .database
            .get_rgb_allocations(unspent_txos, None, None, None)?;

        let mut utxos_to_create = num.unwrap_or(UTXO_NUM);
        if up_to {
            let allocatable = self
                ._get_available_allocations(unspents, vec![], None)?
                .len() as u8;
            if allocatable >= utxos_to_create {
                return Err(Error::AllocationsAlreadyAvailable);
            } else {
                utxos_to_create -= allocatable
            }
        }
        debug!(self.logger, "Will try to create {} UTXOs", utxos_to_create);

        let inputs: Vec<BdkOutPoint> = self._internal_unspents()?.map(|u| u.outpoint).collect();
        let inputs: &[BdkOutPoint] = &inputs;
        let usable_btc_amount = self._get_uncolorable_btc_sum()?;
        let utxo_size = size.unwrap_or(UTXO_SIZE);
        let max_possible_utxos = usable_btc_amount / utxo_size as u64;
        let mut btc_needed: u64 = (utxo_size as u64 * utxos_to_create as u64) + 1000;
        let mut btc_available: u64 = 0;
        let mut num_try_creating = min(utxos_to_create, max_possible_utxos as u8);
        while num_try_creating > 0 {
            match self._create_split_tx(inputs, num_try_creating, utxo_size, fee_rate) {
                Ok(_v) => break,
                Err(e) => {
                    (btc_needed, btc_available) = match e {
                        bdk::Error::InsufficientFunds { needed, available } => (needed, available),
                        _ => return Err(InternalError::Unexpected.into()),
                    };
                    num_try_creating -= 1
                }
            };
        }

        if num_try_creating == 0 {
            Err(Error::InsufficientBitcoins {
                needed: btc_needed,
                available: btc_available,
            })
        } else {
            info!(self.logger, "Create UTXOs completed");
            Ok(self
                ._create_split_tx(inputs, num_try_creating, utxo_size, fee_rate)
                .map_err(InternalError::from)?
                .to_string())
        }
    }

    /// Broadcast the provided PSBT to create new UTXOs.
    ///
    /// This is the second half of the partial version, requiring [`Online`] data but no private keys.
    /// The provided PSBT, prepared with the [`create_utxos_begin`](Wallet::create_utxos_begin)
    /// function, needs to have already been signed.
    ///
    /// Returns the number of created UTXOs
    pub fn create_utxos_end(&self, online: Online, signed_psbt: String) -> Result<u8, Error> {
        info!(self.logger, "Creating UTXOs (end)...");
        self._check_online(online)?;

        let signed_psbt = BdkPsbt::from_str(&signed_psbt)?;
        let tx = self._broadcast_psbt(signed_psbt)?;

        self.database
            .set_wallet_transaction(DbWalletTransactionActMod {
                txid: ActiveValue::Set(tx.txid().to_string()),
                wallet_transaction_type: ActiveValue::Set(WalletTransactionType::CreateUtxos),
                ..Default::default()
            })?;

        let mut num_utxos_created = 0;
        let bdk_utxos: Vec<LocalUtxo> = self
            .bdk_wallet
            .list_unspent()
            .map_err(InternalError::from)?;
        let txid = tx.txid();
        for utxo in bdk_utxos.into_iter() {
            if utxo.outpoint.txid == txid && utxo.keychain == KeychainKind::External {
                num_utxos_created += 1
            }
        }

        self.update_backup_info(false)?;

        info!(self.logger, "Create UTXOs completed");
        Ok(num_utxos_created)
    }

    fn _delete_batch_transfer(
        &self,
        batch_transfer: &DbBatchTransfer,
        asset_transfers: &Vec<DbAssetTransfer>,
    ) -> Result<(), Error> {
        for asset_transfer in asset_transfers {
            self.database.del_coloring(asset_transfer.idx)?;
        }
        Ok(self.database.del_batch_transfer(batch_transfer)?)
    }

    /// Delete eligible transfers from the database and return if any transfer has been deleted
    ///
    /// An optional `recipient_id` can be provided to operate on a single transfer.
    /// An optional `txid` can be provided to operate on a batch transfer.
    /// If both a `recipient_id` and a `txid` are provided, they need to belong to the same batch
    /// transfer or an error is returned.
    ///
    /// If either `recipient_id` or `txid` are provided and `no_asset_only` is true, transfers with
    /// an associated Asset ID will not be deleted and instead return an error.
    ///
    /// Eligible transfers are the ones in status [`TransferStatus::Failed`].
    pub fn delete_transfers(
        &self,
        recipient_id: Option<String>,
        txid: Option<String>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        info!(
            self.logger,
            "Deleting transfer with recipient ID {:?} and TXID {:?}...", recipient_id, txid
        );

        let db_data = self.database.get_db_data(false)?;
        let mut transfers_changed = false;

        if recipient_id.is_some() || txid.is_some() {
            let (batch_transfer, asset_transfers) = if let Some(recipient_id) = recipient_id {
                let db_transfer = &mut self
                    .database
                    .get_transfer_or_fail(recipient_id, &db_data.transfers)?;
                let (_, batch_transfer) = db_transfer
                    .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)?;
                let asset_transfers =
                    batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;
                let asset_transfer_ids: Vec<i32> = asset_transfers.iter().map(|t| t.idx).collect();
                if (db_data
                    .transfers
                    .into_iter()
                    .filter(|t| asset_transfer_ids.contains(&t.asset_transfer_idx))
                    .count()
                    > 1
                    || txid.is_some())
                    && txid != batch_transfer.txid
                {
                    return Err(Error::CannotDeleteTransfer);
                }
                (batch_transfer, asset_transfers)
            } else {
                let batch_transfer = self
                    .database
                    .get_batch_transfer_or_fail(txid.expect("TXID"), &db_data.batch_transfers)?;
                let asset_transfers =
                    batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;
                (batch_transfer, asset_transfers)
            };

            if !batch_transfer.failed() {
                return Err(Error::CannotDeleteTransfer);
            }

            if no_asset_only {
                let connected_assets = asset_transfers.iter().any(|t| t.asset_id.is_some());
                if connected_assets {
                    return Err(Error::CannotDeleteTransfer);
                }
            }

            transfers_changed = true;
            self._delete_batch_transfer(&batch_transfer, &asset_transfers)?
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
                self._delete_batch_transfer(batch_transfer, &asset_transfers)?
            }
        }

        if transfers_changed {
            self.update_backup_info(false)?;
        }

        info!(self.logger, "Delete transfer completed");
        Ok(transfers_changed)
    }

    /// Send bitcoin funds to the provided address. See the
    /// [`drain_to_begin`](Wallet::drain_to_begin) function for details.
    ///
    /// This is the full version, requiring a wallet with private keys and [`Online`] data
    pub fn drain_to(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
        fee_rate: f32,
    ) -> Result<String, Error> {
        info!(
            self.logger,
            "Draining to '{}' destroying asset '{}'...", address, destroy_assets
        );
        self._check_xprv()?;

        let unsigned_psbt =
            self.drain_to_begin(online.clone(), address, destroy_assets, fee_rate)?;

        let psbt = self.sign_psbt(unsigned_psbt)?;

        self.drain_to_end(online, psbt)
    }

    fn _get_unspendable_bdk_outpoints(&self) -> Result<Vec<BdkOutPoint>, Error> {
        Ok(self
            .database
            .iter_txos()?
            .into_iter()
            .map(BdkOutPoint::from)
            .collect())
    }

    /// Prepare the PSBT to send bitcoin funds not in use for RGB allocations, or all if
    /// `destroy_assets` is specified, to the provided `address`.
    ///
    /// Warning: setting `destroy_assets` to true is dangerous, only do this if you know what
    /// you're doing!
    ///
    /// This is the first half of the partial version, requiring no private keys.
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`drain_to_end`](Wallet::drain_to_end) function.
    ///
    /// Returns a PSBT ready to be signed
    pub fn drain_to_begin(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
        fee_rate: f32,
    ) -> Result<String, Error> {
        info!(
            self.logger,
            "Draining (begin) to '{}' destroying asset '{}'...", address, destroy_assets
        );
        self._check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        self._sync_db_txos()?;

        let address = BdkAddress::from_str(&address).map(|x| x.script_pubkey())?;

        let mut tx_builder = self.bdk_wallet.build_tx();
        tx_builder
            .drain_wallet()
            .drain_to(address)
            .fee_rate(FeeRate::from_sat_per_vb(fee_rate));

        if !destroy_assets {
            let unspendable = self._get_unspendable_bdk_outpoints()?;
            tx_builder.unspendable(unspendable);
        }

        let psbt = tx_builder
            .finish()
            .map_err(|e| match e {
                bdk::Error::InsufficientFunds { needed, available } => {
                    Error::InsufficientBitcoins { needed, available }
                }
                _ => Error::from(InternalError::from(e)),
            })?
            .0
            .to_string();

        info!(self.logger, "Drain (begin) completed");
        Ok(psbt)
    }

    /// Broadcast the provided PSBT to send bitcoin funds.
    ///
    /// This is the second half of the partial version, requiring [`Online`] data but no private keys.
    /// The provided PSBT, prepared with the [`drain_to_begin`](Wallet::drain_to_begin) function,
    /// needs to have already been signed.
    ///
    /// Returns the TXID of the transaction that's been broadcast
    pub fn drain_to_end(&self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger, "Draining (end)...");
        self._check_online(online)?;

        let signed_psbt = BdkPsbt::from_str(&signed_psbt)?;
        let tx = self._broadcast_psbt(signed_psbt)?;

        self.database
            .set_wallet_transaction(DbWalletTransactionActMod {
                txid: ActiveValue::Set(tx.txid().to_string()),
                wallet_transaction_type: ActiveValue::Set(WalletTransactionType::Drain),
                ..Default::default()
            })?;

        self.update_backup_info(false)?;

        info!(self.logger, "Drain (end) completed");
        Ok(tx.txid().to_string())
    }

    fn _fail_batch_transfer(&self, batch_transfer: &DbBatchTransfer) -> Result<(), Error> {
        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        updated_batch_transfer.expiration = ActiveValue::Set(Some(now().unix_timestamp()));
        self.database
            .update_batch_transfer(&mut updated_batch_transfer)?;

        Ok(())
    }

    fn _try_fail_batch_transfer(
        &mut self,
        batch_transfer: &DbBatchTransfer,
        throw_err: bool,
        db_data: &mut DbData,
    ) -> Result<(), Error> {
        let updated_batch_transfer = self._refresh_transfer(batch_transfer, db_data, &vec![])?;
        // fail transfer if the status didn't change after a refresh
        if updated_batch_transfer.is_none() {
            self._fail_batch_transfer(batch_transfer)?;
        } else if throw_err {
            return Err(Error::CannotFailTransfer);
        }

        Ok(())
    }

    /// Set the status for eligible transfers to [`TransferStatus::Failed`] and return if any
    /// transfer has changed
    ///
    /// An optional `recipient_id` can be provided to operate on a single transfer.
    /// An optional `txid` can be provided to operate on a batch transfer.
    /// If both a `recipient_id` and a `txid` are provided, they need to belong to the same batch
    /// transfer or an error is returned.
    ///
    /// If either `recipient_id` or `txid` are provided and `no_asset_only` is true, transfers with
    /// an associated Asset ID will not be failed and instead return an error.
    ///
    /// Transfers are eligible if they remain in status [`TransferStatus::WaitingCounterparty`]
    /// after a `refresh` has been performed. If nor `recipient_id` not `txid` have been provided,
    /// only expired transfers will be failed.
    pub fn fail_transfers(
        &mut self,
        online: Online,
        recipient_id: Option<String>,
        txid: Option<String>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        info!(
            self.logger,
            "Failing transfer with recipient ID {:?} and TXID {:?}...", recipient_id, txid
        );
        self._check_online(online)?;

        let mut db_data = self.database.get_db_data(false)?;
        let mut transfers_changed = false;

        if recipient_id.is_some() || txid.is_some() {
            let batch_transfer = if let Some(recipient_id) = recipient_id {
                let db_transfer = &mut self
                    .database
                    .get_transfer_or_fail(recipient_id, &db_data.transfers)?;
                let (_, batch_transfer) = db_transfer
                    .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)?;
                let asset_transfers =
                    batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;
                let asset_transfer_ids: Vec<i32> = asset_transfers.iter().map(|t| t.idx).collect();
                if (db_data
                    .transfers
                    .clone()
                    .into_iter()
                    .filter(|t| asset_transfer_ids.contains(&t.asset_transfer_idx))
                    .count()
                    > 1
                    || txid.is_some())
                    && txid != batch_transfer.txid
                {
                    return Err(Error::CannotFailTransfer);
                }
                batch_transfer
            } else {
                self.database
                    .get_batch_transfer_or_fail(txid.expect("TXID"), &db_data.batch_transfers)?
            };

            if !batch_transfer.waiting_counterparty() {
                return Err(Error::CannotFailTransfer);
            }

            if no_asset_only {
                let asset_transfers =
                    batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;
                let connected_assets = asset_transfers.iter().any(|t| t.asset_id.is_some());
                if connected_assets {
                    return Err(Error::CannotFailTransfer);
                }
            }

            transfers_changed = true;
            self._try_fail_batch_transfer(&batch_transfer, true, &mut db_data)?
        } else {
            // fail all transfers in status WaitingCounterparty
            let now = now().unix_timestamp();
            let mut expired_batch_transfers: Vec<DbBatchTransfer> = db_data
                .batch_transfers
                .clone()
                .into_iter()
                .filter(|t| t.waiting_counterparty() && t.expiration.unwrap_or(now) < now)
                .collect();
            for batch_transfer in expired_batch_transfers.iter_mut() {
                if no_asset_only {
                    let connected_assets = batch_transfer
                        .get_asset_transfers(&db_data.asset_transfers)?
                        .iter()
                        .any(|t| t.asset_id.is_some());
                    if connected_assets {
                        continue;
                    }
                }
                transfers_changed = true;
                self._try_fail_batch_transfer(batch_transfer, false, &mut db_data)?
            }
        }

        if transfers_changed {
            self.update_backup_info(false)?;
        }

        info!(self.logger, "Fail transfers completed");
        Ok(transfers_changed)
    }

    fn _get_new_address(&self) -> BdkAddress {
        self.bdk_wallet
            .get_address(AddressIndex::New)
            .expect("to be able to get a new address")
            .address
    }

    /// Return a new bitcoin address
    pub fn get_address(&self) -> Result<String, Error> {
        info!(self.logger, "Getting address...");
        let address = self
            .bdk_wallet
            .get_internal_address(AddressIndex::New)
            .expect("to be able to get a new address")
            .address
            .to_string();

        self.update_backup_info(false)?;

        info!(self.logger, "Get address completed");
        Ok(address)
    }

    /// Return the [`Balance`] for the requested asset
    pub fn get_asset_balance(&self, asset_id: String) -> Result<Balance, Error> {
        info!(self.logger, "Getting balance for asset '{}'...", asset_id);
        self.database.check_asset_exists(asset_id.clone())?;
        let balance = self
            .database
            .get_asset_balance(asset_id, None, None, None, None);
        info!(self.logger, "Get asset balance completed");
        balance
    }

    /// Return the [`BtcBalance`] of the underlying bitcoin wallets
    pub fn get_btc_balance(&self, online: Online) -> Result<BtcBalance, Error> {
        info!(self.logger, "Getting BTC balance...");
        self._check_online(online)?;

        let bdk_network = self.bdk_wallet.network();
        let secp = Secp256k1::new();
        let (descriptor_keychain_1, _) = self
            .bdk_wallet
            .get_descriptor_for_keychain(KeychainKind::Internal)
            .clone()
            .into_wallet_descriptor(&secp, bdk_network)
            .unwrap();
        let bdk_wallet_keychain_1 = BdkWallet::new(
            descriptor_keychain_1,
            None,
            bdk_network,
            MemoryDatabase::default(),
        )
        .map_err(InternalError::from)?;
        let (descriptor_keychain_9, _) = self
            .bdk_wallet
            .get_descriptor_for_keychain(KeychainKind::External)
            .clone()
            .into_wallet_descriptor(&secp, bdk_network)
            .unwrap();
        let bdk_wallet_keychain_9 = BdkWallet::new(
            descriptor_keychain_9,
            None,
            bdk_network,
            MemoryDatabase::default(),
        )
        .map_err(InternalError::from)?;

        self._sync_wallet(&bdk_wallet_keychain_1)?;
        self._sync_wallet(&bdk_wallet_keychain_9)?;

        let vanilla_balance = bdk_wallet_keychain_1
            .get_balance()
            .map_err(InternalError::from)?;
        let colored_balance = bdk_wallet_keychain_9
            .get_balance()
            .map_err(InternalError::from)?;
        let vanilla_future = vanilla_balance.get_total();
        let colored_future = colored_balance.get_total();
        let balance = BtcBalance {
            vanilla: Balance {
                settled: vanilla_balance.confirmed,
                future: vanilla_future,
                spendable: vanilla_future - vanilla_balance.immature,
            },
            colored: Balance {
                settled: colored_balance.confirmed,
                future: colored_future,
                spendable: colored_future - colored_balance.immature,
            },
        };
        info!(self.logger, "Get BTC balance completed");
        Ok(balance)
    }

    fn _get_contract_iface(
        &self,
        runtime: &mut RgbRuntime,
        asset_schema: &AssetSchema,
        contract_id: ContractId,
    ) -> Result<ContractIface, Error> {
        Ok(match asset_schema {
            AssetSchema::Nia => {
                let iface_name = AssetIface::RGB20.to_typename();
                let iface = runtime.iface_by_name(&iface_name)?.clone();
                runtime.contract_iface(contract_id, iface.iface_id())?
            }
            AssetSchema::Cfa => {
                let iface_name = AssetIface::RGB25.to_typename();
                let iface = runtime.iface_by_name(&iface_name)?.clone();
                runtime.contract_iface(contract_id, iface.iface_id())?
            }
        })
    }

    fn _get_asset_timestamp(&self, contract: &ContractIface) -> Result<i64, Error> {
        let timestamp = if let Ok(created) = contract.global("created") {
            match &created[0] {
                StrictVal::Tuple(fields) => match &fields[0] {
                    StrictVal::Number(StrictNum::Int(num)) => Ok::<i64, Error>(*num as i64),
                    _ => Err(InternalError::Unexpected.into()),
                },
                _ => Err(InternalError::Unexpected.into()),
            }
        } else {
            return Err(InternalError::Unexpected.into());
        }?;
        Ok(timestamp)
    }

    /// Return the [`Metadata`] for the requested asset
    pub fn get_asset_metadata(&mut self, asset_id: String) -> Result<Metadata, Error> {
        info!(self.logger, "Getting metadata for asset '{}'...", asset_id);
        let asset = self.database.check_asset_exists(asset_id)?;

        Ok(Metadata {
            asset_iface: AssetIface::from(asset.schema),
            asset_schema: asset.schema,
            issued_supply: asset.issued_supply.parse::<u64>().unwrap(),
            timestamp: asset.timestamp,
            name: asset.name,
            precision: asset.precision,
            ticker: asset.ticker,
            description: asset.description,
        })
    }

    /// Return the wallet data provided by the user
    pub fn get_wallet_data(&self) -> WalletData {
        self.wallet_data.clone()
    }

    /// Return the wallet data directory
    pub fn get_wallet_dir(&self) -> PathBuf {
        self.wallet_dir.clone()
    }

    fn _check_consistency(
        &self,
        bdk_blockchain: &ElectrumBlockchain,
        runtime: &RgbRuntime,
    ) -> Result<(), Error> {
        info!(self.logger, "Doing a consistency check...");

        self._sync_db_txos_with_blockchain(bdk_blockchain)?;
        let bdk_utxos: Vec<String> = self
            .bdk_wallet
            .list_unspent()
            .map_err(InternalError::from)?
            .into_iter()
            .map(|u| u.outpoint.to_string())
            .collect();
        let bdk_utxos: HashSet<String> = HashSet::from_iter(bdk_utxos);
        let db_utxos: Vec<String> = self
            .database
            .iter_txos()?
            .into_iter()
            .filter(|t| !t.spent)
            .map(|u| u.outpoint().to_string())
            .collect();
        let db_utxos: HashSet<String> = HashSet::from_iter(db_utxos);
        if db_utxos.difference(&bdk_utxos).count() > 0 {
            return Err(Error::Inconsistency {
                details: s!("spent bitcoins with another wallet"),
            });
        }

        let asset_ids: Vec<String> = runtime
            .contract_ids()?
            .iter()
            .map(|id| id.to_string())
            .collect();
        let db_asset_ids: Vec<String> = self.database.get_asset_ids()?;
        if !db_asset_ids.iter().all(|i| asset_ids.contains(i)) {
            return Err(Error::Inconsistency {
                details: s!("DB assets do not match with ones stored in RGB"),
            });
        }

        info!(self.logger, "Consistency check completed");
        Ok(())
    }

    fn _go_online(&self, electrum_url: String) -> Result<(Online, OnlineData), Error> {
        let online_id = now().unix_timestamp_nanos() as u64;
        let online = Online {
            id: online_id,
            electrum_url: electrum_url.clone(),
        };

        // create electrum client
        let electrum_config = ConfigBuilder::new().timeout(Some(ELECTRUM_TIMEOUT)).build();
        let electrum_client =
            ElectrumClient::from_config(&electrum_url, electrum_config).map_err(|e| {
                Error::InvalidElectrum {
                    details: e.to_string(),
                }
            })?;

        // BDK setup
        let config = ElectrumBlockchainConfig {
            url: electrum_url.clone(),
            socks5: None,
            retry: 3,
            timeout: Some(5),
            stop_gap: 20,
            validate_domain: true,
        };
        let bdk_blockchain =
            ElectrumBlockchain::from_config(&config).map_err(|e| Error::InvalidElectrum {
                details: e.to_string(),
            })?;

        // check electrum server
        if self._bitcoin_network() != BitcoinNetwork::Regtest {
            self._get_tx_details(get_txid(self._bitcoin_network()), Some(&electrum_client))?;
        }

        let online_data = OnlineData {
            id: online.id,
            bdk_blockchain,
            electrum_url,
            electrum_client,
        };

        Ok((online, online_data))
    }

    /// Return the existing or freshly generated set of wallet [`Online`] data
    ///
    /// Setting `skip_consistency_check` to true bypasses the check and allows operating an
    /// inconsistent wallet. Warning: this is dangerous, only do this if you know what you're doing!
    pub fn go_online(
        &mut self,
        skip_consistency_check: bool,
        electrum_url: String,
    ) -> Result<Online, Error> {
        info!(self.logger, "Going online...");

        let online = if let Some(online_data) = &self.online_data {
            let online = Online {
                id: online_data.id,
                electrum_url,
            };
            if online_data.electrum_url != online.electrum_url {
                let (online, online_data) = self._go_online(online.electrum_url)?;
                self.online_data = Some(online_data);
                info!(self.logger, "Went online with new electrum URL");
                online
            } else {
                self._check_online(online.clone())?;
                online
            }
        } else {
            let (online, online_data) = self._go_online(electrum_url)?;
            self.online_data = Some(online_data);
            online
        };

        if !skip_consistency_check {
            let runtime = self._rgb_runtime()?;
            self._check_consistency(self._bdk_blockchain()?, &runtime)?;
        }

        info!(self.logger, "Go online completed");
        Ok(online)
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

    fn _add_asset_to_db(
        &self,
        asset_id: String,
        schema: &AssetSchema,
        added_at: Option<i64>,
        description: Option<String>,
        issued_supply: u64,
        name: String,
        precision: u8,
        ticker: Option<String>,
        timestamp: i64,
    ) -> Result<DbAsset, Error> {
        let added_at = added_at.unwrap_or_else(|| now().unix_timestamp());
        let mut db_asset = DbAssetActMod {
            idx: ActiveValue::NotSet,
            asset_id: ActiveValue::Set(asset_id),
            schema: ActiveValue::Set(*schema),
            added_at: ActiveValue::Set(added_at),
            description: ActiveValue::Set(description),
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

    fn _get_total_issue_amount(&self, amounts: &Vec<u64>) -> Result<u64, Error> {
        if amounts.is_empty() {
            return Err(Error::NoIssuanceAmounts);
        }
        amounts.iter().try_fold(0u64, |acc, x| {
            Ok(match acc.checked_add(*x) {
                None => return Err(Error::TooHighIssuanceAmounts),
                Some(sum) => sum,
            })
        })
    }

    /// Issue a new RGB [`AssetNIA`] and return it
    pub fn issue_asset_nia(
        &mut self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetNIA, Error> {
        info!(
            self.logger,
            "Issuing RGB20 asset with ticker '{}' name '{}' precision '{}' amounts '{:?}'...",
            ticker,
            name,
            precision,
            amounts
        );
        self._check_online(online)?;

        let settled = self._get_total_issue_amount(&amounts)?;

        let mut db_data = self.database.get_db_data(false)?;
        self._handle_expired_transfers(&mut db_data)?;

        let mut unspents: Vec<LocalUnspent> = self.database.get_rgb_allocations(
            self.database.get_unspent_txos(db_data.txos)?,
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
        let created = Timestamp::from(created_at);
        let terms = RicardianContract::default();
        #[cfg(test)]
        let data = test::mock_contract_data(terms, None);
        #[cfg(not(test))]
        let data = ContractData { terms, media: None };
        let spec = DivisibleAssetSpec {
            naming: AssetNaming {
                ticker: self._check_ticker(ticker.clone())?,
                name: self._check_name(name.clone())?,
                details: None,
            },
            precision: self._check_precision(precision)?,
        };

        let mut runtime = self._rgb_runtime()?;
        let mut builder = ContractBuilder::with(rgb20(), nia_schema(), nia_rgb20())
            .map_err(InternalError::from)?
            .set_chain(runtime.chain())
            .add_global_state("spec", spec)
            .expect("invalid spec")
            .add_global_state("data", data)
            .expect("invalid data")
            .add_global_state("created", created)
            .expect("invalid created")
            .add_global_state("issuedSupply", Amount::from(settled))
            .expect("invalid issuedSupply");

        let mut issue_utxos: HashMap<DbTxo, u64> = HashMap::new();
        for amount in &amounts {
            let exclude_outpoints: Vec<Outpoint> =
                issue_utxos.keys().map(|txo| txo.outpoint()).collect();
            let utxo = self._get_utxo(exclude_outpoints, Some(unspents.clone()), false)?;
            let outpoint = utxo.outpoint().to_string();
            issue_utxos.insert(utxo, *amount);

            let seal = ExplicitSeal::<RgbTxid>::from_str(&format!("opret1st:{outpoint}"))
                .map_err(InternalError::from)?;
            let seal = GenesisSeal::from(seal);

            builder = builder
                .add_fungible_state("assetOwner", seal, *amount)
                .expect("invalid global state data");
        }
        debug!(self.logger, "Issuing on UTXOs: {issue_utxos:?}");

        let contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = contract.contract_id().to_string();
        let validated_contract = contract
            .validate(&mut self._blockchain_resolver()?)
            .expect("internal error: failed validating self-issued contract");
        runtime
            .import_contract(validated_contract, &mut self._blockchain_resolver()?)
            .expect("failure importing issued contract");

        let asset = self._add_asset_to_db(
            asset_id.clone(),
            &AssetSchema::Nia,
            Some(created_at),
            None,
            settled,
            name,
            precision,
            Some(ticker),
            created_at,
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
            amount: ActiveValue::Set(settled.to_string()),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        for (utxo, amount) in issue_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                coloring_type: ActiveValue::Set(ColoringType::Issue),
                amount: ActiveValue::Set(amount.to_string()),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }

        let asset = AssetNIA::get_asset_details(
            self,
            &asset,
            self.wallet_dir.join(ASSETS_DIR),
            None,
            None,
            None,
            None,
        )?;

        self.update_backup_info(false)?;

        info!(self.logger, "Issue asset RGB20 completed");
        Ok(asset)
    }

    /// Issue a new RGB [`AssetCFA`] and return it
    pub fn issue_asset_cfa(
        &mut self,
        online: Online,
        name: String,
        description: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, Error> {
        info!(
            self.logger,
            "Issuing RGB25 asset with name '{}' precision '{}' amounts '{:?}'...",
            name,
            precision,
            amounts
        );
        self._check_online(online)?;

        let settled = self._get_total_issue_amount(&amounts)?;

        let mut db_data = self.database.get_db_data(false)?;
        self._handle_expired_transfers(&mut db_data)?;

        let mut unspents: Vec<LocalUnspent> = self.database.get_rgb_allocations(
            self.database.get_unspent_txos(db_data.txos)?,
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
        let created = Timestamp::from(created_at);
        let terms = RicardianContract::default();
        let (media, mime) = if let Some(fp) = &file_path {
            let fpath = std::path::Path::new(fp);
            if !fpath.exists() {
                return Err(Error::InvalidFilePath {
                    file_path: fp.clone(),
                });
            }
            let file_bytes = std::fs::read(fp.clone())?;
            let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
            let digest = file_hash.to_byte_array();
            let mime = tree_magic::from_filepath(fpath);
            let media_ty: &'static str = Box::leak(mime.clone().into_boxed_str());
            let media_type = MediaType::with(media_ty);
            (
                Some(Attachment {
                    ty: media_type,
                    digest,
                }),
                Some(mime),
            )
        } else {
            (None, None)
        };
        let data = ContractData {
            terms,
            media: media.clone(),
        };
        let precision_state = self._check_precision(precision)?;
        let name_state = self._check_name(name.clone())?;

        let mut runtime = self._rgb_runtime()?;
        let mut builder = ContractBuilder::with(rgb25(), cfa_schema(), cfa_rgb25())
            .map_err(InternalError::from)?
            .set_chain(runtime.chain())
            .add_global_state("name", name_state)
            .expect("invalid name")
            .add_global_state("precision", precision_state)
            .expect("invalid precision")
            .add_global_state("data", data)
            .expect("invalid data")
            .add_global_state("created", created)
            .expect("invalid created")
            .add_global_state("issuedSupply", Amount::from(settled))
            .expect("invalid issuedSupply");

        if let Some(desc) = &description {
            if desc.is_empty() {
                return Err(Error::InvalidDescription {
                    details: s!("ident must contain at least one character"),
                });
            }
            let details = Details::from_str(desc).map_err(|e| Error::InvalidDescription {
                details: e.to_string(),
            })?;
            builder = builder
                .add_global_state("details", details)
                .expect("invalid details");
        };

        let mut issue_utxos: HashMap<DbTxo, u64> = HashMap::new();
        for amount in &amounts {
            let exclude_outpoints: Vec<Outpoint> =
                issue_utxos.keys().map(|txo| txo.outpoint()).collect();
            let utxo = self._get_utxo(exclude_outpoints, Some(unspents.clone()), false)?;
            let outpoint = utxo.outpoint().to_string();
            issue_utxos.insert(utxo, *amount);

            let seal = ExplicitSeal::<RgbTxid>::from_str(&format!("opret1st:{outpoint}"))
                .map_err(InternalError::from)?;
            let seal = GenesisSeal::from(seal);

            builder = builder
                .add_fungible_state("assetOwner", seal, *amount)
                .expect("invalid global state data");
        }
        debug!(self.logger, "Issuing on UTXOs: {issue_utxos:?}");

        let contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = contract.contract_id().to_string();
        let validated_contract = contract
            .validate(&mut self._blockchain_resolver()?)
            .expect("internal error: failed validating self-issued contract");
        runtime
            .import_contract(validated_contract, &mut self._blockchain_resolver()?)
            .expect("failure importing issued contract");

        if let Some(fp) = file_path {
            let attachment_id = hex::encode(media.unwrap().digest);
            let media_dir = self
                .wallet_dir
                .join(ASSETS_DIR)
                .join(asset_id.clone())
                .join(attachment_id);
            fs::create_dir_all(&media_dir)?;
            let media_path = media_dir.join(MEDIA_FNAME);
            fs::copy(fp, &media_path)?;
            let mime = mime.unwrap();
            fs::write(media_dir.join(MIME_FNAME), mime)?;
        }

        let asset = self._add_asset_to_db(
            asset_id.clone(),
            &AssetSchema::Cfa,
            Some(created_at),
            description,
            settled,
            name,
            precision,
            None,
            created_at,
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
            amount: ActiveValue::Set(settled.to_string()),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        for (utxo, amount) in issue_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                coloring_type: ActiveValue::Set(ColoringType::Issue),
                amount: ActiveValue::Set(amount.to_string()),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }

        let asset = AssetCFA::get_asset_details(
            self,
            &asset,
            self.wallet_dir.join(ASSETS_DIR),
            None,
            None,
            None,
            None,
        )?;

        self.update_backup_info(false)?;

        info!(self.logger, "Issue asset RGB25 completed");
        Ok(asset)
    }

    /// List the assets known by the underlying RGB node
    pub fn list_assets(
        &mut self,
        mut filter_asset_schemas: Vec<AssetSchema>,
    ) -> Result<Assets, Error> {
        info!(self.logger, "Listing assets...");
        if filter_asset_schemas.is_empty() {
            filter_asset_schemas = vec![AssetSchema::Nia, AssetSchema::Cfa];
        }

        let batch_transfers = Some(self.database.iter_batch_transfers()?);
        let colorings = Some(self.database.iter_colorings()?);
        let txos = Some(self.database.iter_txos()?);
        let asset_transfers = Some(self.database.iter_asset_transfers()?);

        let assets = self.database.iter_assets()?;
        let mut nia = None;
        let mut cfa = None;
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
                                    self.wallet_dir.join(ASSETS_DIR),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetNIA>, Error>>()?,
                    );
                }
                AssetSchema::Cfa => {
                    let assets_dir = self.wallet_dir.join(ASSETS_DIR);
                    cfa = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetCFA::get_asset_details(
                                    self,
                                    a,
                                    assets_dir.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetCFA>, Error>>()?,
                    );
                }
            }
        }

        info!(self.logger, "List assets completed");
        Ok(Assets { nia, cfa })
    }

    fn _sync_if_online(&self, online: Option<Online>) -> Result<(), Error> {
        if let Some(online) = online {
            self._check_online(online)?;
            self._sync_wallet(&self.bdk_wallet)?;
        }
        Ok(())
    }

    /// List the [`Transaction`]s known to the RGB wallet
    pub fn list_transactions(&self, online: Option<Online>) -> Result<Vec<Transaction>, Error> {
        info!(self.logger, "Listing transactions...");

        self._sync_if_online(online)?;

        let mut create_utxos_txids = vec![];
        let mut drain_txids = vec![];
        let wallet_transactions = self.database.iter_wallet_transactions()?;
        for tx in wallet_transactions {
            match tx.wallet_transaction_type {
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
            .list_transactions(false)
            .map_err(InternalError::from)?
            .into_iter()
            .map(|t| {
                let txid = t.txid.to_string();
                let transaction_type = if drain_txids.contains(&txid) {
                    TransactionType::Drain
                } else if create_utxos_txids.contains(&txid) {
                    TransactionType::CreateUtxos
                } else if rgb_send_txids.contains(&txid) {
                    TransactionType::RgbSend
                } else {
                    TransactionType::User
                };
                Transaction {
                    transaction_type,
                    txid,
                    received: t.received,
                    sent: t.sent,
                    fee: t.fee,
                    confirmation_time: t.confirmation_time,
                }
            })
            .collect();
        info!(self.logger, "List transactions completed");
        Ok(transactions)
    }

    /// List the [`Transfer`]s known to the RGB wallet.
    ///
    /// When the `asset_id` is not provided return transfers that are not connected to a specific
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

    /// List the [`Unspent`]s known to the RGB wallet,
    /// if `settled` is true only show settled allocations
    /// if `settled` is false also show pending allocations
    pub fn list_unspents(
        &self,
        online: Option<Online>,
        settled_only: bool,
    ) -> Result<Vec<Unspent>, Error> {
        info!(self.logger, "Listing unspents...");

        self._sync_if_online(online)?;

        let db_data = self.database.get_db_data(true)?;

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
        )?;

        txos_allocations
            .iter_mut()
            .for_each(|t| t.rgb_allocations.retain(|a| a.settled() || a.future()));

        let mut unspents: Vec<Unspent> = txos_allocations.into_iter().map(Unspent::from).collect();

        if settled_only {
            unspents
                .iter_mut()
                .for_each(|u| u.rgb_allocations.retain(|a| a.settled));
        }

        let mut internal_unspents: Vec<Unspent> = self
            ._internal_unspents()?
            .filter(|u| !u.is_spent)
            .map(Unspent::from)
            .collect();

        unspents.append(&mut internal_unspents);

        info!(self.logger, "List unspents completed");
        Ok(unspents)
    }

    fn _get_signed_psbt(&self, transfer_dir: PathBuf) -> Result<BdkPsbt, Error> {
        let psbt_file = transfer_dir.join(SIGNED_PSBT_FILE);
        let psbt_str = fs::read_to_string(psbt_file)?;
        Ok(BdkPsbt::from_str(&psbt_str)?)
    }

    fn _fail_batch_transfer_if_no_endpoints(
        &self,
        batch_transfer: &DbBatchTransfer,
        transfer_transport_endpoints_data: &Vec<(DbTransferTransportEndpoint, DbTransportEndpoint)>,
    ) -> Result<bool, Error> {
        if transfer_transport_endpoints_data.is_empty() {
            self._fail_batch_transfer(batch_transfer)?;
            return Ok(true);
        }

        Ok(false)
    }

    fn _refuse_consignment(
        &self,
        proxy_url: String,
        recipient_id: String,
        updated_batch_transfer: &mut DbBatchTransferActMod,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger, "Consignment is invalid");
        let nack_res = self
            .rest_client
            .clone()
            .post_ack(&proxy_url, recipient_id, false)?;
        debug!(self.logger, "Consignment NACK response: {:?}", nack_res);
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        Ok(Some(
            self.database
                .update_batch_transfer(updated_batch_transfer)?,
        ))
    }

    /// Extract the metadata of a new asset and save the asset into the DB
    pub fn save_new_asset(
        &self,
        runtime: &mut RgbRuntime,
        asset_schema: &AssetSchema,
        contract_id: ContractId,
    ) -> Result<ContractIface, Error> {
        let contract_iface = self._get_contract_iface(runtime, asset_schema, contract_id)?;

        let timestamp = self._get_asset_timestamp(&contract_iface)?;
        let (name, precision, issued_supply, ticker, description) = match &asset_schema {
            AssetSchema::Nia => {
                let iface_nia = Rgb20::from(contract_iface.clone());
                let spec = iface_nia.spec();
                let ticker = spec.ticker().to_string();
                let name = spec.name().to_string();
                let precision = spec.precision.into();
                let issued_supply = iface_nia.total_issued_supply().into();
                (name, precision, issued_supply, Some(ticker), None)
            }
            AssetSchema::Cfa => {
                let iface_cfa = Rgb25::from(contract_iface.clone());
                let name = iface_cfa.name().to_string();
                let precision = iface_cfa.precision().into();
                let issued_supply = iface_cfa.total_issued_supply().into();
                let mut details = None;
                if let Some(det) = iface_cfa.details() {
                    details = Some(det.to_string());
                }
                (name, precision, issued_supply, None, details)
            }
        };

        self._add_asset_to_db(
            contract_id.to_string(),
            asset_schema,
            None,
            description,
            issued_supply,
            name,
            precision,
            ticker,
            timestamp,
        )?;

        self.update_backup_info(false)?;

        Ok(contract_iface)
    }

    fn _wait_consignment(
        &mut self,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger, "Waiting consignment...");

        let batch_transfer_data =
            batch_transfer.get_transfers(&db_data.asset_transfers, &db_data.transfers)?;
        let (asset_transfer, transfer) =
            self.database.get_incoming_transfer(&batch_transfer_data)?;
        let recipient_id = transfer
            .recipient_id
            .clone()
            .expect("transfer should have a recipient ID");

        // check if a consignment has been posted
        let tte_data = self
            .database
            .get_transfer_transport_endpoints_data(transfer.idx)?;
        if self._fail_batch_transfer_if_no_endpoints(batch_transfer, &tte_data)? {
            return Ok(None);
        }
        let mut proxy_res = None;
        for (transfer_transport_endpoint, transport_endpoint) in tte_data {
            let consignment_res = self
                .rest_client
                .clone()
                .get_consignment(&transport_endpoint.endpoint, recipient_id.clone());
            if consignment_res.is_err() {
                debug!(
                    self.logger,
                    "Consignment GET response error: {:?}", &consignment_res
                );
                info!(
                    self.logger,
                    "Skipping transport endpoint: {:?}", &transport_endpoint
                );
                continue;
            }
            let consignment_res = consignment_res.unwrap();
            debug!(
                self.logger,
                "Consignment GET response: {:?}", consignment_res
            );

            if let Some(result) = consignment_res.result {
                proxy_res = Some((
                    result.consignment,
                    transport_endpoint.endpoint,
                    result.txid,
                    result.vout,
                ));
                let mut updated_transfer_transport_endpoint: DbTransferTransportEndpointActMod =
                    transfer_transport_endpoint.into();
                updated_transfer_transport_endpoint.used = ActiveValue::Set(true);
                self.database
                    .update_transfer_transport_endpoint(&mut updated_transfer_transport_endpoint)?;
                break;
            }
        }

        let (consignment, proxy_url, txid, vout) = if let Some(res) = proxy_res {
            (res.0, res.1, res.2, res.3)
        } else {
            return Ok(None);
        };

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();

        // write consignment
        let transfer_dir = self
            .wallet_dir
            .join(TRANSFER_DIR)
            .join(recipient_id.clone());
        let consignment_path = transfer_dir.join(CONSIGNMENT_RCV_FILE);
        fs::create_dir_all(transfer_dir)?;
        let consignment_bytes = general_purpose::STANDARD
            .decode(consignment)
            .map_err(InternalError::from)?;
        fs::write(consignment_path.clone(), consignment_bytes).expect("Unable to write file");

        let mut runtime = self._rgb_runtime()?;
        let bindle = Bindle::<RgbTransfer>::load(consignment_path).map_err(InternalError::from)?;
        let consignment: RgbTransfer = bindle.unbindle();
        let contract_id = consignment.contract_id();
        let asset_id = contract_id.to_string();

        // validate consignment
        if let Some(aid) = asset_transfer.asset_id.clone() {
            // check if asset transfer is connected to the asset we are actually receiving
            if aid != asset_id {
                return self._refuse_consignment(
                    proxy_url,
                    recipient_id,
                    &mut updated_batch_transfer,
                );
            }
        }

        debug!(self.logger, "Validating consignment...");
        let validated_consignment = match consignment
            .clone()
            .validate(&mut self._blockchain_resolver()?)
        {
            Ok(consignment) => consignment,
            Err(consignment) => consignment,
        };
        let validation_status = validated_consignment.into_validation_status().unwrap();
        let validity = validation_status.validity();
        debug!(self.logger, "Consignment validity: {:?}", validity);

        if validity == Validity::UnresolvedTransactions {
            warn!(
                self.logger,
                "Consignment contains unresolved TXIDs: {:?}", validation_status.unresolved_txids
            );
            return Ok(None);
        }
        if ![Validity::Valid, Validity::UnminedTerminals].contains(&validity) {
            return self._refuse_consignment(proxy_url, recipient_id, &mut updated_batch_transfer);
        }

        let schema_id = consignment.schema_id().to_string();
        let asset_schema = AssetSchema::from_schema_id(schema_id)?;

        // add asset info to transfer if missing
        let contract_iface = if asset_transfer.asset_id.is_none() {
            // check if asset is known
            let exists_check = self.database.check_asset_exists(asset_id.clone());
            let contract_iface = if exists_check.is_err() {
                // unknown asset
                debug!(self.logger, "Registering contract...");
                let mut minimal_contract = consignment.clone().into_contract();
                minimal_contract.bundles = none!();
                minimal_contract.terminals = none!();
                let minimal_contract_validated =
                    match minimal_contract.validate(&mut self._blockchain_resolver()?) {
                        Ok(consignment) => consignment,
                        Err(consignment) => consignment,
                    };
                runtime
                    .import_contract(
                        minimal_contract_validated,
                        &mut self._blockchain_resolver()?,
                    )
                    .expect("failure importing issued contract");
                debug!(self.logger, "Contract registered");

                self.save_new_asset(&mut runtime, &asset_schema, contract_id)?
            } else {
                self._get_contract_iface(&mut runtime, &asset_schema, contract_id)?
            };

            let mut updated_asset_transfer: DbAssetTransferActMod = asset_transfer.clone().into();
            updated_asset_transfer.asset_id = ActiveValue::Set(Some(asset_id.clone()));
            self.database
                .update_asset_transfer(&mut updated_asset_transfer)?;

            contract_iface
        } else {
            self._get_contract_iface(&mut runtime, &asset_schema, contract_id)?
        };

        let contract_data = match asset_schema {
            AssetSchema::Nia => {
                let iface_nia = Rgb20::from(contract_iface);
                iface_nia.contract_data()
            }
            AssetSchema::Cfa => {
                let iface_cfa = Rgb25::from(contract_iface);
                iface_cfa.contract_data()
            }
        };
        if let Some(media) = contract_data.media {
            let attachment_id = hex::encode(media.digest);
            let media_res = self
                .rest_client
                .clone()
                .get_media(&proxy_url, attachment_id.clone())?;
            debug!(self.logger, "Media GET response: {:?}", media_res);
            if let Some(media_res) = media_res.result {
                let file_bytes = general_purpose::STANDARD
                    .decode(media_res)
                    .map_err(InternalError::from)?;
                let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
                let real_attachment_id = hex::encode(file_hash.to_byte_array());
                if attachment_id != real_attachment_id {
                    return self._refuse_consignment(
                        proxy_url,
                        recipient_id,
                        &mut updated_batch_transfer,
                    );
                }
                let media_dir = self
                    .wallet_dir
                    .join(ASSETS_DIR)
                    .join(asset_id.clone())
                    .join(&attachment_id);
                fs::create_dir_all(&media_dir)?;
                fs::write(media_dir.join(MEDIA_FNAME), file_bytes)?;
                fs::write(media_dir.join(MIME_FNAME), media.ty.to_string())?;
            } else {
                return self._refuse_consignment(
                    proxy_url,
                    recipient_id,
                    &mut updated_batch_transfer,
                );
            }
        }

        let mut amount = 0;
        let known_concealed = if transfer.recipient_type == Some(RecipientType::Blind) {
            Some(SecretSeal::from_str(&recipient_id).expect("saved recipient ID is invalid"))
        } else {
            None
        };
        if let Some(anchored_bundle) = consignment
            .anchored_bundles()
            .find(|ab| ab.anchor.txid.to_string() == txid)
        {
            'outer: for bundle_item in anchored_bundle.bundle.values() {
                if let Some(transition) = &bundle_item.transition {
                    for assignment in transition.assignments.values() {
                        for fungible_assignment in assignment.as_fungible() {
                            if let Assign::ConfidentialSeal { seal, state } = fungible_assignment {
                                if Some(*seal) == known_concealed {
                                    amount = state.value.as_u64();
                                    break 'outer;
                                }
                            };
                            if let Assign::Revealed { seal, state } = fungible_assignment {
                                if Some(seal.vout.into_u32()) == vout {
                                    amount = state.value.as_u64();
                                    break 'outer;
                                }
                            };
                        }
                    }
                }
            }
        }

        if amount == 0 {
            return self._refuse_consignment(proxy_url, recipient_id, &mut updated_batch_transfer);
        }

        debug!(
            self.logger,
            "Consignment is valid. Received '{}' of contract '{}'", amount, asset_id
        );

        let ack_res = self
            .rest_client
            .clone()
            .post_ack(&proxy_url, recipient_id, true)?;
        debug!(self.logger, "Consignment ACK response: {:?}", ack_res);

        let mut updated_transfer: DbTransferActMod = transfer.clone().into();
        updated_transfer.amount = ActiveValue::Set(amount.to_string());
        updated_transfer.vout = ActiveValue::Set(vout);
        self.database.update_transfer(&mut updated_transfer)?;

        if transfer.recipient_type == Some(RecipientType::Blind) {
            let transfer_colorings = db_data
                .colorings
                .clone()
                .into_iter()
                .filter(|c| {
                    c.asset_transfer_idx == asset_transfer.idx
                        && c.coloring_type == ColoringType::Receive
                })
                .collect::<Vec<DbColoring>>()
                .first()
                .cloned();
            let transfer_coloring =
                transfer_colorings.expect("transfer should be connected to at least one coloring");
            let mut updated_coloring: DbColoringActMod = transfer_coloring.into();
            updated_coloring.amount = ActiveValue::Set(amount.to_string());
            self.database.update_coloring(updated_coloring)?;
        }

        updated_batch_transfer.txid = ActiveValue::Set(Some(txid));
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::WaitingConfirmations);

        Ok(Some(
            self.database
                .update_batch_transfer(&mut updated_batch_transfer)?,
        ))
    }

    fn _wait_ack(
        &self,
        batch_transfer: &DbBatchTransfer,
        db_data: &mut DbData,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger, "Waiting ACK...");

        let mut batch_transfer_data =
            batch_transfer.get_transfers(&db_data.asset_transfers, &db_data.transfers)?;
        for asset_transfer_data in batch_transfer_data.asset_transfers_data.iter_mut() {
            for transfer in asset_transfer_data.transfers.iter_mut() {
                if transfer.ack.is_some() {
                    continue;
                }
                let tte_data = self
                    .database
                    .get_transfer_transport_endpoints_data(transfer.idx)?;
                if self._fail_batch_transfer_if_no_endpoints(batch_transfer, &tte_data)? {
                    return Ok(None);
                }
                let (_, transport_endpoint) = tte_data
                    .clone()
                    .into_iter()
                    .find(|(tte, _ce)| tte.used)
                    .expect("there should be 1 used TTE");
                let proxy_url = transport_endpoint.endpoint.clone();
                let ack_res = self.rest_client.clone().get_ack(
                    &proxy_url,
                    transfer
                        .recipient_id
                        .clone()
                        .expect("transfer should have a recipient ID"),
                )?;
                debug!(self.logger, "Consignment ACK/NACK response: {:?}", ack_res);

                if ack_res.result.is_some() {
                    let mut updated_transfer: DbTransferActMod = transfer.clone().into();
                    updated_transfer.ack = ActiveValue::Set(ack_res.result);
                    self.database.update_transfer(&mut updated_transfer)?;
                    transfer.ack = ack_res.result;
                }
            }
        }

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        let mut batch_transfer_transfers: Vec<DbTransfer> = vec![];
        batch_transfer_data
            .asset_transfers_data
            .iter()
            .for_each(|atd| batch_transfer_transfers.extend(atd.transfers.clone()));
        if batch_transfer_transfers
            .iter()
            .any(|t| t.ack == Some(false))
        {
            updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        } else if batch_transfer_transfers.iter().all(|t| t.ack == Some(true)) {
            let transfer_dir = self.wallet_dir.join(TRANSFER_DIR).join(
                batch_transfer
                    .txid
                    .as_ref()
                    .expect("batch transfer should have a TXID"),
            );
            let signed_psbt = self._get_signed_psbt(transfer_dir)?;
            self._broadcast_psbt(signed_psbt)?;
            updated_batch_transfer.status = ActiveValue::Set(TransferStatus::WaitingConfirmations);
        } else {
            return Ok(None);
        }

        Ok(Some(
            self.database
                .update_batch_transfer(&mut updated_batch_transfer)?,
        ))
    }

    fn _wait_confirmations(
        &mut self,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
        incoming: bool,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger, "Waiting confirmations...");
        let txid = batch_transfer
            .txid
            .clone()
            .expect("batch transfer should have a TXID");
        debug!(
            self.logger,
            "Getting details of transaction with ID '{}'...", txid
        );
        let tx_details = match self._get_tx_details(txid.clone(), None) {
            Ok(v) => Ok(v),
            Err(e) => {
                if e.to_string()
                    .contains("No such mempool or blockchain transaction")
                {
                    debug!(self.logger, "Cannot find transaction");
                    return Ok(None);
                } else {
                    Err(e)
                }
            }
        }?;
        debug!(
            self.logger,
            "Confirmations: {:?}",
            tx_details.get("confirmations")
        );

        if batch_transfer.min_confirmations > 0
            && (tx_details.get("confirmations").is_none()
                || tx_details["confirmations"]
                    .as_u64()
                    .expect("confirmations to be a valid u64 number")
                    < batch_transfer.min_confirmations as u64)
        {
            return Ok(None);
        }

        if incoming {
            let batch_transfer_data =
                batch_transfer.get_transfers(&db_data.asset_transfers, &db_data.transfers)?;
            let (asset_transfer, transfer) =
                self.database.get_incoming_transfer(&batch_transfer_data)?;
            let recipient_id = transfer
                .clone()
                .recipient_id
                .expect("transfer should have a recipient ID");
            let transfer_dir = self.wallet_dir.join(TRANSFER_DIR).join(recipient_id);
            let consignment_path = transfer_dir.join(CONSIGNMENT_RCV_FILE);
            let bindle =
                Bindle::<RgbTransfer>::load(consignment_path).map_err(InternalError::from)?;
            let consignment = bindle.unbindle();

            if transfer.recipient_type == Some(RecipientType::Witness) {
                self._sync_db_txos()?;
                let utxo = self
                    .database
                    .get_txo(Outpoint {
                        txid,
                        vout: transfer.vout.unwrap(),
                    })?
                    .expect("outpoint should be in the DB");

                let db_coloring = DbColoringActMod {
                    txo_idx: ActiveValue::Set(utxo.idx),
                    asset_transfer_idx: ActiveValue::Set(asset_transfer.idx),
                    coloring_type: ActiveValue::Set(ColoringType::Receive),
                    amount: ActiveValue::Set(transfer.amount),
                    ..Default::default()
                };
                self.database.set_coloring(db_coloring)?;
            }

            // accept consignment
            let consignment = consignment
                .validate(&mut self._blockchain_resolver()?)
                .unwrap_or_else(|c| c);
            let mut runtime = self._rgb_runtime()?;
            let force = false;
            let validation_status =
                runtime.accept_transfer(consignment, &mut self._blockchain_resolver()?, force)?;
            let validity = validation_status.validity();
            if !matches!(validity, Validity::Valid) {
                return Err(InternalError::Unexpected)?;
            }
        }

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Settled);
        let updated = self
            .database
            .update_batch_transfer(&mut updated_batch_transfer)?;

        Ok(Some(updated))
    }

    fn _wait_counterparty(
        &mut self,
        transfer: &DbBatchTransfer,
        db_data: &mut DbData,
        incoming: bool,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        if incoming {
            self._wait_consignment(transfer, db_data)
        } else {
            self._wait_ack(transfer, db_data)
        }
    }

    fn _refresh_transfer(
        &mut self,
        transfer: &DbBatchTransfer,
        db_data: &mut DbData,
        filter: &Vec<RefreshFilter>,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger, "Refreshing transfer: {:?}", transfer);
        let incoming = transfer.incoming(&db_data.asset_transfers, &db_data.transfers)?;
        if !filter.is_empty() {
            let requested = RefreshFilter {
                status: RefreshTransferStatus::try_from(transfer.status).expect("pending status"),
                incoming,
            };
            if !filter.contains(&requested) {
                return Ok(None);
            }
        }
        match transfer.status {
            TransferStatus::WaitingCounterparty => {
                self._wait_counterparty(transfer, db_data, incoming)
            }
            TransferStatus::WaitingConfirmations => {
                self._wait_confirmations(transfer, db_data, incoming)
            }
            _ => Ok(None),
        }
    }

    /// Refresh the status of pending transfers and return if any transfer has changed
    ///
    /// An optional `asset_id` can be provided to operate on a single asset.
    /// Each item in the [`RefreshFilter`] vector defines a combination of transfer status and
    /// direction to be refreshed, skipping any others. If the vector is empty, all combinations
    /// are refreshed.
    ///
    /// Changes to each transfer depend on its status and whether the wallet is on the receiving or
    /// sending side.
    pub fn refresh(
        &mut self,
        online: Online,
        asset_id: Option<String>,
        filter: Vec<RefreshFilter>,
    ) -> Result<bool, Error> {
        if let Some(aid) = asset_id.clone() {
            info!(self.logger, "Refreshing asset {}...", aid);
            self.database.check_asset_exists(aid)?;
        } else {
            info!(self.logger, "Refreshing assets...");
        }
        self._check_online(online)?;

        let mut db_data = self.database.get_db_data(false)?;

        if asset_id.is_some() {
            let batch_transfers_ids: Vec<i32> = db_data
                .asset_transfers
                .iter()
                .filter(|t| t.asset_id == asset_id)
                .map(|t| t.batch_transfer_idx)
                .collect();
            db_data
                .batch_transfers
                .retain(|t| batch_transfers_ids.contains(&t.idx));
        };
        db_data.batch_transfers.retain(|t| t.pending());

        let mut transfers_changed = false;
        for transfer in db_data.batch_transfers.clone().into_iter() {
            if self
                ._refresh_transfer(&transfer, &mut db_data, &filter)?
                .is_some()
            {
                transfers_changed = true;
            }
        }

        if transfers_changed {
            self.update_backup_info(false)?;
        }

        info!(self.logger, "Refresh completed");
        Ok(transfers_changed)
    }

    fn _select_rgb_inputs(
        &self,
        asset_id: String,
        amount_needed: u64,
        unspents: Vec<LocalUnspent>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
    ) -> Result<AssetSpend, Error> {
        debug!(self.logger, "Selecting inputs for asset '{}'...", asset_id);
        let mut input_allocations: HashMap<DbTxo, u64> = HashMap::new();
        let mut amount_input_asset: u64 = 0;
        for unspent in unspents {
            let mut asset_allocations: Vec<LocalRgbAllocation> = unspent
                .rgb_allocations
                .into_iter()
                .filter(|a| a.asset_id == Some(asset_id.clone()) && a.status.settled())
                .collect();
            if asset_allocations.is_empty() {
                continue;
            }
            asset_allocations.sort_by(|a, b| b.cmp(a));
            let amount_allocation: u64 = asset_allocations.iter().map(|a| a.amount).sum();
            input_allocations.insert(unspent.utxo, amount_allocation);
            amount_input_asset += amount_allocation;
            if amount_input_asset >= amount_needed {
                break;
            }
        }
        if amount_input_asset < amount_needed {
            let ass_balance = self.database.get_asset_balance(
                asset_id.clone(),
                asset_transfers,
                batch_transfers,
                colorings,
                None,
            )?;
            if ass_balance.future < amount_needed {
                return Err(Error::InsufficientTotalAssets { asset_id });
            }
            return Err(Error::InsufficientSpendableAssets { asset_id });
        }
        debug!(self.logger, "Asset input amount {:?}", amount_input_asset);
        let inputs: Vec<DbTxo> = input_allocations.clone().into_keys().collect();
        inputs
            .iter()
            .for_each(|t| debug!(self.logger, "Input outpoint '{}'", t.outpoint().to_string()));
        let txo_map: HashMap<i32, u64> = input_allocations
            .into_iter()
            .map(|(k, v)| (k.idx, v))
            .collect();
        let input_outpoints: Vec<BdkOutPoint> = inputs.into_iter().map(BdkOutPoint::from).collect();
        let change_amount = amount_input_asset - amount_needed;
        debug!(self.logger, "Asset change amount {:?}", change_amount);
        Ok(AssetSpend {
            txo_map,
            input_outpoints,
            change_amount,
        })
    }

    fn _prepare_psbt(
        &self,
        input_outpoints: Vec<BdkOutPoint>,
        witness_recipients: &HashMap<ScriptBuf, u64>,
        fee_rate: f32,
    ) -> Result<BdkPsbt, Error> {
        let mut builder = self.bdk_wallet.build_tx();
        builder
            .add_utxos(&input_outpoints)
            .map_err(InternalError::from)?
            .manually_selected_only()
            .fee_rate(FeeRate::from_sat_per_vb(fee_rate))
            .ordering(bdk::wallet::tx_builder::TxOrdering::Untouched);
        for (script_buf, amount_sat) in witness_recipients {
            let bdk_script = BdkScript::from(script_buf.clone().into_bytes());
            builder.add_recipient(bdk_script, *amount_sat);
        }
        builder
            .drain_to(self._get_new_address().script_pubkey())
            .add_data(&[1]);

        Ok(builder
            .finish()
            .map_err(|e| match e {
                bdk::Error::InsufficientFunds { needed, available } => {
                    Error::InsufficientBitcoins { needed, available }
                }
                _ => Error::from(InternalError::from(e)),
            })?
            .0)
    }

    fn _try_prepare_psbt(
        &self,
        input_unspents: &[LocalUnspent],
        all_inputs: &mut Vec<BdkOutPoint>,
        witness_recipients: &HashMap<ScriptBuf, u64>,
        fee_rate: f32,
    ) -> Result<BdkPsbt, Error> {
        let psbt = loop {
            break match self._prepare_psbt(all_inputs.clone(), witness_recipients, fee_rate) {
                Ok(psbt) => psbt,
                Err(Error::InsufficientBitcoins { .. }) => {
                    let used_txos: Vec<Outpoint> =
                        all_inputs.clone().into_iter().map(|o| o.into()).collect();
                    if let Some(a) = self
                        ._get_available_allocations(
                            input_unspents.to_vec(),
                            used_txos.clone(),
                            Some(0),
                        )?
                        .pop()
                    {
                        all_inputs.push(a.utxo.into());
                        continue;
                    } else {
                        return Err(self._detect_btc_unspendable_err()?);
                    }
                }
                Err(e) => return Err(e),
            };
        };
        Ok(psbt)
    }

    fn _get_change_utxo(
        &self,
        change_utxo_option: &mut Option<DbTxo>,
        change_utxo_idx: &mut Option<i32>,
        input_outpoints: Vec<OutPoint>,
        unspents: Vec<LocalUnspent>,
    ) -> Result<DbTxo, Error> {
        if change_utxo_option.is_none() {
            let change_utxo = self._get_utxo(
                input_outpoints.into_iter().map(|t| t.into()).collect(),
                Some(unspents),
                true,
            )?;
            debug!(
                self.logger,
                "Change outpoint '{}'",
                change_utxo.outpoint().to_string()
            );
            *change_utxo_idx = Some(change_utxo.idx);
            *change_utxo_option = Some(change_utxo);
        }
        Ok(change_utxo_option.clone().unwrap())
    }

    fn _prepare_rgb_psbt(
        &self,
        psbt: &mut PartiallySignedTransaction,
        input_outpoints: Vec<OutPoint>,
        transfer_info_map: BTreeMap<String, InfoAssetTransfer>,
        transfer_dir: PathBuf,
        donation: bool,
        unspents: Vec<LocalUnspent>,
        runtime: &mut RgbRuntime,
        min_confirmations: u8,
    ) -> Result<(), Error> {
        let mut change_utxo_option = None;
        let mut change_utxo_idx = None;

        let prev_outputs = psbt
            .unsigned_tx
            .input
            .iter()
            .map(|txin| txin.previous_output)
            .map(|outpoint| RgbOutpoint::new(outpoint.txid.to_byte_array().into(), outpoint.vout))
            .collect::<Vec<_>>();
        let outputs = psbt.unsigned_tx.output.clone();
        let mut all_transitions: HashMap<ContractId, Transition> = HashMap::new();
        let mut asset_beneficiaries: BTreeMap<String, Vec<BuilderSeal<ChainBlindSeal>>> = bmap![];
        let assignment_name = FieldName::from("beneficiary");
        for (asset_id, transfer_info) in transfer_info_map.clone() {
            let change_amount = transfer_info.asset_spend.change_amount;
            let iface = transfer_info.asset_iface.to_typename();
            let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
            let mut asset_transition_builder =
                runtime.transition_builder(contract_id, iface.clone(), None::<&str>)?;

            let assignment_id = asset_transition_builder.assignments_type(&assignment_name);
            let assignment_id = assignment_id.ok_or(InternalError::Unexpected)?;

            for (opout, _state) in
                runtime.state_for_outpoints(contract_id, prev_outputs.iter().copied())?
            {
                asset_transition_builder = asset_transition_builder
                    .add_input(opout)
                    .map_err(InternalError::from)?;
            }

            if change_amount > 0 {
                let change_utxo = self._get_change_utxo(
                    &mut change_utxo_option,
                    &mut change_utxo_idx,
                    input_outpoints.clone(),
                    unspents.clone(),
                )?;
                let seal = ExplicitSeal::with(
                    CloseMethod::OpretFirst,
                    RgbTxid::from_str(&change_utxo.txid).unwrap().into(),
                    change_utxo.vout,
                );
                let seal = GraphSeal::from(seal);
                let change = TypedState::Amount(change_amount);
                asset_transition_builder = asset_transition_builder
                    .add_raw_state(assignment_id, seal, change)
                    .map_err(InternalError::from)?;
            };

            let mut beneficiaries: Vec<BuilderSeal<ChainBlindSeal>> = vec![];
            for recipient in transfer_info.recipients.clone() {
                let seal: BuilderSeal<GraphSeal> = match recipient.recipient_data {
                    RecipientData::BlindedUTXO(secret_seal) => BuilderSeal::Concealed(secret_seal),
                    RecipientData::WitnessData {
                        script_buf,
                        blinding,
                        ..
                    } => {
                        let vout = outputs
                            .iter()
                            .position(|o| o.script_pubkey == script_buf)
                            .unwrap() as u32;
                        let graph_seal = if let Some(blinding) = blinding {
                            GraphSeal::with_vout(CloseMethod::OpretFirst, vout, blinding)
                        } else {
                            GraphSeal::new_vout(CloseMethod::OpretFirst, vout)
                        };
                        BuilderSeal::Revealed(graph_seal)
                    }
                };

                beneficiaries.push(seal);
                asset_transition_builder = asset_transition_builder
                    .add_raw_state(assignment_id, seal, TypedState::Amount(recipient.amount))
                    .map_err(InternalError::from)?;
            }

            let transition = asset_transition_builder
                .complete_transition(contract_id)
                .map_err(InternalError::from)?;
            all_transitions.insert(contract_id, transition);
            asset_beneficiaries.insert(asset_id.clone(), beneficiaries);

            let asset_transfer_dir = transfer_dir.join(asset_id.clone());
            if asset_transfer_dir.is_dir() {
                fs::remove_dir_all(asset_transfer_dir.clone())?;
            }
            fs::create_dir_all(asset_transfer_dir.clone())?;

            // save asset transfer data to file (for send_end)
            let serialized_info =
                serde_json::to_string(&transfer_info).map_err(InternalError::from)?;
            let info_file = asset_transfer_dir.join(TRANSFER_DATA_FILE);
            fs::write(info_file, serialized_info)?;
        }

        let mut contract_inputs = HashMap::<ContractId, Vec<RgbOutpoint>>::new();
        let mut blank_state = HashMap::<ContractId, BTreeMap<Opout, TypedState>>::new();
        for outpoint in prev_outputs {
            for id in runtime.contracts_by_outpoints([outpoint])? {
                contract_inputs.entry(id).or_default().push(outpoint);
                let cid_str = id.to_string();
                if transfer_info_map.contains_key(&cid_str) {
                    continue;
                }
                blank_state
                    .entry(id)
                    .or_default()
                    .extend(runtime.state_for_outpoints(id, [outpoint])?);
            }
        }

        let mut blank_allocations: HashMap<String, u64> = HashMap::new();
        for (cid, opouts) in blank_state {
            let asset_iface = self._get_asset_iface(cid, runtime)?;
            let iface = asset_iface.to_typename();
            let mut blank_builder = runtime.blank_builder(cid, iface.clone())?;
            let mut moved_amount = 0;
            for (opout, state) in opouts {
                if let TypedState::Amount(amt) = &state {
                    moved_amount += amt
                }
                let change_utxo = self._get_change_utxo(
                    &mut change_utxo_option,
                    &mut change_utxo_idx,
                    input_outpoints.clone(),
                    unspents.clone(),
                )?;
                let seal = ExplicitSeal::with(
                    CloseMethod::OpretFirst,
                    RgbTxid::from_str(&change_utxo.txid).unwrap().into(),
                    change_utxo.vout,
                );
                let seal = GraphSeal::from(seal);
                blank_builder = blank_builder
                    .add_input(opout)
                    .map_err(InternalError::from)?
                    .add_raw_state(opout.ty, seal, state)
                    .map_err(InternalError::from)?;
            }
            let blank_transition = blank_builder
                .complete_transition(cid)
                .map_err(InternalError::from)?;
            all_transitions.insert(cid, blank_transition);
            blank_allocations.insert(cid.to_string(), moved_amount);
        }

        for (id, transition) in all_transitions {
            let inputs = contract_inputs.remove(&id).unwrap_or_default();
            for (input, txin) in psbt.inputs.iter_mut().zip(&psbt.unsigned_tx.input) {
                let prevout = txin.previous_output;
                let outpoint = RgbOutpoint::new(prevout.txid.to_byte_array().into(), prevout.vout);
                if inputs.contains(&outpoint) {
                    input
                        .set_rgb_consumer(id, transition.id())
                        .map_err(InternalError::from)?;
                }
            }
            psbt.push_rgb_transition(transition)
                .map_err(InternalError::from)?;
        }

        let bundles = psbt.rgb_bundles().map_err(InternalError::from)?;
        let (opreturn_index, _) = psbt
            .unsigned_tx
            .output
            .iter()
            .enumerate()
            .find(|(_, o)| o.script_pubkey.is_op_return())
            .expect("psbt should have an op_return output");
        let (_, opreturn_output) = psbt
            .outputs
            .iter_mut()
            .enumerate()
            .find(|(i, _)| i == &opreturn_index)
            .unwrap();
        opreturn_output
            .set_opret_host()
            .expect("cannot set opret host");
        psbt.rgb_bundle_to_lnpbp4().map_err(InternalError::from)?;
        let anchor = psbt
            .dbc_conclude(CloseMethod::OpretFirst)
            .map_err(InternalError::from)?;
        let witness_txid = psbt.unsigned_tx.txid();
        runtime.consume_anchor(anchor)?;
        for (id, bundle) in bundles {
            runtime.consume_bundle(id, bundle, witness_txid.to_byte_array().into())?;
        }

        for (asset_id, _transfer_info) in transfer_info_map {
            let asset_transfer_dir = transfer_dir.join(asset_id.clone());
            let consignment_path = asset_transfer_dir.join(CONSIGNMENT_FILE);
            let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
            let beneficiaries = asset_beneficiaries[&asset_id].clone();
            let mut beneficiaries_with_txid = vec![];
            for beneficiary in beneficiaries {
                let beneficiary_with_txid = match beneficiary {
                    BuilderSeal::Revealed(seal) => BuilderSeal::Revealed(
                        seal.resolve(BpTxid::from_raw_array(witness_txid.to_byte_array())),
                    ),
                    BuilderSeal::Concealed(seal) => BuilderSeal::Concealed(seal),
                };
                beneficiaries_with_txid.push(beneficiary_with_txid);
            }
            let transfer = runtime.transfer(contract_id, beneficiaries_with_txid)?;
            transfer.save(&consignment_path)?;
        }

        // save batch transfer data to file (for send_end)
        let info_contents = InfoBatchTransfer {
            change_utxo_idx,
            blank_allocations,
            donation,
            min_confirmations,
        };
        let serialized_info = serde_json::to_string(&info_contents).map_err(InternalError::from)?;
        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        fs::write(info_file, serialized_info)?;

        Ok(())
    }

    fn _post_transfer_data(
        &self,
        recipients: &mut Vec<LocalRecipient>,
        asset_transfer_dir: PathBuf,
        asset_dir: Option<PathBuf>,
        txid: String,
    ) -> Result<(), Error> {
        let mut attachments = vec![];
        if let Some(ass_dir) = &asset_dir {
            for fp in fs::read_dir(ass_dir)? {
                let fpath = fp?.path();
                let file_path = fpath.join(MEDIA_FNAME);
                let file_bytes = std::fs::read(file_path.clone())?;
                let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
                let attachment_id = hex::encode(file_hash.to_byte_array());
                attachments.push((attachment_id, file_path))
            }
        }

        let consignment_path = asset_transfer_dir.join(CONSIGNMENT_FILE);
        for recipient in recipients {
            let recipient_id = recipient.recipient_id();
            let mut found_valid = false;
            for transport_endpoint in recipient.transport_endpoints.iter_mut() {
                if transport_endpoint.transport_type != TransportType::JsonRpc
                    || !transport_endpoint.usable
                {
                    debug!(
                        self.logger,
                        "Skipping transport endpoint {:?}", transport_endpoint
                    );
                    continue;
                }
                let proxy_url = transport_endpoint.endpoint.clone();
                let consignment_res = self.rest_client.clone().post_consignment(
                    &proxy_url,
                    recipient_id.clone(),
                    consignment_path.clone(),
                    txid.clone(),
                    recipient.vout,
                )?;
                debug!(
                    self.logger,
                    "Consignment POST response: {:?}", consignment_res
                );

                if let Some(err) = consignment_res.error {
                    if err.code == -101 {
                        return Err(Error::RecipientIDAlreadyUsed)?;
                    }
                    continue;
                } else if consignment_res.result.is_none() {
                    continue;
                } else {
                    for attachment in attachments.clone() {
                        let media_res = self.rest_client.clone().post_media(
                            &proxy_url,
                            attachment.0,
                            attachment.1,
                        )?;
                        debug!(self.logger, "Attachment POST response: {:?}", media_res);
                        if let Some(_err) = media_res.error {
                            return Err(InternalError::Unexpected)?;
                        }
                    }
                    transport_endpoint.used = true;
                    found_valid = true;
                    break;
                }
            }
            if !found_valid {
                return Err(Error::NoValidTransportEndpoint);
            }
        }

        Ok(())
    }

    fn _save_transfers(
        &self,
        txid: String,
        transfer_info_map: BTreeMap<String, InfoAssetTransfer>,
        blank_allocations: HashMap<String, u64>,
        change_utxo_idx: Option<i32>,
        status: TransferStatus,
        min_confirmations: u8,
    ) -> Result<(), Error> {
        let created_at = now().unix_timestamp();
        let expiration = Some(created_at + DURATION_SEND_TRANSFER);

        let batch_transfer = DbBatchTransferActMod {
            txid: ActiveValue::Set(Some(txid)),
            status: ActiveValue::Set(status),
            expiration: ActiveValue::Set(expiration),
            created_at: ActiveValue::Set(created_at),
            min_confirmations: ActiveValue::Set(min_confirmations),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;

        for (asset_id, transfer_info) in transfer_info_map {
            let asset_spend = transfer_info.asset_spend;
            let recipients = transfer_info.recipients;

            let asset_transfer = DbAssetTransferActMod {
                user_driven: ActiveValue::Set(true),
                batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
                asset_id: ActiveValue::Set(Some(asset_id)),
                ..Default::default()
            };
            let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;

            for (input_idx, amount) in asset_spend.txo_map.clone().into_iter() {
                let db_coloring = DbColoringActMod {
                    txo_idx: ActiveValue::Set(input_idx),
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    coloring_type: ActiveValue::Set(ColoringType::Input),
                    amount: ActiveValue::Set(amount.to_string()),
                    ..Default::default()
                };
                self.database.set_coloring(db_coloring)?;
            }
            if asset_spend.change_amount > 0 {
                let db_coloring = DbColoringActMod {
                    txo_idx: ActiveValue::Set(change_utxo_idx.unwrap()),
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    coloring_type: ActiveValue::Set(ColoringType::Change),
                    amount: ActiveValue::Set(asset_spend.change_amount.to_string()),
                    ..Default::default()
                };
                self.database.set_coloring(db_coloring)?;
            }

            for recipient in recipients.clone() {
                let transfer = DbTransferActMod {
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    amount: ActiveValue::Set(recipient.amount.to_string()),
                    incoming: ActiveValue::Set(false),
                    recipient_id: ActiveValue::Set(Some(recipient.recipient_id().clone())),
                    ..Default::default()
                };
                let transfer_idx = self.database.set_transfer(transfer)?;
                for transport_endpoint in recipient.transport_endpoints {
                    self._save_transfer_transport_endpoint(transfer_idx, &transport_endpoint)?;
                }
            }
        }

        for (asset_id, amt) in blank_allocations {
            let asset_transfer = DbAssetTransferActMod {
                user_driven: ActiveValue::Set(false),
                batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
                asset_id: ActiveValue::Set(Some(asset_id)),
                ..Default::default()
            };
            let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(change_utxo_idx.unwrap()),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                coloring_type: ActiveValue::Set(ColoringType::Change),
                amount: ActiveValue::Set(amt.to_string()),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }

        Ok(())
    }

    /// Send tokens. See the [`send_begin`](Wallet::send_begin) function for details.
    ///
    /// This is the full version, requiring a wallet with private keys
    pub fn send(
        &mut self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: f32,
        min_confirmations: u8,
    ) -> Result<String, Error> {
        info!(self.logger, "Sending to: {:?}...", recipient_map);
        self._check_xprv()?;

        let unsigned_psbt = self.send_begin(
            online.clone(),
            recipient_map,
            donation,
            fee_rate,
            min_confirmations,
        )?;

        let psbt = self.sign_psbt(unsigned_psbt)?;

        self.send_end(online, psbt)
    }

    /// Prepare the PSBT to send tokens according to the given recipient map.
    ///
    /// The `recipient_map` maps Asset IDs to a vector of [`Recipient`]s. Each recipient
    /// is specified by a `recipient_id` and the `amount` to send.
    ///
    /// If `donation` is true, the resulting transaction will be broadcast (by
    /// [`send_end`](Wallet::send_end)) as soon as it's ready, without the need for recipients to
    /// acknowledge the transfer.
    /// If `donation` is false, all recipients will need to ACK the transfer before the transaction
    /// is broadcast (as part of [`refresh`](Wallet::refresh)).
    ///
    /// This is the first half of the partial version, requiring no private keys.
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the `send_end` function for broadcasting.
    ///
    /// Returns a PSBT ready to be signed
    pub fn send_begin(
        &mut self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: f32,
        min_confirmations: u8,
    ) -> Result<String, Error> {
        info!(self.logger, "Sending (begin) to: {:?}...", recipient_map);
        self._check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        let mut db_data = self.database.get_db_data(false)?;
        self._handle_expired_transfers(&mut db_data)?;

        let receive_ids: Vec<String> = recipient_map
            .values()
            .map(|r| r.iter().map(|r| r.recipient_id()).collect())
            .collect();
        let mut hasher = DefaultHasher::new();
        receive_ids.hash(&mut hasher);
        let transfer_dir = self
            .wallet_dir
            .join(TRANSFER_DIR)
            .join(hasher.finish().to_string());
        if transfer_dir.exists() {
            fs::remove_dir_all(&transfer_dir)?;
        }

        // input selection
        let utxos = self.database.get_unspent_txos(db_data.txos.clone())?;

        let unspents = self.database.get_rgb_allocations(
            utxos,
            Some(db_data.colorings.clone()),
            Some(db_data.batch_transfers.clone()),
            Some(db_data.asset_transfers.clone()),
        )?;

        let mut input_unspents = unspents.clone();
        input_unspents.retain(|u| {
            !((u.rgb_allocations
                .iter()
                .any(|a| a.incoming && a.status.pending()))
                || (u
                    .rgb_allocations
                    .iter()
                    .any(|a| !a.incoming && a.status.waiting_counterparty())))
        });

        let mut runtime = self._rgb_runtime()?;
        let mut witness_recipients: HashMap<ScriptBuf, u64> = HashMap::new();
        let mut transfer_info_map: BTreeMap<String, InfoAssetTransfer> = BTreeMap::new();
        for (asset_id, recipients) in recipient_map {
            self.database.check_asset_exists(asset_id.clone())?;

            let mut local_recipients: Vec<LocalRecipient> = vec![];
            let mut recipient_vout = 0;
            for recipient in recipients.clone() {
                self._check_transport_endpoints(&recipient.transport_endpoints)?;

                let mut transport_endpoints: Vec<LocalTransportEndpoint> = vec![];
                let mut found_valid = false;
                for endpoint_str in &recipient.transport_endpoints {
                    let transport_endpoint = TransportEndpoint::new(endpoint_str.clone())?;
                    let mut local_transport_endpoint = LocalTransportEndpoint {
                        transport_type: transport_endpoint.transport_type,
                        endpoint: transport_endpoint.endpoint.clone(),
                        used: false,
                        usable: false,
                    };
                    if let Ok(server_info) = self
                        .rest_client
                        .clone()
                        .get_info(&transport_endpoint.endpoint)
                    {
                        if let Some(info) = server_info.result {
                            if info.protocol_version == *PROXY_PROTOCOL_VERSION {
                                local_transport_endpoint.usable = true;
                                found_valid = true;
                            }
                        }
                    };
                    transport_endpoints.push(local_transport_endpoint);
                }

                if !found_valid {
                    return Err(Error::InvalidTransportEndpoints {
                        details: s!("no valid transport endpoints"),
                    });
                }

                let vout = match &recipient.recipient_data {
                    RecipientData::WitnessData {
                        script_buf,
                        amount_sat,
                        ..
                    } => {
                        witness_recipients.insert(script_buf.clone(), *amount_sat);
                        let vout = recipient_vout;
                        recipient_vout += 1;
                        Some(vout)
                    }
                    _ => None,
                };

                local_recipients.push(LocalRecipient {
                    recipient_data: recipient.recipient_data,
                    amount: recipient.amount,
                    transport_endpoints,
                    vout,
                })
            }

            let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
            let asset_iface = self._get_asset_iface(contract_id, &runtime)?;
            let amount: u64 = recipients.iter().map(|a| a.amount).sum();
            let asset_spend = self._select_rgb_inputs(
                asset_id.clone(),
                amount,
                input_unspents.clone(),
                Some(db_data.asset_transfers.clone()),
                Some(db_data.batch_transfers.clone()),
                Some(db_data.colorings.clone()),
            )?;
            let transfer_info = InfoAssetTransfer {
                recipients: local_recipients.clone(),
                asset_spend,
                asset_iface,
            };
            transfer_info_map.insert(asset_id.clone(), transfer_info);
        }

        // prepare BDK PSBT
        let mut all_inputs: Vec<BdkOutPoint> = transfer_info_map
            .values()
            .cloned()
            .map(|i| i.asset_spend.input_outpoints)
            .collect::<Vec<Vec<BdkOutPoint>>>()
            .concat();
        all_inputs.sort();
        all_inputs.dedup();
        let psbt = self._try_prepare_psbt(
            &input_unspents,
            &mut all_inputs,
            &witness_recipients,
            fee_rate,
        )?;
        let vbytes = psbt.extract_tx().vsize() as f32;
        let updated_fee_rate = ((vbytes + OPRET_VBYTES) / vbytes) * fee_rate;
        let psbt = self._try_prepare_psbt(
            &input_unspents,
            &mut all_inputs,
            &witness_recipients,
            updated_fee_rate,
        )?;
        let mut psbt = PartiallySignedTransaction::from_str(&psbt.to_string()).unwrap();
        let all_inputs: Vec<OutPoint> = all_inputs
            .iter()
            .map(|i| OutPoint {
                txid: Txid::from_str(&i.txid.to_string()).unwrap(),
                vout: i.vout,
            })
            .collect();

        // prepare RGB PSBT
        self._prepare_rgb_psbt(
            &mut psbt,
            all_inputs,
            transfer_info_map.clone(),
            transfer_dir.clone(),
            donation,
            unspents,
            &mut runtime,
            min_confirmations,
        )?;

        // rename transfer directory
        let txid = psbt.clone().extract_tx().txid().to_string();
        let new_transfer_dir = self.wallet_dir.join(TRANSFER_DIR).join(txid);
        fs::rename(transfer_dir, new_transfer_dir)?;

        info!(self.logger, "Send (begin) completed");
        Ok(psbt.to_string())
    }

    /// Complete the send operation by saving the PSBT to disk, POSTing consignments to the proxy
    /// server, saving the transfer to DB and broadcasting the provided PSBT, if appropriate.
    ///
    /// This is the second half of the partial version. The provided PSBT, prepared with the
    /// `send_begin` function, needs to have already been signed.
    ///
    /// Returns the TXID of the signed PSBT that's been saved and optionally broadcast
    pub fn send_end(&self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger, "Sending (end)...");
        self._check_online(online)?;

        // save signed PSBT
        let psbt = BdkPsbt::from_str(&signed_psbt)?;
        let txid = psbt.clone().extract_tx().txid().to_string();
        let transfer_dir = self.wallet_dir.join(TRANSFER_DIR).join(txid.clone());
        let psbt_out = transfer_dir.join(SIGNED_PSBT_FILE);
        fs::write(psbt_out, psbt.to_string())?;

        // restore transfer data
        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        let serialized_info = fs::read_to_string(info_file)?;
        let info_contents: InfoBatchTransfer =
            serde_json::from_str(&serialized_info).map_err(InternalError::from)?;
        let blank_allocations = info_contents.blank_allocations;
        let change_utxo_idx = info_contents.change_utxo_idx;
        let donation = info_contents.donation;
        let mut transfer_info_map: BTreeMap<String, InfoAssetTransfer> = BTreeMap::new();
        for ass_transf_dir in fs::read_dir(transfer_dir)? {
            let asset_transfer_dir = ass_transf_dir?.path();
            if !asset_transfer_dir.is_dir() {
                continue;
            }
            let info_file = asset_transfer_dir.join(TRANSFER_DATA_FILE);
            let serialized_info = fs::read_to_string(info_file)?;
            let mut info_contents: InfoAssetTransfer =
                serde_json::from_str(&serialized_info).map_err(InternalError::from)?;
            let asset_id: String = asset_transfer_dir
                .file_name()
                .expect("valid directory name")
                .to_str()
                .expect("should be possible to convert path to a string")
                .to_string();

            // post consignment(s) and optional media
            let ass_dir = self.wallet_dir.join(ASSETS_DIR).join(asset_id.clone());
            let asset_dir = if ass_dir.is_dir() {
                Some(ass_dir)
            } else {
                None
            };
            self._post_transfer_data(
                &mut info_contents.recipients,
                asset_transfer_dir,
                asset_dir,
                txid.clone(),
            )?;

            transfer_info_map.insert(asset_id, info_contents.clone());
        }

        // broadcast PSBT if donation and finally save transfer to DB
        let status = if donation {
            self._broadcast_psbt(psbt)?;
            TransferStatus::WaitingConfirmations
        } else {
            TransferStatus::WaitingCounterparty
        };
        self._save_transfers(
            txid.clone(),
            transfer_info_map,
            blank_allocations,
            change_utxo_idx,
            status,
            info_contents.min_confirmations,
        )?;

        self.update_backup_info(false)?;

        info!(self.logger, "Send (end) completed");
        Ok(txid)
    }

    /// Send bitcoins using the internal vanilla wallet.
    ///
    /// Returns the TXID of the broadcasted transaction
    pub fn send_btc(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: f32,
    ) -> Result<String, Error> {
        info!(self.logger, "Sending BTC...");
        self._check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        self._sync_db_txos()?;

        let address = BdkAddress::from_str(&address)?;
        if !address.is_valid_for_network(self._bitcoin_network().into()) {
            return Err(Error::InvalidAddress {
                details: s!("belongs to another network"),
            });
        }

        let unspendable = self._get_unspendable_bdk_outpoints()?;

        let mut tx_builder = self.bdk_wallet.build_tx();
        tx_builder
            .unspendable(unspendable)
            .add_recipient(address.script_pubkey(), amount)
            .fee_rate(FeeRate::from_sat_per_vb(fee_rate));

        let mut psbt = tx_builder
            .finish()
            .map_err(|e| match e {
                bdk::Error::InsufficientFunds { needed, available } => {
                    Error::InsufficientBitcoins { needed, available }
                }
                bdk::Error::OutputBelowDustLimit(_) => Error::OutputBelowDustLimit,
                _ => Error::from(InternalError::from(e)),
            })?
            .0;

        self._sign_psbt(&mut psbt)?;

        let tx = self._broadcast_psbt(psbt)?;

        info!(self.logger, "Send BTC completed");
        Ok(tx.txid().to_string())
    }
}

pub(crate) mod backup;

#[cfg(test)]
mod test;
