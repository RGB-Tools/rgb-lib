//! RGB wallet
//!
//! This module defines the [`Wallet`] structure and all its related data.

use amplify::{bmap, bset, empty, s, Wrapper};
use amplify_num::hex::FromHex;
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::Network as BdkNetwork;
use bdk::blockchain::{
    Blockchain, ConfigurableBlockchain, ElectrumBlockchain, ElectrumBlockchainConfig,
};
use bdk::database::any::SqliteDbConfiguration as BdkSqliteDbConfiguration;
use bdk::database::{
    ConfigurableDatabase as BdkConfigurableDatabase, SqliteDatabase as BdkSqliteDatabase,
};
use bdk::keys::bip39::{Language, Mnemonic};
use bdk::keys::{DerivableKey, ExtendedKey};
use bdk::wallet::AddressIndex;
use bdk::{FeeRate, KeychainKind, LocalUtxo, SignOptions, SyncOptions, Wallet as BdkWallet};
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::{sha256, Hash as Sha256Hash};
use bitcoin::psbt::serialize::Deserialize as BitcoinDeserialize;
use bitcoin::psbt::PartiallySignedTransaction;
use bitcoin::util::bip32::ExtendedPubKey;
use bitcoin::Txid;
use bitcoin::{Address, OutPoint, Transaction};
use bp::seals::txout::blind::ConcealedSeal;
use bp::seals::txout::{CloseMethod, ExplicitSeal};
use chrono::NaiveDateTime;
use commit_verify::commit_verify::CommitVerify;
use electrum_client::{Client as ElectrumClient, ConfigBuilder, ElectrumApi, Param};
use futures::executor::block_on;
use internet2::addr::ServiceAddr;
use invoice::{
    AmountExt, Beneficiary, ConsignmentEndpoint as InvoiceConsignmentEndpoint,
    Invoice as UniversalInvoice,
};
use lnpbp::chain::{AssetId, Chain as RgbNetwork};
use psbt::Psbt;
use reqwest::blocking::Client as RestClient;
use rgb::blank::{BlankBundle, Error as BlankError};
use rgb::fungible::allocation::{AllocatedValue, OutpointValue as RgbOutpointValue, UtxobValue};
use rgb::psbt::{RgbExt, RgbInExt};
use rgb::{
    seal, Consignment, Contract, ContractId, IntoRevealedSeal, Node, OutpointState, StateTransfer,
    TransitionBundle,
};
use rgb121::{
    Asset as Rgb121Asset, FieldType as Rgb121FieldType, FileAttachment,
    OwnedRightType as Rgb121OwnedRightType, Rgb121, SCHEMA_ID_BECH32 as RGB121_SCHEMA_ID,
};
use rgb20::schema::FieldType as Rgb20FieldType;
use rgb20::{Asset as Rgb20Asset, Rgb20};
use rgb_core::schema::OwnedRightType;
use rgb_core::vm::embedded::constants::{
    STATE_TYPE_OWNERSHIP_RIGHT, TRANSITION_TYPE_VALUE_TRANSFER,
};
use rgb_core::{
    Assignment, AttachmentId, Metadata as RgbMetadata, NodeId, OwnedRights, ParentOwnedRights,
    SealEndpoint, Transition, TypedAssignments, Validator, Validity,
};
use rgb_lib_migration::{Migrator, MigratorTrait};
use rgb_node::{rgbd, Config};
use rgb_rpc::client::Client;
use rgb_rpc::{ContractValidity, Reveal};
use sea_orm::{ActiveValue, ConnectOptions, Database};
use serde::{Deserialize, Serialize};
use slog::{debug, error, info, Logger};
use std::cmp::min;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::panic;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use stens::AsciiString;
use stored::Config as StoreConfig;
use strict_encoding::{strict_deserialize, strict_serialize, StrictDecode, StrictEncode};

use crate::api::Proxy;
use crate::database::entities::asset_rgb121::Model as DbAssetRgb121;
use crate::database::entities::asset_rgb20::Model as DbAssetRgb20;
use crate::database::entities::asset_transfer::{
    ActiveModel as DbAssetTransferActMod, Model as DbAssetTransfer,
};
use crate::database::entities::batch_transfer::{
    ActiveModel as DbBatchTransferActMod, Model as DbBatchTransfer,
};
use crate::database::entities::coloring::{ActiveModel as DbColoringActMod, Model as DbColoring};
use crate::database::entities::consignment_endpoint::{
    ActiveModel as DbConsignmentEndpointActMod, Model as DbConsignmentEndpoint,
};
use crate::database::entities::transfer::{ActiveModel as DbTransferActMod, Model as DbTransfer};
use crate::database::entities::transfer_consignment_endpoint::{
    ActiveModel as DbTransferConsignmentEndpointActMod, Model as DbTransferConsignmentEndpoint,
};
use crate::database::entities::txo::{ActiveModel as DbTxoActMod, Model as DbTxo};
use crate::database::enums::{ColoringType, ConsignmentEndpointProtocol, TransferStatus};
use crate::database::{
    DbData, LocalConsignmentEndpoint, LocalRecipient, LocalRgbAllocation, LocalUnspent,
    RgbLibDatabase, TransferData,
};
use crate::error::{Error, InternalError};
use crate::utils::{
    calculate_descriptor_from_xprv, calculate_descriptor_from_xpub, get_txid, now, setup_logger,
    BitcoinNetwork,
};

const RGB_DB_NAME: &str = "rgb_db";
const BDK_DB_NAME: &str = "bdk_db";

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

const MAX_LEN_NAME: u16 = 256;
const MAX_LEN_TICKER: u16 = 8;
const MAX_PRECISION: u8 = 18;

const UTXO_SIZE: u32 = 1000;
const UTXO_NUM: u8 = 5;

const MIN_CONFIRMATIONS: u8 = 1;

const MAX_ALLOCATIONS_PER_UTXO: u32 = 5;

const MAX_CONSIGNMENT_ENDPOINTS: u8 = 3;

const MIN_FEE_RATE: f32 = 1.0;
const MAX_FEE_RATE: f32 = 1000.0;

const DURATION_SEND_TRANSFER: i64 = 3600;
const DURATION_RCV_TRANSFER: u32 = 86400;

const ELECTRUM_TIMEOUT: u8 = 4;
const PROXY_TIMEOUT: u8 = 90;

const PROXY_PROTOCOL_VERSION: &str = "0.1";

/// The type of an asset
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum AssetType {
    /// Rgb20 schema for fungible assets
    Rgb20,
    /// Rgb121 schema for non-fungible assets
    Rgb121,
}

/// An RGB20 fungible asset
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetRgb20 {
    /// ID of the asset
    pub asset_id: String,
    /// Ticker of the asset
    pub ticker: String,
    /// Name of the asset
    pub name: String,
    /// Precision, also known as divisibility, of the asset
    pub precision: u8,
    /// Current balance of the asset
    pub balance: Balance,
}

impl AssetRgb20 {
    fn from_db_asset(x: DbAssetRgb20, balance: Balance) -> AssetRgb20 {
        AssetRgb20 {
            asset_id: x.asset_id,
            ticker: x.ticker,
            name: x.name,
            precision: x.precision,
            balance,
        }
    }
}

/// An asset media file
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Media {
    /// Path of the media file
    pub file_path: String,
    /// Mime of the media file
    pub mime: String,
}

/// Metadata of an asset
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// Asset schema type
    pub asset_type: AssetType,
    /// Total issued amount
    pub issued_supply: u64,
    /// Timestamp of asset genesis
    pub timestamp: i64,
    /// Assset name
    pub name: String,
    /// Assset precision
    pub precision: u8,
    /// Assset ticker
    pub ticker: Option<String>,
    /// Assset description
    pub description: Option<String>,
    /// Assset parent ID
    pub parent_id: Option<String>,
}

/// An RGB121 collectible asset
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetRgb121 {
    /// ID of the asset
    pub asset_id: String,
    /// Name of the asset
    pub name: String,
    /// Description of the asset
    pub description: Option<String>,
    /// Precision, also known as divisibility, of the asset
    pub precision: u8,
    /// Current balance of the asset
    pub balance: Balance,
    /// List of asset data file paths
    pub data_paths: Vec<Media>,
    /// Parent ID of the asset
    pub parent_id: Option<String>,
}

impl AssetRgb121 {
    fn from_db_asset(
        x: DbAssetRgb121,
        balance: Balance,
        assets_dir: PathBuf,
    ) -> Result<AssetRgb121, Error> {
        let mut data_paths = vec![];
        let asset_dir = assets_dir.join(x.asset_id.clone());
        if asset_dir.is_dir() {
            for fp in fs::read_dir(asset_dir)? {
                let fpath = fp?.path();
                let file_path = fpath.join(MEDIA_FNAME).to_string_lossy().to_string();
                let mime = fs::read_to_string(fpath.join(MIME_FNAME))?;
                data_paths.push(Media { file_path, mime });
            }
        }
        Ok(AssetRgb121 {
            asset_id: x.asset_id,
            description: x.description,
            name: x.name,
            precision: x.precision,
            balance,
            data_paths,
            parent_id: x.parent_id,
        })
    }
}

/// List of known assets, grouped by asset type
pub struct Assets {
    /// List of Rgb20 assets
    pub rgb20: Option<Vec<AssetRgb20>>,
    /// List of Rgb121 assets
    pub rgb121: Option<Vec<AssetRgb121>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AssetSpend {
    txo_map: HashMap<i64, u64>,
    input_outpoints: Vec<OutPoint>,
    change_amount: u64,
}

/// An asset balance
///
/// The settled balance includes allocations created by operations that have completed and are in a
/// final status.
/// The future balance also includes operations that have not yet completed or are not yet final,
/// reflecting what the balance will be once all pending operations will have settled.
/// The spendable balance is a subset of the settled balance, excluding allocations on UTXOs that
/// are supporting any pending operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Balance {
    /// Settled balance
    pub settled: u64,
    /// Future balance
    pub future: u64,
    /// Spendable balance
    pub spendable: u64,
}

trait BlankBundleRgb121 {
    fn blank_rgb121(
        prev_state: &BTreeMap<OutPoint, BTreeSet<OutpointState>>,
        new_outpoints: &BTreeMap<OwnedRightType, (OutPoint, CloseMethod)>,
    ) -> Result<TransitionBundle, BlankError>;
}

impl BlankBundleRgb121 for TransitionBundle {
    fn blank_rgb121(
        prev_state: &BTreeMap<OutPoint, BTreeSet<OutpointState>>,
        new_outpoints: &BTreeMap<OwnedRightType, (OutPoint, CloseMethod)>,
    ) -> Result<TransitionBundle, BlankError> {
        let mut transitions: BTreeMap<Transition, BTreeSet<u16>> = bmap! {};

        for (tx_outpoint, inputs) in prev_state {
            let mut parent_owned_rights: BTreeMap<NodeId, BTreeMap<OwnedRightType, Vec<u16>>> =
                bmap! {};
            let mut owned_rights: BTreeMap<OwnedRightType, TypedAssignments> = bmap! {};
            for OutpointState {
                node_outpoint: input,
                state,
            } in inputs
            {
                parent_owned_rights
                    .entry(input.node_id)
                    .or_default()
                    .entry(input.ty)
                    .or_default()
                    .push(input.no);
                let (op, close_method) = new_outpoints
                    .get(&input.ty)
                    .ok_or(BlankError::NoOutpoint(input.ty))?;
                let new_seal = seal::Revealed::new(*close_method, *op);
                let new_assignments = state.to_revealed_assignment_vec(new_seal);
                owned_rights.insert(input.ty, new_assignments);
            }
            let transition = Transition::with(
                TRANSITION_TYPE_VALUE_TRANSFER,
                empty!(),
                empty!(),
                OwnedRights::from(owned_rights),
                empty!(),
                ParentOwnedRights::from(parent_owned_rights),
            );
            transitions.insert(transition, bset! { tx_outpoint.vout as u16 });
        }

        TransitionBundle::try_from(transitions).map_err(BlankError::from)
    }
}

/// Data for a UTXO blinding
#[derive(Debug)]
pub struct BlindData {
    /// Bech32 invoice
    pub invoice: String,
    /// Blinded UTXO
    pub blinded_utxo: String,
    /// Secret used to blind the UTXO
    pub blinding_secret: u64,
    /// Expiration of the `blinded_utxo`
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
        ConcealedSeal::from_str(&blinded_utxo)?;
        Ok(BlindedUTXO { blinded_utxo })
    }
}

/// An RGB consignment endpoint
#[derive(Debug)]
pub struct ConsignmentEndpoint {
    /// Endpoint address
    pub endpoint: String,
    /// Endpoint protocol
    pub protocol: ConsignmentEndpointProtocol,
}

impl ConsignmentEndpoint {
    /// Check that the provided [`ConsignmentEndpoint::consignment_endpoint`] is valid
    pub fn new(consignment_endpoint: String) -> Result<Self, Error> {
        let invoice_consignment_endpoint =
            InvoiceConsignmentEndpoint::from_str(&consignment_endpoint)?;
        ConsignmentEndpoint::try_from(invoice_consignment_endpoint)
    }

    /// Return the protocol of this consignment endpoint
    pub fn protocol(&self) -> ConsignmentEndpointProtocol {
        self.protocol
    }
}

impl TryFrom<InvoiceConsignmentEndpoint> for ConsignmentEndpoint {
    type Error = Error;

    fn try_from(x: InvoiceConsignmentEndpoint) -> Result<Self, Self::Error> {
        match x {
            InvoiceConsignmentEndpoint::Storm(addr) => Ok(ConsignmentEndpoint {
                endpoint: addr.to_string(),
                protocol: ConsignmentEndpointProtocol::Storm,
            }),
            InvoiceConsignmentEndpoint::RgbHttpJsonRpc(addr) => Ok(ConsignmentEndpoint {
                endpoint: addr,
                protocol: ConsignmentEndpointProtocol::RgbHttpJsonRpc,
            }),
            _ => Err(Error::UnsupportedConsignmentEndpointProtocol),
        }
    }
}

/// Supported database types
#[derive(Clone)]
pub enum DatabaseType {
    /// A SQLite database
    Sqlite,
}

#[derive(Debug, Deserialize, Serialize)]
struct InfoBatchTransfer {
    change_utxo_idx: i64,
    blank_allocations: HashMap<String, u64>,
    donation: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct InfoAssetTransfer {
    recipients: Vec<LocalRecipient>,
    asset_spend: AssetSpend,
    asset_type: AssetType,
}

/// An RGB invoice
pub struct Invoice {
    /// The RGB invoice in bech32 encoding
    bech32_invoice: String,
    /// The decoded RGB invoice
    invoice_data: InvoiceData,
}

impl Invoice {
    /// Parse the provided [`Invoice::bech32_invoice`].
    /// Throws an error if the provided string is not a valid bech32 invoice.
    pub fn new(bech32_invoice: String) -> Result<Self, Error> {
        let decoded = UniversalInvoice::from_str(&bech32_invoice)?;
        let asset_id = decoded.rgb_asset().map(|rid| rid.to_string());
        let amount = match decoded.amount() {
            AmountExt::Any => None,
            AmountExt::Normal(v) => Some(*v),
            AmountExt::Milli(_v, _m) => return Err(Error::UnsupportedInvoice),
        };
        let blinded_utxo = match decoded.beneficiary() {
            Beneficiary::BlindUtxo(concealed_seal) => concealed_seal.to_string(),
            _ => return Err(Error::UnsupportedInvoice),
        };
        let expiration_timestamp = decoded.expiry().as_ref().map(|exp| exp.timestamp());
        let consignment_endpoints: Vec<String> = decoded
            .consignment_endpoints()
            .iter()
            .map(|e| e.to_string())
            .collect();
        let invoice_data = InvoiceData {
            blinded_utxo,
            asset_id,
            amount,
            expiration_timestamp,
            consignment_endpoints,
        };

        Ok(Invoice {
            bech32_invoice,
            invoice_data,
        })
    }

    /// Parse the provided [`Invoice::invoice_data`].
    /// Throws an error if the provided data is invalid.
    pub fn from_invoice_data(invoice_data: InvoiceData) -> Result<Self, Error> {
        let concealed_seal = ConcealedSeal::from_str(&invoice_data.blinded_utxo)?;
        let beneficiary = Beneficiary::BlindUtxo(concealed_seal);
        let rgb_asset_id = if let Some(cid) = invoice_data.asset_id.clone() {
            let contract_id = ContractId::from_str(&cid).map_err(InternalError::from)?;
            let rid = AssetId::from(contract_id);
            Some(rid)
        } else {
            None
        };
        let mut invoice = UniversalInvoice::new(beneficiary, invoice_data.amount, rgb_asset_id);
        if let Some(exp) = invoice_data.expiration_timestamp {
            if let Some(expiry) = NaiveDateTime::from_timestamp_opt(exp, 0) {
                invoice.set_expiry(expiry);
            }
        }
        for endpoint in invoice_data.consignment_endpoints.clone() {
            invoice.add_consignment_endpoint(InvoiceConsignmentEndpoint::from_str(&endpoint)?);
        }

        let bech32_invoice = invoice.to_string();

        Ok(Invoice {
            bech32_invoice,
            invoice_data,
        })
    }

    /// Return the data associated with this [`Invoice`]
    pub fn invoice_data(&self) -> InvoiceData {
        self.invoice_data.clone()
    }

    /// Return the bech32 invoice string associated with this [`Invoice`]
    pub fn bech32_invoice(&self) -> String {
        self.bech32_invoice.clone()
    }
}

/// A decoded RGB invoice
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct InvoiceData {
    /// Blinded UTXO
    pub blinded_utxo: String,
    /// RGB asset ID
    pub asset_id: Option<String>,
    /// RGB amount
    pub amount: Option<u64>,
    /// Invoice expiration
    pub expiration_timestamp: Option<i64>,
    /// Consignment endpoints
    pub consignment_endpoints: Vec<String>,
}

/// Data for operations that require the wallet to be online
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Online {
    /// ID to tell different Online structs apart
    pub id: u64,
    /// URL of the electrum server to be used for online operations
    pub electrum_url: String,
}

/// Bitcoin transaction outpoint
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// An RGB recipient
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Recipient {
    /// Blinded UTXO
    pub blinded_utxo: String,
    /// RGB amount
    pub amount: u64,
    /// Consignment endpoints
    pub consignment_endpoints: Vec<String>,
}

/// A transfer refresh filter
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefreshFilter {
    /// Transfer status
    pub status: RefreshTransferStatus,
    /// Whether the transfer is incoming
    pub incoming: bool,
}

/// The pending status of a [`Transfer`] (eligible for refresh)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
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

/// An RGB transfer
#[derive(Clone, Debug)]
pub struct Transfer {
    /// ID of the transfer
    pub idx: i64,
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
    /// Blinded UTXO of the transfer's recipient
    pub blinded_utxo: Option<String>,
    /// Unblinded UTXO of the transfer's recipient
    pub unblinded_utxo: Option<Outpoint>,
    /// Change UTXO for the transfer's sender
    pub change_utxo: Option<Outpoint>,
    /// Secret used to blind the UTXO
    pub blinding_secret: Option<u64>,
    /// Expiration of the transfer
    pub expiration: Option<i64>,
    /// Consignment endpoints of the transfer
    pub consignment_endpoints: Vec<TransferConsignmentEndpoint>,
}

impl Transfer {
    fn from_db_transfer(
        x: DbTransfer,
        td: TransferData,
        consignment_endpoints: Vec<TransferConsignmentEndpoint>,
    ) -> Transfer {
        let blinding_secret = x.blinding_secret.map(|bs| {
            bs.parse::<u64>()
                .expect("DB should contain a valid u64 value")
        });
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
            blinded_utxo: x.blinded_utxo,
            unblinded_utxo: td.unblinded_utxo,
            change_utxo: td.change_utxo,
            blinding_secret,
            expiration: td.expiration,
            consignment_endpoints,
        }
    }
}

/// An RGB transfer consignment endpoint
#[derive(Clone, Debug)]
pub struct TransferConsignmentEndpoint {
    /// Endpoint address
    pub endpoint: String,
    /// Endpoint protocol
    pub protocol: ConsignmentEndpointProtocol,
    /// Whether the endpoint has been used
    pub used: bool,
}

impl TransferConsignmentEndpoint {
    fn from_db_transfer_consignment_endpoint(
        x: &DbTransferConsignmentEndpoint,
        ce: &DbConsignmentEndpoint,
    ) -> TransferConsignmentEndpoint {
        TransferConsignmentEndpoint {
            endpoint: ce.endpoint.clone(),
            protocol: ce.protocol,
            used: x.used,
        }
    }
}

/// The type of an RGB transfer
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferKind {
    /// A transfer that issued the asset
    Issuance,
    /// An incoming transfer
    Receive,
    /// An outgoing transfer
    Send,
}

/// A wallet unspent
#[derive(Clone, Debug)]
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

/// An unspent transaction output
#[derive(Clone, Debug)]
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
            colorable: x.colorable,
        }
    }
}

/// Wallet data provided by the user
#[derive(Clone)]
pub struct WalletData {
    /// Directory where the wallet directory is to be created
    pub data_dir: String,
    /// Bitcoin network for the wallet
    pub bitcoin_network: BitcoinNetwork,
    /// Database type for the wallet
    pub database_type: DatabaseType,
    /// Wallet xpub
    pub pubkey: String,
    /// Wallet mnemonic phrase
    pub mnemonic: Option<String>,
}

/// An RGB wallet
///
/// A `Wallet` struct holds all the data required to operate it
pub struct Wallet {
    wallet_data: WalletData,
    logger: Logger,
    watch_only: bool,
    database: Arc<RgbLibDatabase>,
    bitcoin_network: BitcoinNetwork,
    wallet_dir: PathBuf,
    bdk_wallet: BdkWallet<BdkSqliteDatabase>,
    rest_client: RestClient,
    online: Option<Online>,
    bdk_blockchain: Option<ElectrumBlockchain>,
    electrum_client: Option<ElectrumClient>,
    rgb_client: Option<Client>,
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
        let logger = setup_logger(wallet_dir.clone())?;
        info!(logger.clone(), "New wallet in '{:?}'", wallet_dir);
        let panic_logger = logger.clone();
        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            error!(panic_logger.clone(), "PANIC: {:?}", info);
            prev_hook(info);
        }));

        // BDK setup
        let bdk_db = wallet_dir.join(BDK_DB_NAME);
        let bdk_config = BdkSqliteDbConfiguration {
            path: bdk_db
                .into_os_string()
                .into_string()
                .expect("should be possible to convert path to a string"),
        };
        let bdk_database =
            BdkSqliteDatabase::from_config(&bdk_config).map_err(InternalError::from)?;
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
            let descriptor = calculate_descriptor_from_xprv(xprv, wdata.bitcoin_network, false);
            let change_descriptor =
                calculate_descriptor_from_xprv(xprv, wdata.bitcoin_network, true);
            BdkWallet::new(
                &descriptor,
                Some(&change_descriptor),
                bdk_network,
                bdk_database,
            )
            .map_err(InternalError::from)?
        } else {
            let descriptor_pub =
                calculate_descriptor_from_xpub(xpub, wdata.bitcoin_network, false)?;
            let change_descriptor_pub =
                calculate_descriptor_from_xpub(xpub, wdata.bitcoin_network, true)?;
            BdkWallet::new(
                &descriptor_pub,
                Some(&change_descriptor_pub),
                bdk_network,
                bdk_database,
            )
            .map_err(InternalError::from)?
        };

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
            bitcoin_network: wdata.bitcoin_network,
            wallet_dir,
            bdk_wallet,
            rest_client,
            online: None,
            bdk_blockchain: None,
            electrum_client: None,
            rgb_client: None,
        })
    }

    fn _bdk_blockchain(&self) -> Result<&ElectrumBlockchain, InternalError> {
        match self.bdk_blockchain {
            Some(ref x) => Ok(x),
            None => Err(InternalError::Unexpected),
        }
    }

    fn _electrum_client(&self) -> Result<&ElectrumClient, InternalError> {
        match self.electrum_client {
            Some(ref x) => Ok(x),
            None => Err(InternalError::Unexpected),
        }
    }

    fn _rgb_client(&mut self) -> Result<&mut Client, Error> {
        match self.rgb_client {
            Some(ref mut x) => Ok(x),
            None => Err(InternalError::Unexpected)?,
        }
    }

    fn _get_tx_details(&self, txid: String) -> Result<serde_json::Value, Error> {
        Ok(self._electrum_client()?.raw_call(
            "blockchain.transaction.get",
            vec![Param::String(txid), Param::Bool(true)],
        )?)
    }

    fn _check_consignment_endpoints(
        &self,
        consignment_endpoints: &Vec<String>,
    ) -> Result<(), Error> {
        if consignment_endpoints.is_empty() {
            return Err(Error::InvalidConsignmentEndpoints {
                details: s!("must provide at least a consignment endpoint"),
            });
        }
        if consignment_endpoints.len() > MAX_CONSIGNMENT_ENDPOINTS as usize {
            return Err(Error::InvalidConsignmentEndpoints {
                details: format!(
                    "library supports at max {} consignment endpoints",
                    MAX_CONSIGNMENT_ENDPOINTS
                ),
            });
        }

        Ok(())
    }

    fn _check_fee_rate(&self, fee_rate: f32) -> Result<(), Error> {
        if fee_rate < MIN_FEE_RATE {
            return Err(Error::InvalidFeeRate {
                details: format!("value under minimum {}", MIN_FEE_RATE),
            });
        } else if fee_rate > MAX_FEE_RATE {
            return Err(Error::InvalidFeeRate {
                details: format!("value above maximum {}", MAX_FEE_RATE),
            });
        }
        Ok(())
    }

    fn _sync_db_txos(&self) -> Result<(), Error> {
        debug!(self.logger, "Syncing TXOs...");
        self.bdk_wallet
            .sync(self._bdk_blockchain()?, SyncOptions { progress: None })
            .map_err(|e| Error::FailedBdkSync {
                details: e.to_string(),
            })?;

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
            .filter(|u| !db_outpoints.contains(&u.outpoint.to_string()))
            .map(DbTxoActMod::from)
            .collect();
        for new_utxo in new_utxos.iter().cloned() {
            self.database.set_txo(new_utxo)?;
        }

        Ok(())
    }

    fn _broadcast_psbt(
        &self,
        signed_psbt: PartiallySignedTransaction,
    ) -> Result<Transaction, Error> {
        let tx = signed_psbt.extract_tx();
        self._bdk_blockchain()?
            .broadcast(&tx)
            .map_err(|e| Error::FailedBroadcast {
                details: e.to_string(),
            })?;
        debug!(self.logger, "Broadcasted TX with ID '{}'", tx.txid());

        for input in tx.clone().input {
            let mut db_txo: DbTxoActMod = self
                .database
                .get_txo(Outpoint {
                    txid: input.previous_output.txid.to_string(),
                    vout: input.previous_output.vout,
                })?
                .expect("outpoint should be in the DB")
                .into();
            db_txo.spent = ActiveValue::Set(true);
            self.database.update_txo(db_txo)?;
        }

        self._sync_db_txos()?;

        Ok(tx)
    }

    fn _check_online(&self, online: Online) -> Result<(), Error> {
        let stored_online = self.online.clone();
        if stored_online.is_none() || Some(online) != stored_online {
            error!(self.logger, "Invalid online object");
            return Err(Error::InvalidOnline);
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

    fn _get_uncolorable_btc_sum(&self, unspents: &Vec<LocalUnspent>) -> u64 {
        unspents
            .iter()
            .filter(|u| !u.utxo.colorable)
            .map(|u| {
                u.utxo
                    .btc_amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value")
            })
            .sum()
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
            let updated_transfer = self._refresh_transfer(transfer, db_data, &vec![])?;
            if updated_transfer.is_none() {
                let mut updated_transfer: DbBatchTransferActMod = transfer.clone().into();
                updated_transfer.status = ActiveValue::Set(TransferStatus::Failed);
                self.database.update_batch_transfer(&mut updated_transfer)?;
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
        let max_allocs = max_allocations.unwrap_or(MAX_ALLOCATIONS_PER_UTXO - 1);
        Ok(mut_unspents
            .iter()
            .filter(|u| !exclude_utxos.contains(&u.utxo.outpoint()))
            .filter(|u| {
                (u.rgb_allocations.len() as u32) <= max_allocs
                    && u.utxo.colorable
                    && !u
                        .rgb_allocations
                        .iter()
                        .any(|a| !a.incoming && a.status.waiting_counterparty())
            })
            .cloned()
            .collect())
    }

    fn _detect_btc_unspendable_err(&self, unspents: &Vec<LocalUnspent>) -> Error {
        let available = self._get_uncolorable_btc_sum(unspents);
        if available < MIN_BTC_REQUIRED {
            Error::InsufficientBitcoins {
                needed: MIN_BTC_REQUIRED,
                available,
            }
        } else {
            Error::InsufficientAllocationSlots
        }
    }

    fn _get_utxo(
        &mut self,
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
        let mut allocatable =
            self._get_available_allocations(unspents.clone(), exclude_utxos, None)?;
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
            None => Err(self._detect_btc_unspendable_err(&unspents)),
        }
    }

    fn _save_transfer_consignment_endpoint(
        &self,
        transfer_idx: i64,
        consignment_endpoint: &LocalConsignmentEndpoint,
    ) -> Result<(), Error> {
        let consignment_endpoint_idx = match self
            .database
            .get_consignment_endpoint(consignment_endpoint.endpoint.clone())?
        {
            Some(ce) => ce.idx,
            None => self
                .database
                .set_consignment_endpoint(DbConsignmentEndpointActMod {
                    protocol: ActiveValue::Set(consignment_endpoint.protocol),
                    endpoint: ActiveValue::Set(consignment_endpoint.endpoint.clone()),
                    ..Default::default()
                })?,
        };

        self.database
            .set_transfer_consignment_endpoint(DbTransferConsignmentEndpointActMod {
                transfer_idx: ActiveValue::Set(transfer_idx),
                consignment_endpoint_idx: ActiveValue::Set(consignment_endpoint_idx),
                used: ActiveValue::Set(consignment_endpoint.used),
                ..Default::default()
            })?;

        Ok(())
    }

    /// Blind an UTXO and return the resulting [`BlindData`]
    ///
    /// Optional Asset ID and duration (in seconds) can be specified
    pub fn blind(
        &mut self,
        asset_id: Option<String>,
        amount: Option<u64>,
        duration_seconds: Option<u32>,
        consignment_endpoints: Vec<String>,
    ) -> Result<BlindData, Error> {
        info!(
            self.logger,
            "Blinding for asset '{:?}' with duration '{:?}'...", asset_id, duration_seconds
        );
        let (asset_type, rgb_asset_id) = if let Some(cid) = asset_id.clone() {
            let asset_type = self.database.get_asset_or_fail(cid.clone())?;
            let contract_id = ContractId::from_str(&cid).map_err(InternalError::from)?;
            let rid = AssetId::from(contract_id);
            (Some(asset_type), Some(rid))
        } else {
            (None, None)
        };

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

        let seal = seal::Revealed::new(CloseMethod::OpretFirst, OutPoint::from(utxo.clone()));
        let concealed_seal = seal.to_concealed_seal();
        let blinded_utxo = concealed_seal.to_string();

        let created_at = now().unix_timestamp();
        let (expiration, expiry) = if duration_seconds == Some(0) {
            (None, None)
        } else {
            let duration_seconds = duration_seconds.unwrap_or(DURATION_RCV_TRANSFER) as i64;
            let expiration = created_at + duration_seconds;
            let expiry = NaiveDateTime::from_timestamp_opt(expiration, 0);
            (Some(expiration), expiry)
        };

        let beneficiary = Beneficiary::BlindUtxo(concealed_seal);
        let mut invoice = UniversalInvoice::new(beneficiary, amount, rgb_asset_id);
        if let Some(exp) = expiry {
            invoice.set_expiry(exp);
        }
        self._check_consignment_endpoints(&consignment_endpoints)?;
        let mut consignment_endpoints_dedup = consignment_endpoints.clone();
        consignment_endpoints_dedup.sort();
        consignment_endpoints_dedup.dedup();
        if consignment_endpoints_dedup.len() != consignment_endpoints.len() {
            return Err(Error::InvalidConsignmentEndpoints {
                details: s!("no duplicate consignment endpoints allowed"),
            });
        }
        let mut endpoints: Vec<String> = vec![];
        for endpoint_str in consignment_endpoints {
            let consignment_endpoint = InvoiceConsignmentEndpoint::from_str(&endpoint_str)?;
            match consignment_endpoint {
                InvoiceConsignmentEndpoint::RgbHttpJsonRpc(ref endpoint) => {
                    invoice.add_consignment_endpoint(consignment_endpoint.clone());
                    endpoints.push(endpoint.clone());
                }
                _ => {
                    return Err(Error::UnsupportedConsignmentEndpointProtocol);
                }
            }
        }

        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::WaitingCounterparty),
            expiration: ActiveValue::Set(expiration),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;
        let mut asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            ..Default::default()
        };
        if let Some(at) = asset_type {
            let cid = asset_id.expect("asset ID");
            match at {
                AssetType::Rgb20 => asset_transfer.asset_rgb20_id = ActiveValue::Set(Some(cid)),
                AssetType::Rgb121 => asset_transfer.asset_rgb121_id = ActiveValue::Set(Some(cid)),
            }
        }
        let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            amount: ActiveValue::Set(s!("0")),
            blinded_utxo: ActiveValue::Set(Some(blinded_utxo.clone())),
            blinding_secret: ActiveValue::Set(Some(seal.blinding.to_string())),
            ..Default::default()
        };
        let transfer_idx = self.database.set_transfer(transfer)?;
        let db_coloring = DbColoringActMod {
            txo_idx: ActiveValue::Set(utxo.idx),
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            coloring_type: ActiveValue::Set(ColoringType::Blind),
            amount: ActiveValue::Set(s!("0")),
            ..Default::default()
        };
        self.database.set_coloring(db_coloring)?;

        for endpoint in endpoints {
            self._save_transfer_consignment_endpoint(
                transfer_idx,
                &LocalConsignmentEndpoint {
                    endpoint,
                    protocol: ConsignmentEndpointProtocol::RgbHttpJsonRpc,
                    used: false,
                    usable: true,
                },
            )?;
        }

        info!(self.logger, "Blind completed");
        Ok(BlindData {
            invoice: invoice.to_string(),
            blinded_utxo,
            blinding_secret: seal.blinding,
            expiration_timestamp: expiration,
        })
    }

    fn _create_split_tx(
        &self,
        inputs: &[OutPoint],
        num_utxos_to_create: u8,
        size: u32,
        fee_rate: f32,
    ) -> Result<PartiallySignedTransaction, bdk::Error> {
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

        let mut psbt =
            PartiallySignedTransaction::from_str(&unsigned_psbt).map_err(InternalError::from)?;
        self.bdk_wallet
            .sign(&mut psbt, SignOptions::default())
            .map_err(InternalError::from)?;

        self.create_utxos_end(online, psbt.to_string())
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
    /// Providing the optional `size` parameter requests that UTXOs be crated of that size, if it's
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
            .get_rgb_allocations(unspent_txos.clone(), None, None, None)?;

        let mut utxos_to_create = num.unwrap_or(UTXO_NUM);
        if up_to {
            let allocatable = self
                ._get_available_allocations(unspents.clone(), vec![], None)?
                .len() as u8;
            if allocatable >= utxos_to_create {
                return Err(Error::AllocationsAlreadyAvailable);
            } else {
                utxos_to_create -= allocatable
            }
        }
        debug!(self.logger, "Will try to create {} UTXOs", utxos_to_create);

        let inputs: Vec<OutPoint> = unspent_txos
            .into_iter()
            .filter(|u| !u.colorable)
            .map(OutPoint::from)
            .collect();
        let inputs: &[OutPoint] = &inputs;
        let new_btc_amount = self._get_uncolorable_btc_sum(&unspents);
        let utxo_size = size.unwrap_or(UTXO_SIZE);
        let max_possible_utxos = new_btc_amount / utxo_size as u64;
        let mut btc_needed: u64 = 0;
        let mut btc_available: u64 = 0;
        let mut num_try_creating = min(utxos_to_create, max_possible_utxos as u8);
        while num_try_creating > 0 {
            match self._create_split_tx(inputs, num_try_creating, utxo_size, fee_rate) {
                Ok(_v) => break,
                Err(e) => {
                    (btc_needed, btc_available) = match e {
                        bdk::Error::InsufficientFunds { needed, available } => (needed, available),
                        _ => (0, 0),
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

        let signed_psbt = PartiallySignedTransaction::from_str(&signed_psbt)?;
        let tx = self._broadcast_psbt(signed_psbt)?;

        let mut num_utxos_created = 0;
        let bdk_utxos: Vec<LocalUtxo> = self
            .bdk_wallet
            .list_unspent()
            .map_err(InternalError::from)?;
        for utxo in bdk_utxos.into_iter() {
            let db_txo = self
                .database
                .get_txo(Outpoint::from(utxo.outpoint))?
                .expect("outpoint should be in the DB");
            if utxo.outpoint.txid == tx.txid() && utxo.keychain == KeychainKind::External {
                let mut updated_txo: DbTxoActMod = db_txo.into();
                updated_txo.colorable = ActiveValue::Set(true);
                self.database.update_txo(updated_txo)?;
                num_utxos_created += 1
            }
        }

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
    /// An optional `blinded_utxo` can be provided to operate on a single transfer.
    /// An optional `txid` can be provided to operate on a batch transfer.
    /// If both a `blinded_utxo` and a `txid` are provided, they need to belong to the same batch
    /// transfer or an error is returned.
    ///
    /// If either `blinded_utxo` or `txid` are provided and `no_asset_only` is true, transfers with
    /// an associated Asset ID will not be deleted and instead return an error.
    ///
    /// Eligible transfers are the ones in status [`TransferStatus::Failed`].
    pub fn delete_transfers(
        &self,
        blinded_utxo: Option<String>,
        txid: Option<String>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        info!(
            self.logger,
            "Deleting transfer with blinded UTXO {:?} and TXID {:?}...", blinded_utxo, txid
        );

        let db_data = self.database.get_db_data(false)?;
        let mut transfers_changed = false;

        if blinded_utxo.is_some() || txid.is_some() {
            let (batch_transfer, asset_transfers) = if let Some(bu) = blinded_utxo {
                let db_transfer =
                    &mut self.database.get_transfer_or_fail(bu, &db_data.transfers)?;
                let (_, batch_transfer) = db_transfer
                    .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)?;
                let asset_transfers =
                    batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;
                let asset_transfer_ids: Vec<i64> = asset_transfers.iter().map(|t| t.idx).collect();
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
                let connected_assets = asset_transfers.iter().any(|t| t.asset_id().is_some());
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
                    let connected_assets = asset_transfers.iter().any(|t| t.asset_id().is_some());
                    if connected_assets {
                        continue;
                    }
                }
                transfers_changed = true;
                self._delete_batch_transfer(batch_transfer, &asset_transfers)?
            }
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

        let mut psbt =
            PartiallySignedTransaction::from_str(&unsigned_psbt).map_err(InternalError::from)?;
        self.bdk_wallet
            .sign(&mut psbt, SignOptions::default())
            .map_err(InternalError::from)?;

        self.drain_to_end(online, psbt.to_string())
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

        let address = Address::from_str(&address).map(|x| x.script_pubkey())?;

        let mut tx_builder = self.bdk_wallet.build_tx();
        tx_builder
            .drain_wallet()
            .drain_to(address)
            .fee_rate(FeeRate::from_sat_per_vb(fee_rate));

        if !destroy_assets {
            let colored_txos: Vec<i64> = self
                .database
                .iter_colorings()?
                .into_iter()
                .map(|c| c.txo_idx)
                .collect();
            let unspendable: Vec<OutPoint> = self
                .database
                .iter_txos()?
                .into_iter()
                .filter(|t| t.colorable || colored_txos.contains(&t.idx))
                .map(OutPoint::from)
                .collect();
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
    /// Returns the txid of the transaction that's been broadcast
    pub fn drain_to_end(&self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger, "Draining (end)...");
        self._check_online(online)?;

        let signed_psbt = PartiallySignedTransaction::from_str(&signed_psbt)?;
        let tx = self._broadcast_psbt(signed_psbt)?;

        info!(self.logger, "Drain (end) completed");
        Ok(tx.txid().to_string())
    }

    fn _fail_batch_transfer(&self, batch_transfer: &DbBatchTransfer) -> Result<(), Error> {
        let mut updated_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        updated_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        updated_transfer.expiration = ActiveValue::Set(Some(now().unix_timestamp()));
        self.database.update_batch_transfer(&mut updated_transfer)?;

        Ok(())
    }

    fn _try_fail_batch_transfer(
        &mut self,
        batch_transfer: &DbBatchTransfer,
        throw_err: bool,
        db_data: &mut DbData,
    ) -> Result<(), Error> {
        let updated_transfer = self._refresh_transfer(batch_transfer, db_data, &vec![])?;
        // fail transfer if the status didn't change after a refresh
        if updated_transfer.is_none() {
            self._fail_batch_transfer(batch_transfer)?;
        } else if throw_err {
            return Err(Error::CannotFailTransfer);
        }

        Ok(())
    }

    /// Set the status for eligible transfers to [`TransferStatus::Failed`] and return if any
    /// transfer has changed
    ///
    /// An optional `blinded_utxo` can be provided to operate on a single transfer.
    /// An optional `txid` can be provided to operate on a batch transfer.
    /// If both a `blinded_utxo` and a `txid` are provided, they need to belong to the same batch
    /// transfer or an error is returned.
    ///
    /// If either `blinded_utxo` or `txid` are provided and `no_asset_only` is true, transfers with
    /// an associated Asset ID will not be failed and instead return an error.
    ///
    /// Transfers are eligible if they remain in status [`TransferStatus::WaitingCounterparty`]
    /// after a `refresh` has been performed. If nor `blinded_utxo` not `txid` have been provided,
    /// only expired transfers will be failed.
    pub fn fail_transfers(
        &mut self,
        online: Online,
        blinded_utxo: Option<String>,
        txid: Option<String>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        info!(
            self.logger,
            "Failing transfer with blinded UTXO {:?} and TXID {:?}...", blinded_utxo, txid
        );
        self._check_online(online)?;

        let mut db_data = self.database.get_db_data(false)?;
        let mut transfers_changed = false;

        if blinded_utxo.is_some() || txid.is_some() {
            let batch_transfer = if let Some(bu) = blinded_utxo {
                let db_transfer =
                    &mut self.database.get_transfer_or_fail(bu, &db_data.transfers)?;
                let (_, batch_transfer) = db_transfer
                    .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)?;
                let asset_transfers =
                    batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;
                let asset_transfer_ids: Vec<i64> = asset_transfers.iter().map(|t| t.idx).collect();
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
                let connected_assets = asset_transfers.iter().any(|t| t.asset_id().is_some());
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
                        .any(|t| t.asset_id().is_some());
                    if connected_assets {
                        continue;
                    }
                }
                transfers_changed = true;
                self._try_fail_batch_transfer(batch_transfer, false, &mut db_data)?
            }
        }

        info!(self.logger, "Fail transfers completed");
        Ok(transfers_changed)
    }

    fn _get_new_address(&self) -> Address {
        self.bdk_wallet
            .get_address(AddressIndex::New)
            .expect("to be able to get a new address")
            .address
    }

    /// Return a new bitcoin address
    pub fn get_address(&self) -> String {
        info!(self.logger, "Getting address...");
        let address = self._get_new_address().to_string();
        info!(self.logger, "Get address completed");
        address
    }

    /// Return the [`Balance`] for the requested asset
    pub fn get_asset_balance(&self, asset_id: String) -> Result<Balance, Error> {
        info!(self.logger, "Getting balance for asset '{}'...", asset_id);
        self.database.get_asset_or_fail(asset_id.clone())?;
        let balance = self
            .database
            .get_asset_balance(asset_id, None, None, None, None);
        info!(self.logger, "Get asset balance completed");
        balance
    }

    /// Return the [`Metadata`] for the requested asset
    pub fn get_asset_metadata(
        &mut self,
        online: Online,
        asset_id: String,
    ) -> Result<Metadata, Error> {
        info!(self.logger, "Getting metadata for asset '{}'...", asset_id);
        self._check_online(online)?;
        self.database.get_asset_or_fail(asset_id.clone())?;

        let contract_id = ContractId::from_str(&asset_id).map_err(InternalError::from)?;
        let contract_state = self
            ._rgb_client()?
            .contract_state(contract_id)
            .map_err(InternalError::from)?;
        let node_id = NodeId::from_inner(contract_id.into_inner());
        let metadata = match contract_state.metadata.get(&node_id) {
            Some(m) => Ok(RgbMetadata::from_inner(m.clone())),
            None => Err(InternalError::Unexpected),
        }?;

        let schema_id = contract_state.schema_id.to_string();
        let asset_metadata = match &schema_id[..] {
            rgb20::schema::SCHEMA_ID_BECH32 => {
                let issued_supply = *metadata
                    .u64(Rgb20FieldType::IssuedSupply)
                    .first()
                    .expect("valid consignment should contain the issued supply");
                let timestamp = *metadata
                    .i64(Rgb20FieldType::Timestamp)
                    .first()
                    .expect("valid consignment should contain the genesis timestamp");
                let name = metadata
                    .ascii_string(Rgb20FieldType::Name)
                    .first()
                    .expect("valid consignment should contain the asset name")
                    .to_string();
                let precision = *metadata
                    .u8(Rgb20FieldType::Precision)
                    .first()
                    .expect("valid consignment should contain the asset precision");
                let ticker = metadata
                    .ascii_string(Rgb20FieldType::Ticker)
                    .first()
                    .expect("valid consignment should contain the asset ticker")
                    .to_string();
                Metadata {
                    asset_type: AssetType::Rgb20,
                    issued_supply,
                    timestamp,
                    name,
                    precision,
                    ticker: Some(ticker),
                    description: None,
                    parent_id: None,
                }
            }
            RGB121_SCHEMA_ID => {
                let issued_supply = *metadata
                    .u64(Rgb121FieldType::IssuedSupply)
                    .first()
                    .expect("valid consignment should contain the issued supply");
                let timestamp = *metadata
                    .i64(Rgb121FieldType::Timestamp)
                    .first()
                    .expect("valid consignment should contain the genesis timestamp");
                let name = metadata
                    .ascii_string(Rgb121FieldType::Name)
                    .first()
                    .expect("valid consignment should contain the asset name")
                    .to_string();
                let precision = *metadata
                    .u8(Rgb121FieldType::Precision)
                    .first()
                    .expect("valid consignment should contain the asset precision");
                let description = metadata.ascii_string(Rgb121FieldType::Description);
                let description = description.first().map(|desc| desc.to_string());
                let parent_id = metadata.ascii_string(Rgb121FieldType::ParentId);
                let parent_id = parent_id.first().map(|pid| pid.to_string());
                Metadata {
                    asset_type: AssetType::Rgb121,
                    issued_supply,
                    timestamp,
                    name,
                    precision,
                    ticker: None,
                    description,
                    parent_id,
                }
            }
            _ => return Err(Error::UnknownRgbSchema { schema_id }),
        };

        info!(self.logger, "Get asset metadata completed");
        Ok(asset_metadata)
    }

    /// Return the wallet data provided by the user
    pub fn get_wallet_data(&self) -> WalletData {
        self.wallet_data.clone()
    }

    /// Return the wallet data directory
    pub fn get_wallet_dir(&self) -> PathBuf {
        self.wallet_dir.clone()
    }

    fn _check_consistency(&mut self) -> Result<(), Error> {
        info!(self.logger, "Doing a consistency check...");

        self._sync_db_txos()?;
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

        let asset_ids: Vec<String> = self
            ._rgb_client()?
            .list_contracts()
            .map_err(InternalError::from)?
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

    fn _go_online(
        &mut self,
        skip_consistency_check: bool,
        electrum_url: String,
    ) -> Result<Online, Error> {
        let online_id = now().unix_timestamp_nanos() as u64;
        let online = Online {
            id: online_id,
            electrum_url: electrum_url.clone(),
        };
        self.online = Some(online.clone());

        // create electrum client
        let electrum_config = ConfigBuilder::new()
            .timeout(Some(ELECTRUM_TIMEOUT))
            .expect("cannot fail since socks5 is unset")
            .build();
        self.electrum_client = Some(
            ElectrumClient::from_config(&electrum_url, electrum_config).map_err(|e| {
                Error::InvalidElectrum {
                    details: e.to_string(),
                }
            })?,
        );
        // check electrum server
        if self.bitcoin_network != BitcoinNetwork::Regtest {
            self._get_tx_details(get_txid(self.bitcoin_network))
                .map_err(|e| Error::InvalidElectrum {
                    details: e.to_string(),
                })?;
        }

        // BDK setup
        let config = ElectrumBlockchainConfig {
            url: electrum_url.clone(),
            socks5: None,
            retry: 3,
            timeout: Some(5),
            stop_gap: 20,
        };
        self.bdk_blockchain =
            Some(
                ElectrumBlockchain::from_config(&config).map_err(|e| Error::InvalidElectrum {
                    details: e.to_string(),
                })?,
            );

        // RGB setup
        let rgb_network = RgbNetwork::from(self.bitcoin_network);
        let rpc_endpoint = ServiceAddr::Inproc(format!("rpc-endpoint-{}", online_id));
        let ctl_endpoint = ServiceAddr::Inproc(format!("ctl-endpoint-{}", online_id));
        let storm_endpoint = ServiceAddr::Inproc(format!("storm-endpoint-{}", online_id));
        let store_endpoint = ServiceAddr::Inproc(format!("store-endpoint-{}", online_id));
        let mut config = StoreConfig {
            data_dir: self.wallet_dir.clone(),
            rpc_endpoint: store_endpoint.clone(),
            verbose: 7,
            databases: vec![].into_iter().collect(),
        };
        config.process();
        thread::spawn(move || {
            stored::service::run(config).expect("running stored runtime");
        });
        let config = Config {
            rpc_endpoint: rpc_endpoint.clone(),
            ctl_endpoint,
            storm_endpoint,
            store_endpoint,
            data_dir: self.wallet_dir.clone(),
            electrum_url,
            chain: rgb_network.clone(),
            threaded: true,
        };
        thread::spawn(move || {
            rgbd::run(config).expect("running rgbd runtime");
        });
        self.rgb_client = Some(
            Client::with(rpc_endpoint, "rgb-ffi".to_string(), rgb_network)
                .expect("Error initializing client"),
        );
        let mut tries_left: usize = 20;
        while let Err(_assets) = self._rgb_client()?.list_contracts() {
            if tries_left < 1 {
                return Err(InternalError::CannotQueryRgbNode)?;
            }
            debug!(
                self.logger,
                "Trying to contact rgbd, tries left {}", tries_left
            );
            tries_left -= 1;
            std::thread::sleep(Duration::from_millis(500));
        }

        if !skip_consistency_check {
            self._check_consistency()?;
        }

        Ok(online)
    }

    /// Return the existing or freshly generated set of wallet [`Online`] data
    ///
    /// Setting `skip_consistency_check` to true bypases the check and allows operating an
    /// inconsistent wallet. Warning: this is dangerous, only do this if you know what you're doing!
    pub fn go_online(
        &mut self,
        skip_consistency_check: bool,
        electrum_url: String,
    ) -> Result<Online, Error> {
        info!(self.logger, "Going online...");
        let online = if let Some(online) = self.online.clone() {
            if electrum_url == online.electrum_url {
                Ok(online)
            } else {
                Err(Error::CannotChangeOnline)
            }
        } else {
            let online = self._go_online(skip_consistency_check, electrum_url);
            if online.is_err() {
                self.online = None;
                self.bdk_blockchain = None;
                self.electrum_client = None;
                self.rgb_client = None;
            }
            online
        };
        info!(self.logger, "Go online completed");
        online
    }

    fn _check_name(&self, name: &str) -> Result<AsciiString, Error> {
        let name_ascii = AsciiString::from_str(name).map_err(|e| Error::InvalidName {
            details: e.to_string(),
        })?;
        if name_ascii.is_empty() {
            return Err(Error::InvalidName {
                details: s!("name cannot be empty"),
            });
        }
        if name_ascii.len() > MAX_LEN_NAME {
            return Err(Error::InvalidName {
                details: s!("name too long"),
            });
        }
        Ok(name_ascii)
    }

    fn _check_precision(&self, precision: u8) -> Result<u8, Error> {
        if precision > MAX_PRECISION {
            return Err(Error::InvalidPrecision {
                details: s!("precision is too high"),
            });
        }
        Ok(precision)
    }

    fn _check_ticker(&self, ticker: &str) -> Result<AsciiString, Error> {
        let ticker_ascii = AsciiString::from_str(ticker).map_err(|e| Error::InvalidTicker {
            details: e.to_string(),
        })?;
        if ticker_ascii.is_empty() {
            return Err(Error::InvalidTicker {
                details: s!("ticker cannot be empty"),
            });
        }
        if ticker_ascii.len() > MAX_LEN_TICKER {
            return Err(Error::InvalidTicker {
                details: s!("ticker too long"),
            });
        }
        if ticker_ascii.to_ascii_uppercase() != ticker_ascii.to_string() {
            return Err(Error::InvalidTicker {
                details: s!("ticker needs to be all uppercase"),
            });
        }
        Ok(ticker_ascii)
    }

    /// Issue a new RGB [`AssetRgb20`] and return it
    pub fn issue_asset_rgb20(
        &mut self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetRgb20, Error> {
        if amounts.is_empty() {
            return Err(Error::NoIssuanceAmounts);
        }
        info!(
            self.logger,
            "Issuing RGB20 asset with ticker '{}' name '{}' precision '{}' amounts '{:?}'...",
            ticker,
            name,
            precision,
            amounts
        );
        self._check_online(online)?;

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
        let mut issue_utxos: HashMap<DbTxo, u64> = HashMap::new();
        let mut allocations = vec![];
        for amount in &amounts {
            let exclude_outpoints: Vec<Outpoint> =
                issue_utxos.keys().map(|txo| txo.outpoint()).collect();
            let utxo = self._get_utxo(exclude_outpoints, Some(unspents.clone()), false)?;
            let outpoint = utxo.outpoint().to_string();
            issue_utxos.insert(utxo, *amount);
            let allocation = RgbOutpointValue::from_str(&format!("{amount}@{outpoint}"))
                .expect("allocation structure should be correct");
            allocations.push(allocation)
        }
        debug!(self.logger, "Issuing asset allocations '{:?}'", allocations);
        let asset = Contract::create_rgb20(
            RgbNetwork::from(self.bitcoin_network),
            self._check_ticker(&ticker)?,
            self._check_name(&name)?,
            self._check_precision(precision)?,
            allocations,
            BTreeMap::new(),
            CloseMethod::OpretFirst,
            None,
            None,
        );
        let _rgb_asset =
            Rgb20Asset::try_from(&asset).expect("create_rgb20 does not match RGB20 schema");
        let force = true;
        debug!(self.logger, "Registering contract...");
        let status = self
            ._rgb_client()?
            .register_contract(asset.clone(), force, |_| ())
            .map_err(InternalError::from)?;
        debug!(self.logger, "Contract registered");
        if !matches!(status, ContractValidity::Valid) {
            return Err(Error::FailedIssuance {
                details: format!("{:?}", status),
            });
        }
        let asset_id = asset.contract_id().to_string();
        debug!(self.logger, "Issued asset with ID '{:?}'", asset_id);

        let db_asset = DbAssetRgb20 {
            idx: 0,
            asset_id: asset_id.clone(),
            ticker,
            name,
            precision,
        };
        self.database.set_asset_rgb20(db_asset.clone())?;
        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::Settled),
            expiration: ActiveValue::Set(None),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_rgb20_id: ActiveValue::Set(Some(asset_id.clone())),
            ..Default::default()
        };
        let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
        let settled: u64 = amounts.iter().sum();
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            amount: ActiveValue::Set(settled.to_string()),
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

        let asset = AssetRgb20::from_db_asset(
            db_asset,
            self.database
                .get_asset_balance(asset_id, None, None, None, None)?,
        );

        info!(self.logger, "Issue asset RGB20 completed");
        Ok(asset)
    }

    /// Issue a new RGB [`AssetRgb121`] and return it
    pub fn issue_asset_rgb121(
        &mut self,
        online: Online,
        name: String,
        description: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        parent_id: Option<String>,
        file_path: Option<String>,
    ) -> Result<AssetRgb121, Error> {
        if amounts.is_empty() {
            return Err(Error::NoIssuanceAmounts);
        }
        info!(
            self.logger,
            "Issuing RGB121 asset with name '{}' precision '{}' amounts '{:?}'...",
            name,
            precision,
            amounts
        );
        self._check_online(online)?;

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
        let mut issue_utxos: HashMap<DbTxo, u64> = HashMap::new();
        let mut allocations = vec![];
        for amount in &amounts {
            let exclude_outpoints: Vec<Outpoint> =
                issue_utxos.keys().map(|txo| txo.outpoint()).collect();
            let utxo = self._get_utxo(exclude_outpoints, Some(unspents.clone()), false)?;
            let outpoint = utxo.outpoint().to_string();
            issue_utxos.insert(utxo, *amount);
            let allocation = RgbOutpointValue::from_str(&format!("{amount}@{outpoint}"))
                .expect("allocation structure should be correct");
            allocations.push(allocation)
        }
        debug!(self.logger, "Issuing asset allocations '{:?}'", allocations);
        let desc = if let Some(desc) = description.clone() {
            Some(
                AsciiString::from_str(&desc).map_err(|e| Error::InvalidDescription {
                    details: e.to_string(),
                })?,
            )
        } else {
            None
        };
        let pid = if let Some(pid) = &parent_id {
            if pid.is_empty() {
                return Err(Error::InvalidParentId {
                    details: s!("parent_id cannot be empty"),
                });
            }
            ContractId::from_str(pid).map_err(|e| Error::InvalidParentId {
                details: e.to_string(),
            })?;
            Some(
                AsciiString::from_str(pid).map_err(|e| Error::InvalidParentId {
                    details: e.to_string(),
                })?,
            )
        } else {
            None
        };
        let attachments = if let Some(fp) = &file_path {
            let fpath = std::path::Path::new(fp);
            if !fpath.exists() {
                return Err(Error::InvalidFilePath {
                    file_path: fp.clone(),
                });
            }
            vec![FileAttachment {
                file_path: PathBuf::from(fp),
                mime: AsciiString::from_str(&tree_magic::from_filepath(fpath)).expect("valid mime"),
                salt: 1,
            }]
        } else {
            vec![]
        };

        let asset = Contract::create_rgb121(
            RgbNetwork::from(self.bitcoin_network),
            self._check_name(&name)?,
            desc,
            self._check_precision(precision)?,
            pid,
            attachments,
            vec![],
            allocations,
            CloseMethod::OpretFirst,
        )
        .map_err(InternalError::from)?;
        let _rgb_asset =
            Rgb121Asset::try_from(&asset).expect("create_rgb121 does not match RGB121 schema");
        let force = true;
        debug!(self.logger, "Registering contract...");
        let status = self
            ._rgb_client()?
            .register_contract(asset.clone(), force, |_| ())
            .map_err(InternalError::from)?;
        debug!(self.logger, "Contract registered");
        if !matches!(status, ContractValidity::Valid) {
            return Err(Error::FailedIssuance {
                details: format!("{:?}", status),
            });
        }
        let asset_id = asset.contract_id().to_string();
        debug!(self.logger, "Issued asset with ID '{:?}'", asset_id);

        if let Some(fp) = file_path {
            let file_bytes = std::fs::read(fp.clone())?;
            let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
            let attachment_id = AttachmentId::commit(&file_hash).to_string();
            let media_dir = self
                .wallet_dir
                .join(ASSETS_DIR)
                .join(asset_id.clone())
                .join(attachment_id);
            fs::create_dir_all(&media_dir)?;
            let media_path = media_dir.join(MEDIA_FNAME);
            fs::copy(fp, &media_path)?;
            let mime = AsciiString::from_str(&tree_magic::from_filepath(media_path.as_path()))
                .expect("valid mime");
            fs::write(media_dir.join(MIME_FNAME), mime.to_string())?;
        }

        let db_asset = DbAssetRgb121 {
            idx: 0,
            asset_id: asset_id.clone(),
            name,
            precision,
            description,
            parent_id,
        };
        self.database.set_asset_rgb121(db_asset.clone())?;
        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::Settled),
            expiration: ActiveValue::Set(None),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_rgb121_id: ActiveValue::Set(Some(asset_id.clone())),
            ..Default::default()
        };
        let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
        let settled: u64 = amounts.iter().sum();
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            amount: ActiveValue::Set(settled.to_string()),
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

        let asset = AssetRgb121::from_db_asset(
            db_asset,
            self.database
                .get_asset_balance(asset_id, None, None, None, None)?,
            self.wallet_dir.join(ASSETS_DIR),
        )?;

        info!(self.logger, "Issue asset RGB121 completed");
        Ok(asset)
    }

    /// List the assets known by the underlying RGB node
    pub fn list_assets(&self, mut filter_asset_types: Vec<AssetType>) -> Result<Assets, Error> {
        info!(self.logger, "Listing assets...");
        if filter_asset_types.is_empty() {
            filter_asset_types = vec![AssetType::Rgb20, AssetType::Rgb121];
        }

        let batch_transfers = Some(self.database.iter_batch_transfers()?);
        let colorings = Some(self.database.iter_colorings()?);
        let txos = Some(self.database.iter_txos()?);
        let asset_transfers = Some(self.database.iter_asset_transfers()?);

        let mut rgb20 = None;
        let mut rgb121 = None;
        for asset_type in filter_asset_types {
            match asset_type {
                AssetType::Rgb20 => {
                    rgb20 = Some(
                        self.database
                            .iter_assets_rgb20()?
                            .iter()
                            .map(|c| {
                                Ok(AssetRgb20::from_db_asset(
                                    c.clone(),
                                    self.database.get_asset_balance(
                                        c.asset_id.clone(),
                                        asset_transfers.clone(),
                                        batch_transfers.clone(),
                                        colorings.clone(),
                                        txos.clone(),
                                    )?,
                                ))
                            })
                            .collect::<Result<Vec<AssetRgb20>, Error>>()?,
                    );
                }
                AssetType::Rgb121 => {
                    let assets_dir = self.wallet_dir.join(ASSETS_DIR);
                    rgb121 = Some(
                        self.database
                            .iter_assets_rgb121()?
                            .iter()
                            .map(|c| {
                                AssetRgb121::from_db_asset(
                                    c.clone(),
                                    self.database.get_asset_balance(
                                        c.asset_id.clone(),
                                        asset_transfers.clone(),
                                        batch_transfers.clone(),
                                        colorings.clone(),
                                        txos.clone(),
                                    )?,
                                    assets_dir.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetRgb121>, Error>>()?,
                    );
                }
            }
        }

        info!(self.logger, "List assets completed");
        Ok(Assets { rgb20, rgb121 })
    }

    /// List the [`Transfer`]s known to the RGB wallet
    pub fn list_transfers(&self, asset_id: String) -> Result<Vec<Transfer>, Error> {
        info!(self.logger, "Listing transfers for asset '{}'...", asset_id);
        self.database.get_asset_or_fail(asset_id.clone())?;
        let db_data = self.database.get_db_data(false)?;
        let asset_transfer_ids: Vec<i64> = self
            .database
            .iter_asset_asset_transfers(asset_id, db_data.asset_transfers.clone())
            .iter()
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
                let tce_data = self
                    .database
                    .get_transfer_consignment_endpoints_data(t.idx)?;
                Ok(Transfer::from_db_transfer(
                    t,
                    self.database.get_transfer_data(
                        &asset_transfer,
                        &batch_transfer,
                        &db_data.txos,
                        &db_data.colorings,
                    )?,
                    tce_data
                        .iter()
                        .map(|(tce, ce)| {
                            TransferConsignmentEndpoint::from_db_transfer_consignment_endpoint(
                                tce, ce,
                            )
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
    pub fn list_unspents(&self, settled_only: bool) -> Result<Vec<Unspent>, Error> {
        info!(self.logger, "Listing unspents...");

        let db_data = self.database.get_db_data(true)?;

        let mut allocation_txos = self.database.get_unspent_txos(db_data.txos.clone())?;
        let spent_txos_ids: Vec<i64> = db_data
            .txos
            .clone()
            .into_iter()
            .filter(|t| t.spent)
            .map(|u| u.idx)
            .collect();
        let waiting_confs_batch_transfer_ids: Vec<i64> = db_data
            .batch_transfers
            .clone()
            .into_iter()
            .filter(|t| t.waiting_confirmations())
            .map(|t| t.idx)
            .collect();
        let waiting_confs_transfer_ids: Vec<i64> = db_data
            .asset_transfers
            .clone()
            .into_iter()
            .filter(|t| waiting_confs_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let almost_spent_txos_ids: Vec<i64> = db_data
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

        info!(self.logger, "List unspents completed");
        Ok(unspents)
    }

    fn _get_signed_psbt(&self, transfer_dir: PathBuf) -> Result<PartiallySignedTransaction, Error> {
        let psbt_file = transfer_dir.join(SIGNED_PSBT_FILE);
        let psbt_str = fs::read_to_string(psbt_file)?;
        Ok(PartiallySignedTransaction::from_str(&psbt_str)?)
    }

    fn _fail_batch_transfer_if_no_endpoints(
        &self,
        batch_transfer: &DbBatchTransfer,
        transfer_consignment_endpoints_data: &Vec<(
            DbTransferConsignmentEndpoint,
            DbConsignmentEndpoint,
        )>,
    ) -> Result<bool, Error> {
        if transfer_consignment_endpoints_data.is_empty() {
            self._fail_batch_transfer(batch_transfer)?;
            return Ok(true);
        }

        Ok(false)
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
        let blinded_utxo = transfer
            .blinded_utxo
            .clone()
            .expect("transfer should have a blinded UTXO");

        // check if a consignment has been posted
        let tce_data = self
            .database
            .get_transfer_consignment_endpoints_data(transfer.idx)?;
        if self._fail_batch_transfer_if_no_endpoints(batch_transfer, &tce_data)? {
            return Ok(None);
        }
        let (mut proxy_url, mut consignment) = (None, None);
        let mut used_endpoint = None;
        for (transfer_consignment_endpoint, consignment_endpoint) in tce_data {
            let consignment_res = self
                .rest_client
                .clone()
                .get_consignment(&consignment_endpoint.endpoint, blinded_utxo.clone());
            if consignment_res.is_err() {
                debug!(
                    self.logger,
                    "Consignment GET response error: {:?}", &consignment_res
                );
                info!(
                    self.logger,
                    "Skipping consignment endpoint: {:?}", &consignment_endpoint
                );
                continue;
            }
            let consignment_res = consignment_res.unwrap();
            debug!(
                self.logger,
                "Consignment GET response: {:?}", consignment_res
            );

            if consignment_res.result.is_some() {
                proxy_url = Some(consignment_endpoint.endpoint);
                consignment = consignment_res.result;
                let mut updated_transfer_consignment_endpoint: DbTransferConsignmentEndpointActMod =
                    transfer_consignment_endpoint.into();
                updated_transfer_consignment_endpoint.used = ActiveValue::Set(true);
                used_endpoint = Some(updated_transfer_consignment_endpoint);
                break;
            }
        }

        let (consignment, proxy_url) = if let Some(cons) = consignment {
            (cons, proxy_url.expect("should be defined"))
        } else {
            return Ok(None);
        };

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();

        // write consignment
        let transfer_dir = self
            .wallet_dir
            .join(TRANSFER_DIR)
            .join(blinded_utxo.clone());
        let consignment_path = transfer_dir.join(CONSIGNMENT_RCV_FILE);
        fs::create_dir_all(transfer_dir)?;
        let consignment_bytes = base64::decode(consignment).map_err(InternalError::from)?;
        fs::write(consignment_path.clone(), consignment_bytes).expect("Unable to write file");
        let consignment =
            StateTransfer::strict_file_load(&consignment_path).map_err(InternalError::from)?;
        let cid = consignment.contract_id().to_string();
        let mut valid = true;

        // validate consignment
        let asset_id = asset_transfer.asset_id();
        if let Some(aid) = asset_id.clone() {
            // check if blinded is connected to the correct asset
            if aid != cid {
                valid = false
            }
        }

        debug!(self.logger, "Validating consignment...");
        let validation_status = Validator::validate(&consignment, self._electrum_client()?);
        let validity = validation_status.validity();
        debug!(self.logger, "Consignment validity: {:?}", validity);
        if valid && !vec![Validity::Valid, Validity::ValidExceptEndpoints].contains(&validity) {
            debug!(self.logger, "Consignment is invalid");
            valid = false;
        } else if valid {
            let genesis_media_file = consignment
                .genesis()
                .owned_rights_by_type(Rgb121OwnedRightType::Engraving as u16);

            if let Some(TypedAssignments::Attachment(assigments)) = genesis_media_file {
                for ass in assigments {
                    if let Assignment::Revealed { seal: _, state } = ass {
                        let attachment_id = state.id;
                        let media_res = self
                            .rest_client
                            .clone()
                            .get_media(&proxy_url, attachment_id.to_string().clone())?;
                        debug!(self.logger, "Media GET response: {:?}", media_res);
                        if let Some(media) = media_res.result {
                            let file_bytes = base64::decode(media).map_err(InternalError::from)?;
                            let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
                            let real_attachment_id = AttachmentId::commit(&file_hash);
                            if attachment_id != real_attachment_id {
                                valid = false;
                                break;
                            }
                            let media_dir = self
                                .wallet_dir
                                .join(ASSETS_DIR)
                                .join(cid.clone())
                                .join(attachment_id.to_string());
                            fs::create_dir_all(&media_dir)?;
                            fs::write(media_dir.join(MEDIA_FNAME), file_bytes)?;
                            fs::write(media_dir.join(MIME_FNAME), state.mime.to_string())?;
                        } else {
                            valid = false;
                            break;
                        }
                    }
                }
            }
        }

        if !valid {
            let nack_res = self
                .rest_client
                .clone()
                .post_ack(&proxy_url, blinded_utxo, false)?;
            debug!(self.logger, "Consignment NACK response: {:?}", nack_res);
            updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
            return Ok(Some(
                self.database
                    .update_batch_transfer(&mut updated_batch_transfer)?,
            ));
        }

        debug!(self.logger, "Consignment is valid");
        let anchored_bundles = consignment.anchored_bundles();
        let (anchor, transition_bundle) = anchored_bundles
            .last()
            .expect("there should be at least an anchored bundle");
        let txid = anchor.txid;
        let ack_res = self
            .rest_client
            .clone()
            .post_ack(&proxy_url, blinded_utxo, true)?;
        debug!(self.logger, "Consignment ACK response: {:?}", ack_res);

        // add asset info to transfer if missing
        if asset_id.is_none() {
            // check if asset is known
            let asset_type = self.database.get_asset_or_fail(cid.clone());
            let asset_type: AssetType = if asset_type.is_err() {
                // unknown asset, registering contract
                let ser_cons = strict_serialize(&consignment).map_err(InternalError::from)?;
                let contract_consignment: Contract =
                    strict_deserialize(ser_cons).map_err(InternalError::from)?;
                debug!(self.logger, "Registering contract...");
                self._rgb_client()?
                    .register_contract(contract_consignment, true, |_| ())
                    .map_err(InternalError::from)?;
                debug!(self.logger, "Contract registered");

                // extract asset data from consignment
                let metadata = consignment.genesis().metadata();
                let schema_id = consignment.schema_id().to_string();
                match &schema_id[..] {
                    rgb20::schema::SCHEMA_ID_BECH32 => {
                        let name = metadata
                            .ascii_string(Rgb20FieldType::Name)
                            .first()
                            .expect("valid consignment should contain the asset name")
                            .to_string();
                        let precision = *metadata
                            .u8(Rgb20FieldType::Precision)
                            .first()
                            .expect("valid consignment should contain the asset precision");
                        let ticker = metadata
                            .ascii_string(Rgb20FieldType::Ticker)
                            .first()
                            .expect("valid consignment should contain the asset ticker")
                            .to_string();
                        let db_asset = DbAssetRgb20 {
                            idx: 0,
                            asset_id: cid.clone(),
                            ticker,
                            name,
                            precision,
                        };
                        self.database.set_asset_rgb20(db_asset)?;
                        AssetType::Rgb20
                    }
                    RGB121_SCHEMA_ID => {
                        let name = metadata
                            .ascii_string(Rgb121FieldType::Name)
                            .first()
                            .expect("valid consignment should contain the asset name")
                            .to_string();
                        let precision = *metadata
                            .u8(Rgb121FieldType::Precision)
                            .first()
                            .expect("valid consignment should contain the asset precision");
                        let description = metadata.ascii_string(Rgb121FieldType::Description);
                        let description = description.first().map(|desc| desc.to_string());
                        let parent_id = metadata.ascii_string(Rgb121FieldType::ParentId);
                        let parent_id = parent_id.first().map(|pid| pid.to_string());
                        let db_asset = DbAssetRgb121 {
                            idx: 0,
                            asset_id: cid.clone(),
                            name,
                            precision,
                            description,
                            parent_id,
                        };
                        self.database.set_asset_rgb121(db_asset)?;
                        AssetType::Rgb121
                    }
                    _ => return Err(Error::UnknownRgbSchema { schema_id }),
                }
            } else {
                asset_type?
            };
            let mut updated_asset_transfer: DbAssetTransferActMod = asset_transfer.clone().into();
            let db_asset_id = ActiveValue::Set(Some(cid.clone()));
            match asset_type {
                AssetType::Rgb20 => updated_asset_transfer.asset_rgb20_id = db_asset_id,
                AssetType::Rgb121 => updated_asset_transfer.asset_rgb121_id = db_asset_id,
            }
            self.database
                .update_asset_transfer(&mut updated_asset_transfer)?;
        }

        // get and update transfer amount
        let mut amount = 0;
        let known_transitions = transition_bundle.known_transitions();
        let transfer_data = self.database.get_transfer_data(
            &asset_transfer,
            batch_transfer,
            &db_data.txos,
            &db_data.colorings,
        )?;
        let detailed_transfer = Transfer::from_db_transfer(transfer.clone(), transfer_data, vec![]);
        let blinding = detailed_transfer
            .blinding_secret
            .expect("incoming transfer should have a blinding secret");
        let unblinded_utxo = detailed_transfer
            .unblinded_utxo
            .ok_or(InternalError::Unexpected)?;
        let known_concealed = seal::Revealed {
            method: CloseMethod::OpretFirst,
            blinding,
            txid: Some(Txid::from_str(&unblinded_utxo.txid).expect("should be a valid TXID")),
            vout: unblinded_utxo.vout,
        }
        .to_concealed_seal();
        for transition in known_transitions {
            let owned_rights = transition.owned_rights();
            for (_owned_right_type, typed_assignment) in owned_rights.iter() {
                for assignment in typed_assignment.to_value_assignments() {
                    if let Assignment::ConfidentialSeal { seal, state } = assignment {
                        if seal == known_concealed {
                            amount += state.value;
                        }
                    };
                }
            }
        }
        debug!(self.logger, "Received '{}' of contract '{}'", amount, cid);
        let transfer_colorings = db_data
            .colorings
            .clone()
            .into_iter()
            .filter(|c| {
                c.asset_transfer_idx == asset_transfer.idx && c.coloring_type == ColoringType::Blind
            })
            .collect::<Vec<DbColoring>>()
            .first()
            .cloned();
        let transfer_coloring =
            transfer_colorings.expect("transfer should be connected to at least one coloring");
        let mut updated_coloring: DbColoringActMod = transfer_coloring.into();
        updated_coloring.amount = ActiveValue::Set(amount.to_string());
        self.database.update_coloring(updated_coloring)?;

        self.database
            .update_transfer_consignment_endpoint(&mut used_endpoint.expect("should be defined"))?;

        let mut updated_transfer: DbTransferActMod = transfer.into();
        updated_transfer.amount = ActiveValue::Set(amount.to_string());
        self.database.update_transfer(&mut updated_transfer)?;

        updated_batch_transfer.txid = ActiveValue::Set(Some(txid.to_string()));
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
                let tce_data = self
                    .database
                    .get_transfer_consignment_endpoints_data(transfer.idx)?;
                if self._fail_batch_transfer_if_no_endpoints(batch_transfer, &tce_data)? {
                    return Ok(None);
                }
                let (_, consignment_endpoint) = tce_data
                    .clone()
                    .into_iter()
                    .find(|(tce, _ce)| tce.used)
                    .expect("there should be 1 used tce");
                let proxy_url = consignment_endpoint.endpoint.clone();
                let ack_res = self.rest_client.clone().get_ack(
                    &proxy_url,
                    transfer
                        .blinded_utxo
                        .clone()
                        .expect("transfer should have a blinded UTXO"),
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

        let mut update_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        let mut batch_transfer_transfers: Vec<DbTransfer> = vec![];
        batch_transfer_data
            .asset_transfers_data
            .iter()
            .for_each(|atd| batch_transfer_transfers.extend(atd.transfers.clone()));
        if batch_transfer_transfers
            .iter()
            .any(|t| t.ack == Some(false))
        {
            update_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        } else if batch_transfer_transfers.iter().all(|t| t.ack == Some(true)) {
            let transfer_dir = self.wallet_dir.join(TRANSFER_DIR).join(
                batch_transfer
                    .txid
                    .as_ref()
                    .expect("batch transfer should have a txid"),
            );
            let signed_psbt = self._get_signed_psbt(transfer_dir)?;
            self._broadcast_psbt(signed_psbt)?;
            update_batch_transfer.status = ActiveValue::Set(TransferStatus::WaitingConfirmations);
        } else {
            return Ok(None);
        }

        Ok(Some(
            self.database
                .update_batch_transfer(&mut update_batch_transfer)?,
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
            .expect("batch transfer should have a txid");
        debug!(
            self.logger,
            "Getting details of transaction with ID '{}'...", txid
        );
        let tx_details = match self._get_tx_details(txid.clone()) {
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

        if tx_details.get("confirmations").is_none()
            || tx_details["confirmations"]
                .as_u64()
                .expect("confirmations to be a valid u64 number")
                < MIN_CONFIRMATIONS as u64
        {
            return Ok(None);
        }

        let batch_transfer_data =
            batch_transfer.get_transfers(&db_data.asset_transfers, &db_data.transfers)?;

        let asset_transfers: Vec<DbAssetTransfer> = batch_transfer_data
            .asset_transfers_data
            .iter()
            .map(|atd| atd.asset_transfer.clone())
            .collect();

        let (transfer, detailed_transfer) = if incoming {
            let (asset_transfer, transfer) =
                self.database.get_incoming_transfer(&batch_transfer_data)?;
            let transfer_data = self.database.get_transfer_data(
                &asset_transfer,
                batch_transfer,
                &db_data.txos,
                &db_data.colorings,
            )?;
            let detailed_transfer =
                Transfer::from_db_transfer(transfer.clone(), transfer_data, vec![]);
            (Some(transfer), Some(detailed_transfer))
        } else {
            (None, None)
        };

        let transfer_dir = if let Some(t) = transfer {
            self.wallet_dir
                .join(TRANSFER_DIR)
                .join(t.blinded_utxo.expect("transfer should have a blinded UTXO"))
        } else {
            self.wallet_dir.join(TRANSFER_DIR).join(txid.clone())
        };

        if !incoming {
            // set change outpoints as colorable
            let tx = self._get_signed_psbt(transfer_dir.clone())?.extract_tx();
            let txid = tx.txid().to_string();
            for (vout, output) in tx.output.iter().enumerate() {
                if output.value == 0 {
                    continue;
                }
                let mut db_txo: DbTxoActMod = self
                    .database
                    .get_txo(Outpoint {
                        txid: txid.clone(),
                        vout: vout as u32,
                    })?
                    .expect("DB should contain the txo")
                    .into();
                db_txo.colorable = ActiveValue::Set(true);
                self.database.update_txo(db_txo)?;
            }
        }

        // accept consignment(s)
        let consignment_paths = if incoming {
            vec![transfer_dir.join(CONSIGNMENT_RCV_FILE)]
        } else {
            asset_transfers
                .iter()
                .filter(|t| t.user_driven)
                .map(|t| {
                    let ass_id = t.asset_id().expect("corrupt DB or broken code");
                    transfer_dir.join(ass_id).join(CONSIGNMENT_FILE)
                })
                .collect()
        };
        for consignment_path in consignment_paths {
            let consignment =
                StateTransfer::strict_file_load(consignment_path).map_err(InternalError::from)?;
            let reveal = if let Some(dt) = detailed_transfer.clone() {
                let blinding_factor = dt
                    .blinding_secret
                    .expect("incoming transfer should have a blinding secret");
                let outpoint = OutPoint::from(
                    dt.unblinded_utxo
                        .expect("incoming transfer should have an unblinded UTXO"),
                );
                Some(Reveal {
                    blinding_factor,
                    outpoint,
                    close_method: CloseMethod::OpretFirst,
                })
            } else {
                None
            };
            debug!(self.logger, "Consuming RGB transfer...");
            let status = self
                ._rgb_client()?
                .consume_transfer(consignment, true, reveal, |_| ())
                .map_err(InternalError::from)?;
            debug!(self.logger, "Consumed RGB transfer");
            if !matches!(status, ContractValidity::Valid) {
                return Err(InternalError::Unexpected)?;
            }
        }

        if asset_transfers.into_iter().any(|t| !t.user_driven) {
            debug!(self.logger, "Processing disclosure...");
            self._rgb_client()?
                .process_disclosure(
                    Txid::from_str(&txid).expect("transaction should have a valid ID"),
                    |_| (),
                )
                .map_err(InternalError::from)?;
        }

        let mut updated_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        updated_transfer.status = ActiveValue::Set(TransferStatus::Settled);
        let updated = self.database.update_batch_transfer(&mut updated_transfer)?;

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
        if asset_id.is_some() {
            info!(self.logger, "Refreshing asset {:?}...", asset_id);
            self.database
                .get_asset_or_fail(asset_id.clone().expect("asset ID"))?;
        } else {
            info!(self.logger, "Refreshing assets...");
        }
        self._check_online(online)?;

        let mut db_data = self.database.get_db_data(false)?;

        if let Some(aid) = asset_id {
            let batch_transfers_ids: Vec<i64> = self
                .database
                .iter_asset_asset_transfers(aid, db_data.asset_transfers.clone())
                .into_iter()
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
        let txo_map: HashMap<i64, u64> = input_allocations
            .into_iter()
            .map(|(k, v)| (k.idx, v))
            .collect();
        let input_outpoints: Vec<OutPoint> = inputs.into_iter().map(OutPoint::from).collect();
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
        input_outpoints: Vec<OutPoint>,
        fee_rate: f32,
    ) -> Result<PartiallySignedTransaction, Error> {
        let mut builder = self.bdk_wallet.build_tx();
        builder
            .add_utxos(&input_outpoints)
            .map_err(InternalError::from)?
            .manually_selected_only()
            .fee_rate(FeeRate::from_sat_per_vb(fee_rate))
            .drain_to(self._get_new_address().script_pubkey());
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
        input_unspents: &Vec<LocalUnspent>,
        all_inputs: &mut Vec<OutPoint>,
        fee_rate: f32,
    ) -> Result<PartiallySignedTransaction, Error> {
        let psbt = loop {
            break match self._prepare_psbt(all_inputs.clone(), fee_rate) {
                Ok(psbt) => psbt,
                Err(Error::InsufficientBitcoins { .. }) => {
                    let used_txos: Vec<Outpoint> =
                        all_inputs.clone().into_iter().map(|o| o.into()).collect();
                    if let Some(a) = self
                        ._get_available_allocations(
                            input_unspents.clone(),
                            used_txos.clone(),
                            Some(0),
                        )?
                        .pop()
                    {
                        all_inputs.push(a.utxo.into());
                        continue;
                    } else {
                        return Err(self._detect_btc_unspendable_err(&input_unspents));
                    }
                }
                Err(e) => return Err(e),
            };
        };
        Ok(psbt)
    }

    fn _prepare_rgb_psbt(
        &mut self,
        final_psbt: &mut PartiallySignedTransaction,
        input_outpoints: Vec<OutPoint>,
        transfer_info_map: BTreeMap<String, InfoAssetTransfer>,
        transfer_dir: PathBuf,
        donation: bool,
        unspents: Vec<LocalUnspent>,
        db_data: &DbData,
    ) -> Result<(), Error> {
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

        let failed_batch_transfer_ids: Vec<i64> = db_data
            .batch_transfers
            .clone()
            .into_iter()
            .filter(|t| t.failed())
            .map(|t| t.idx)
            .collect();
        let failed_asset_transfer_ids: Vec<i64> = db_data
            .asset_transfers
            .clone()
            .into_iter()
            .filter(|t| failed_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let mut asset_beneficiaries: BTreeMap<String, BTreeMap<SealEndpoint, u64>> = bmap![];
        for (asset_id, transfer_info) in transfer_info_map.clone() {
            let asset_spend = transfer_info.asset_spend;
            let recipients = transfer_info.recipients;

            // RGB node compose
            let input_outpoints_bt: BTreeSet<OutPoint> =
                asset_spend.input_outpoints.clone().into_iter().collect();
            debug!(self.logger, "RGB contract ID...");
            let rgb_asset_id = ContractId::from_str(&asset_id).map_err(InternalError::from)?;
            debug!(self.logger, "RGB consign...");
            let transfer = self
                ._rgb_client()?
                .consign(rgb_asset_id, vec![], input_outpoints_bt.clone(), |_| ())
                .map_err(InternalError::from)?;
            debug!(self.logger, "RGB consign completed");
            let asset_transfer_dir = transfer_dir.join(asset_id.clone());
            if asset_transfer_dir.is_dir() {
                fs::remove_dir_all(asset_transfer_dir.clone())?;
            }
            fs::create_dir_all(asset_transfer_dir.clone())?;
            let consignment_path = asset_transfer_dir.join(CONSIGNMENT_FILE);
            let consignment_file = fs::File::create(consignment_path.clone())?;
            transfer
                .strict_encode(&consignment_file)
                .map_err(InternalError::from)?;

            // RGB node contract embed
            debug!(self.logger, "RGB contract...");
            let contract = self
                ._rgb_client()?
                .contract(rgb_asset_id, vec![], |_| {})
                .map_err(InternalError::from)?;
            debug!(self.logger, "RGB contract completed");
            let mut psbt = <Psbt as BitcoinDeserialize>::deserialize(&serialize(&final_psbt))
                .map_err(InternalError::from)?;
            psbt.set_rgb_contract(contract)
                .map_err(InternalError::from)?;

            // RGB20-RGB121 transfer
            let mut out_allocations: Vec<UtxobValue> = vec![];
            for recipient in recipients.clone() {
                if let Some(existing_transfer) = db_data
                    .transfers
                    .iter()
                    .filter(|t| !failed_asset_transfer_ids.contains(&t.asset_transfer_idx))
                    .find(|t| t.blinded_utxo == Some(recipient.blinded_utxo.clone()))
                {
                    if existing_transfer.blinding_secret.is_some() {
                        return Err(Error::CannotSendToSelf);
                    }
                    return Err(Error::BlindedUTXOAlreadyUsed)?;
                }
                out_allocations.push(UtxobValue {
                    value: recipient.amount,
                    seal_confidential: ConcealedSeal::from_str(&recipient.blinded_utxo)?,
                });
            }
            let beneficiaries: BTreeMap<SealEndpoint, u64> = out_allocations
                .into_iter()
                .map(|v| (v.seal_confidential.into(), v.value))
                .collect();
            asset_beneficiaries.insert(asset_id, beneficiaries.clone());
            let change: Vec<AllocatedValue> = if asset_spend.change_amount > 0 {
                vec![AllocatedValue {
                    value: asset_spend.change_amount,
                    seal: ExplicitSeal::from_str(&format!("opret1st:{}", change_utxo.outpoint(),))
                        .map_err(InternalError::from)?,
                }]
            } else {
                vec![]
            };
            let revealed_seal = change
                .into_iter()
                .map(|v| (v.into_revealed_seal(), v.value))
                .collect();
            let schema_id = transfer.schema_id().to_string();
            debug!(self.logger, "Creating RGB transition...");
            let transition = match &schema_id[..] {
                rgb20::schema::SCHEMA_ID_BECH32 => {
                    let rgb_asset = Rgb20Asset::try_from(&transfer)
                        .expect("to have provided a valid consignment");
                    rgb_asset
                        .transfer(input_outpoints_bt.clone(), beneficiaries, revealed_seal)
                        .map_err(|_| InternalError::Unexpected)?
                }
                RGB121_SCHEMA_ID => {
                    let rgb_asset = Rgb121Asset::try_from(&transfer)
                        .expect("to have provided a valid consignment");
                    rgb_asset
                        .transfer(input_outpoints_bt.clone(), beneficiaries, revealed_seal)
                        .map_err(|_| InternalError::Unexpected)?
                }
                _ => return Err(Error::UnknownRgbSchema { schema_id }),
            };
            debug!(self.logger, "Created RGB transition");

            // RGB node transfer combine
            let node_id = transition.node_id();
            psbt.push_rgb_transition(transition.clone())
                .map_err(InternalError::from)?;
            for input in &mut psbt.inputs {
                if input_outpoints_bt.contains(&input.previous_outpoint) {
                    input
                        .set_rgb_consumer(rgb_asset_id, node_id)
                        .map_err(InternalError::from)?;
                }
            }

            let psbt_serialized =
                &Vec::<u8>::from_hex(&psbt.to_string()).expect("provided psbt should be valid");
            let intermediate_psbt: PartiallySignedTransaction =
                deserialize(psbt_serialized).map_err(InternalError::from)?;
            *final_psbt = intermediate_psbt.clone();
        }

        let mut psbt = <Psbt as BitcoinDeserialize>::deserialize(&serialize(&final_psbt))
            .map_err(InternalError::from)?;

        // handle blank transitions
        let outpoints: BTreeSet<_> = psbt
            .inputs
            .iter()
            .map(|input| input.previous_outpoint)
            .collect();
        debug!(self.logger, "RGB outpoint state...");
        let state_map = self
            ._rgb_client()?
            .outpoint_state(outpoints, |_| ())
            .map_err(InternalError::from)?;
        debug!(self.logger, "RGB outpoint state completed");
        let new_outpoints: BTreeMap<u16, (OutPoint, CloseMethod)> = bmap! {
            STATE_TYPE_OWNERSHIP_RIGHT => (OutPoint::from(change_utxo.clone()), CloseMethod::OpretFirst)
        };
        let mut blank_allocations: HashMap<String, u64> = HashMap::new();
        for (cid, mut outpoint_map) in state_map {
            let cid_str = cid.to_string();
            debug!(self.logger, "Getting RGB contract for blank...");
            let contract = if transfer_info_map.contains_key(&cid_str) {
                let input_outpoints = transfer_info_map[&cid_str]
                    .clone()
                    .asset_spend
                    .input_outpoints;
                outpoint_map.retain(|&k, _| !input_outpoints.contains(&k));
                if outpoint_map.is_empty() {
                    continue;
                }
                self._rgb_client()?
                    .contract(cid, vec![], |_| ())
                    .map_err(InternalError::from)?
            } else {
                let contract = self
                    ._rgb_client()?
                    .contract(cid, vec![], |_| ())
                    .map_err(InternalError::from)?;
                psbt.set_rgb_contract(contract.clone())
                    .map_err(InternalError::from)?;
                contract
            };
            debug!(self.logger, "Getting RGB contract for blank completed");

            let schema_id = contract.schema_id().to_string();
            let blank_bundle = match &schema_id[..] {
                rgb20::schema::SCHEMA_ID_BECH32 => {
                    TransitionBundle::blank(&outpoint_map, &new_outpoints)
                        .map_err(InternalError::from)?
                }
                RGB121_SCHEMA_ID => {
                    for inputs in &mut outpoint_map.values_mut() {
                        inputs.retain(
                            |OutpointState {
                                 node_outpoint: input,
                                 state: _,
                             }| {
                                input.ty != Rgb121OwnedRightType::Engraving as u16
                            },
                        );
                    }
                    TransitionBundle::blank_rgb121(&outpoint_map, &new_outpoints)
                        .map_err(InternalError::from)?
                }
                _ => return Err(Error::UnknownRgbSchema { schema_id }),
            };

            for (transition, _indexes) in blank_bundle.revealed_iter() {
                let node_id = transition.node_id();
                psbt.push_rgb_transition(transition.clone())
                    .map_err(InternalError::from)?;
                for input in psbt.inputs.iter_mut() {
                    if outpoint_map.get(&input.previous_outpoint).is_some() {
                        input
                            .set_rgb_consumer(cid, node_id)
                            .map_err(InternalError::from)?;
                    }
                }
            }
            let mut moved_amount = 0;
            for transition in blank_bundle.known_transitions() {
                let owned_rights = transition.owned_rights();
                for (_owned_right_type, typed_assignment) in owned_rights.iter() {
                    for assignment in typed_assignment.to_value_assignments() {
                        if let Assignment::Revealed { seal: _, state } = assignment {
                            moved_amount += state.value;
                        };
                    }
                }
            }
            blank_allocations.insert(cid.to_string(), moved_amount);
        }

        // RGB std PSBT bundle
        debug!(self.logger, "RGB bundle to LNPBP4...");
        let _count = psbt.rgb_bundle_to_lnpbp4().map_err(InternalError::from)?;
        psbt.outputs
            .last_mut()
            .expect("PSBT should have outputs")
            .set_opret_host()
            .expect("given output should be valid");

        let mut transfers = vec![];
        for (asset_id, transfer_info) in transfer_info_map {
            // RGB node transfer finalize
            let beneficiaries = asset_beneficiaries[&asset_id].clone();
            let endseals = beneficiaries.into_iter().map(|b| b.0).collect();
            let asset_transfer_dir = transfer_dir.join(asset_id.clone());
            let consignment_path = asset_transfer_dir.join(CONSIGNMENT_FILE);
            let consignment =
                StateTransfer::strict_file_load(consignment_path).map_err(InternalError::from)?;
            transfers.push((consignment, endseals));

            // save asset transefer data to file (for send_end)
            let serialized_info =
                serde_json::to_string(&transfer_info).map_err(InternalError::from)?;
            let info_file = asset_transfer_dir.join(TRANSFER_DATA_FILE);
            fs::write(info_file, serialized_info)?;
        }

        debug!(self.logger, "Finalizing RGB transfers...");
        let transfer_consignment = self
            ._rgb_client()?
            .finalize_transfers(transfers, psbt.clone(), |_| ())
            .map_err(InternalError::from)?;

        for consignment in transfer_consignment.consignments {
            let asset_id = consignment.contract_id().to_string();
            let asset_transfer_dir = transfer_dir.join(asset_id.clone());
            let consignment_path = asset_transfer_dir.join(CONSIGNMENT_FILE);
            consignment
                .strict_file_save(consignment_path)
                .map_err(InternalError::from)?;
        }

        let psbt = transfer_consignment.psbt;
        let psbt_serialized =
            &Vec::<u8>::from_hex(&psbt.to_string()).expect("provided psbt should be valid");
        *final_psbt = deserialize(psbt_serialized).map_err(InternalError::from)?;

        // save batch transefer data to file (for send_end)
        let info_contents = InfoBatchTransfer {
            change_utxo_idx: change_utxo.idx,
            blank_allocations,
            donation,
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
    ) -> Result<(), Error> {
        let mut attachments = vec![];
        if let Some(ass_dir) = &asset_dir {
            for fp in fs::read_dir(ass_dir)? {
                let fpath = fp?.path();
                let file_path = fpath.join(MEDIA_FNAME);
                let file_bytes = std::fs::read(file_path.clone())?;
                let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
                let attachment_id = AttachmentId::commit(&file_hash).to_string();
                attachments.push((attachment_id, file_path))
            }
        }

        let consignment_path = asset_transfer_dir.join(CONSIGNMENT_FILE);
        for recipient in recipients {
            let mut found_valid = false;
            for consignment_endpoint in recipient.consignment_endpoints.iter_mut() {
                if consignment_endpoint.protocol != ConsignmentEndpointProtocol::RgbHttpJsonRpc
                    || !consignment_endpoint.usable
                {
                    debug!(
                        self.logger,
                        "Skipping consignment endpoint {:?}", consignment_endpoint
                    );
                    continue;
                }
                let proxy_url = consignment_endpoint.endpoint.clone();
                let consignment_res = self.rest_client.clone().post_consignment(
                    &proxy_url,
                    recipient.blinded_utxo.clone(),
                    consignment_path.clone(),
                )?;
                debug!(
                    self.logger,
                    "Consignment POST response: {:?}", consignment_res
                );

                if let Some(err) = consignment_res.error {
                    if err.code == -101 {
                        return Err(Error::BlindedUTXOAlreadyUsed)?;
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
                    consignment_endpoint.used = true;
                    found_valid = true;
                    break;
                }
            }
            if !found_valid {
                return Err(Error::NoValidConsignmentEndpoint);
            }
        }

        Ok(())
    }

    fn _save_transfers(
        &self,
        txid: String,
        transfer_info_map: BTreeMap<String, InfoAssetTransfer>,
        blank_allocations: HashMap<String, u64>,
        change_utxo_idx: i64,
        status: TransferStatus,
    ) -> Result<(), Error> {
        let created_at = now().unix_timestamp();
        let expiration = Some(created_at + DURATION_SEND_TRANSFER);

        let batch_transfer = DbBatchTransferActMod {
            txid: ActiveValue::Set(Some(txid)),
            status: ActiveValue::Set(status),
            expiration: ActiveValue::Set(expiration),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;

        for (asset_id, transfer_info) in transfer_info_map {
            let asset_spend = transfer_info.asset_spend;
            let recipients = transfer_info.recipients;

            let mut asset_transfer = DbAssetTransferActMod {
                user_driven: ActiveValue::Set(true),
                batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
                ..Default::default()
            };
            match transfer_info.asset_type {
                AssetType::Rgb20 => {
                    asset_transfer.asset_rgb20_id = ActiveValue::Set(Some(asset_id))
                }
                AssetType::Rgb121 => {
                    asset_transfer.asset_rgb121_id = ActiveValue::Set(Some(asset_id))
                }
            }
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
                    txo_idx: ActiveValue::Set(change_utxo_idx),
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
                    blinded_utxo: ActiveValue::Set(Some(recipient.blinded_utxo.clone())),
                    ..Default::default()
                };
                let transfer_idx = self.database.set_transfer(transfer)?;
                for consignment_endpoint in recipient.consignment_endpoints {
                    self._save_transfer_consignment_endpoint(transfer_idx, &consignment_endpoint)?;
                }
            }
        }

        for (asset_id, amt) in blank_allocations {
            let mut asset_transfer = DbAssetTransferActMod {
                user_driven: ActiveValue::Set(false),
                batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
                ..Default::default()
            };
            match self.database.get_asset_or_fail(asset_id.clone())? {
                AssetType::Rgb20 => {
                    asset_transfer.asset_rgb20_id = ActiveValue::Set(Some(asset_id))
                }
                AssetType::Rgb121 => {
                    asset_transfer.asset_rgb121_id = ActiveValue::Set(Some(asset_id))
                }
            }
            let asset_transfer_idx = self.database.set_asset_transfer(asset_transfer)?;
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(change_utxo_idx),
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
    ) -> Result<String, Error> {
        info!(self.logger, "Sending to: {:?}...", recipient_map);
        self._check_xprv()?;

        let unsigned_psbt = self.send_begin(online.clone(), recipient_map, donation, fee_rate)?;

        let mut psbt =
            PartiallySignedTransaction::from_str(&unsigned_psbt).map_err(InternalError::from)?;
        self.bdk_wallet
            .sign(&mut psbt, SignOptions::default())
            .map_err(InternalError::from)?;

        self.send_end(online, psbt.to_string())
    }

    /// Prepare the PSBT to send tokens according to the given recipient map.
    ///
    /// The `recipient_map` maps Asset IDs to a vector of [`Recipient`]s. Each recipient
    /// is specified by a `blinded_utxo` and the `amount` to send.
    ///
    /// If `donation` is true, the resulting transaction will be broadcast (by
    /// [`send_end`](Wallet::send_end)) as soon as it's ready, without the need for recipients to
    /// acknowledge the transfer.
    /// If `donation` is false, all recipients will need to ack the transfer before the transaction
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
    ) -> Result<String, Error> {
        info!(self.logger, "Sending (begin) to: {:?}...", recipient_map);
        self._check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        let mut db_data = self.database.get_db_data(false)?;
        self._handle_expired_transfers(&mut db_data)?;

        let blinded_utxos: Vec<String> = recipient_map
            .values()
            .map(|r| r.iter().map(|r| r.blinded_utxo.clone()).collect())
            .collect();
        let mut hasher = DefaultHasher::new();
        blinded_utxos.hash(&mut hasher);
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

        let mut transfer_info_map: BTreeMap<String, InfoAssetTransfer> = BTreeMap::new();
        for (asset_id, recipients) in recipient_map {
            let mut local_recipients: Vec<LocalRecipient> = vec![];
            for recipient in recipients.clone() {
                self._check_consignment_endpoints(&recipient.consignment_endpoints)?;

                let mut consignment_endpoints: Vec<LocalConsignmentEndpoint> = vec![];
                let mut found_valid = false;
                for endpoint_str in recipient.consignment_endpoints {
                    let consignment_endpoint = InvoiceConsignmentEndpoint::from_str(&endpoint_str)?;

                    match consignment_endpoint {
                        InvoiceConsignmentEndpoint::RgbHttpJsonRpc(url) => {
                            let mut local_consignment_endpoint = LocalConsignmentEndpoint {
                                protocol: ConsignmentEndpointProtocol::RgbHttpJsonRpc,
                                endpoint: url.clone(),
                                used: false,
                                usable: false,
                            };
                            if let Ok(server_info) = self.rest_client.clone().get_info(&url) {
                                if let Some(info) = server_info.result {
                                    if info.protocol_version == *PROXY_PROTOCOL_VERSION {
                                        local_consignment_endpoint.usable = true;
                                        found_valid = true;
                                    }
                                }
                            };
                            consignment_endpoints.push(local_consignment_endpoint);
                        }
                        InvoiceConsignmentEndpoint::Storm(addr) => {
                            consignment_endpoints.push(LocalConsignmentEndpoint {
                                protocol: ConsignmentEndpointProtocol::Storm,
                                endpoint: addr.to_string(),
                                used: false,
                                usable: false,
                            });
                        }
                        _ => return Err(Error::UnsupportedConsignmentEndpointProtocol),
                    }
                }

                if !found_valid {
                    return Err(Error::InvalidConsignmentEndpoints {
                        details: s!("no valid consignment endpoints"),
                    });
                }

                local_recipients.push(LocalRecipient {
                    blinded_utxo: recipient.blinded_utxo,
                    amount: recipient.amount,
                    consignment_endpoints,
                })
            }

            let asset_type = self.database.get_asset_or_fail(asset_id.clone())?;
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
                asset_type,
            };
            transfer_info_map.insert(asset_id.clone(), transfer_info);
        }

        // prepare BDK PSBT
        let mut all_inputs: Vec<OutPoint> = transfer_info_map
            .values()
            .cloned()
            .map(|i| i.asset_spend.input_outpoints)
            .collect::<Vec<Vec<OutPoint>>>()
            .concat();
        all_inputs.sort();
        all_inputs.dedup();
        let psbt = self._try_prepare_psbt(&input_unspents, &mut all_inputs, fee_rate)?;
        let vbytes = psbt.clone().extract_tx().vsize() as f32;
        let updated_fee_rate = ((vbytes + OPRET_VBYTES) / vbytes) * fee_rate;
        let mut psbt =
            self._try_prepare_psbt(&input_unspents, &mut all_inputs, updated_fee_rate)?;

        // prepare RGB PSBT
        self._prepare_rgb_psbt(
            &mut psbt,
            all_inputs.clone(),
            transfer_info_map.clone(),
            transfer_dir.clone(),
            donation,
            unspents.clone(),
            &db_data,
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
    /// Returns the txid of the signed PSBT that's been saved and optionally broadcast
    pub fn send_end(&self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger, "Sending (end)...");
        self._check_online(online)?;

        // save signed PSBT
        let psbt = PartiallySignedTransaction::from_str(&signed_psbt)?;
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
            let asset_dir = if info_contents.asset_type == AssetType::Rgb121 {
                let ass_dir = self.wallet_dir.join(ASSETS_DIR).join(asset_id.clone());
                if ass_dir.is_dir() {
                    Some(ass_dir)
                } else {
                    None
                }
            } else {
                None
            };
            self._post_transfer_data(&mut info_contents.recipients, asset_transfer_dir, asset_dir)?;

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
        )?;

        info!(self.logger, "Send (end) completed");
        Ok(txid)
    }
}

#[cfg(test)]
mod test;
