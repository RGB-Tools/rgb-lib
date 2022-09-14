//! RGB wallet
//!
//! This module defines the [`Wallet`] structure and all its related data.

use amplify::{bmap, s};
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
use bitcoin::psbt::serialize::Deserialize as BitcoinDeserialize;
use bitcoin::psbt::PartiallySignedTransaction;
use bitcoin::util::bip32::ExtendedPubKey;
use bitcoin::{Address, OutPoint, Transaction};
use bp::seals::txout::{CloseMethod, ExplicitSeal};
use electrum_client::{Client as ElectrumClient, ElectrumApi, Param};
use futures::executor::block_on;
use internet2::addr::ServiceAddr;
use lnpbp::chain::Chain as RgbNetwork;
use psbt::Psbt;
use reqwest::blocking::Client as RestClient;
use rgb::blank::BlankBundle;
use rgb::fungible::allocation::{AllocatedValue, OutpointValue as RgbOutpointValue, UtxobValue};
use rgb::psbt::{RgbExt, RgbInExt};
use rgb::{
    seal, Consignment, Contract, ContractId, IntoRevealedSeal, Node, StateTransfer,
    TransitionBundle,
};
use rgb20::schema::FieldType;
use rgb20::{Asset as RgbAsset, Rgb20};
use rgb_core::schema::{OwnedRightType, TransitionType};
use rgb_core::{Assignment, SealEndpoint, Validator};
use rgb_lib_migration::{Migrator, MigratorTrait};
use rgb_node::{rgbd, Config};
use rgb_rpc::client::Client;
use rgb_rpc::{ContractValidity, Reveal};
use sea_orm::{ActiveValue, ConnectOptions, Database, DeriveActiveEnum, EnumIter};
use serde::{Deserialize, Serialize};
use slog::{debug, error, info, Logger};
use std::cmp::min;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use stens::AsciiString;
use stored::Config as StoreConfig;
use strict_encoding::{StrictDecode, StrictEncode};

use crate::api::consignment_proxy::AckResponse;
use crate::api::ConsignmentProxy;
use crate::database::entities::asset::Model as DbAsset;
use crate::database::entities::coloring::{ActiveModel as DbColoringActMod, Model as DbColoring};
use crate::database::entities::transfer::{ActiveModel as DbTransferActMod, Model as DbTransfer};
use crate::database::entities::txo::{ActiveModel as DbTxoActMod, Model as DbTxo};
use crate::database::{ColoringType, LocalUnspent, RgbLibDatabase, TransferData};
use crate::error::{Error, InternalError};
use crate::utils::{
    calculate_descriptor_from_xprv, calculate_descriptor_from_xpub, get_txid, now, setup_logger,
    BitcoinNetwork,
};

const RGB_DB_NAME: &str = "rgb_db";
const BDK_DB_NAME: &str = "bdk_db";

const TRANSFER_DIR: &str = "transfers";
const TRANSFER_DATA_FILE: &str = "transfer_data.txt";
const SIGNED_PSBT_FILE: &str = "signed.psbt";
const CONSIGNMENT_FILE: &str = "consignment_out";
const CONSIGNMENT_RCV_FILE: &str = "rcv_compose.rgbc";

const UTXO_SIZE: u64 = 1000;
const UTXO_NUM: u8 = 5;

const MIN_CONFIRMATIONS: u8 = 1;

const MAX_ALLOCATIONS_PER_UTXO: u32 = 1;

const DURATION_SEND_TRANSFER: i64 = 3600;
const DURATION_RCV_TRANSFER: u32 = 86400;

/// An RGB asset
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Asset {
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

impl Asset {
    fn from_db_asset(x: DbAsset, balance: Balance) -> Asset {
        Asset {
            asset_id: x.asset_id,
            ticker: x.ticker,
            name: x.name,
            precision: x.precision,
            balance,
        }
    }
}

/// An asset balance
///
/// The settled balance includes all operations that have completed and are in a final status.
/// The future balance also includes operations that have not yet completed or are not yet final.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Balance {
    /// Settled balance
    pub settled: u64,
    /// Future balance, as if everything was settled
    pub future: u64,
}

/// Data for a UTXO blinding
pub struct BlindData {
    /// Blinded UTXO
    pub blinded_utxo: String,
    /// Secret used to blind the UTXO
    pub blinding_secret: u64,
    /// Expiration of the blinded_utxo
    pub expiration_timestamp: Option<i64>,
}

/// Supported database types
#[derive(Clone)]
pub enum DatabaseType {
    /// A SQLite database
    Sqlite,
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

impl From<Outpoint> for OutPoint {
    fn from(x: Outpoint) -> OutPoint {
        OutPoint::from_str(&x.to_string()).expect("outpoint should be parsable")
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

#[derive(Debug, Deserialize, Serialize)]
struct TransferInfoFile {
    asset_id: String,
    blinded_utxo: String,
    change_amount: u64,
    change_txo_idx: i64,
    input_allocations: HashMap<i64, u64>,
    auto_allocations: HashMap<String, u64>,
}

/// The status of a [`Transfer`]
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum TransferStatus {
    /// Waiting for the counterparty to take action
    #[sea_orm(num_value = 1)]
    WaitingCounterparty = 1,
    /// Waiting for the transfer transcation to be confirmed
    #[sea_orm(num_value = 2)]
    WaitingConfirmations = 2,
    /// Settled transfer, this status is final
    #[sea_orm(num_value = 3)]
    Settled = 3,
    /// Failed transfer, this status is final
    #[sea_orm(num_value = 4)]
    Failed = 4,
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
    /// Received amount
    pub received: u64,
    /// Sent amount
    pub sent: u64,
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
}

impl Transfer {
    fn from_db_transfer(x: DbTransfer, td: TransferData) -> Transfer {
        let blinding_secret = x.blinding_secret.map(|bs| {
            bs.parse::<u64>()
                .expect("DB should contain a valid u64 value")
        });
        Transfer {
            idx: x.idx,
            created_at: x.created_at,
            updated_at: x.updated_at,
            status: x.status,
            received: td.received,
            sent: td.sent,
            txid: x.txid,
            blinded_utxo: x.blinded_utxo,
            unblinded_utxo: td.unblinded_utxo,
            change_utxo: td.change_utxo,
            blinding_secret,
            expiration: x.expiration,
        }
    }
}

/// A wallet unspent
#[derive(Clone, Debug)]
pub struct Unspent {
    /// Bitcoin UTXO
    pub utxo: Utxo,
    /// RGB allocations on the utxo
    pub rgb_allocations: Vec<RgbAllocation>,
}

impl From<LocalUnspent> for Unspent {
    fn from(x: LocalUnspent) -> Unspent {
        Unspent {
            utxo: Utxo::from(x.utxo),
            rgb_allocations: x.rgb_allocations,
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
    /// Database type used by the wallet
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
        info!(logger, "Creating wallet in '{:?}'", wallet_dir);

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
                return Err(Error::InvalidBitcoinKeys());
            }
            let xkey: ExtendedKey = mnemonic
                .into_extended_key()
                .expect("a valid key should have been provided");
            let xprv = xkey
                .into_xprv(bdk_network)
                .expect("should be possible to get an extended private key");
            let descriptor = calculate_descriptor_from_xprv(xprv, false);
            let change_descriptor = calculate_descriptor_from_xprv(xprv, true);
            BdkWallet::new(
                &descriptor,
                Some(&change_descriptor),
                bdk_network,
                bdk_database,
            )
            .map_err(InternalError::from)?
        } else {
            let descriptor_pub = calculate_descriptor_from_xpub(xpub, false)?;
            let change_descriptor_pub = calculate_descriptor_from_xpub(xpub, true)?;
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
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .idle_timeout(Duration::from_secs(8))
            .max_lifetime(Duration::from_secs(8));
        let db_cnn = block_on(Database::connect(opt));
        let connection = db_cnn.map_err(InternalError::from)?;
        block_on(Migrator::up(&connection, None)).map_err(InternalError::from)?;
        let database = RgbLibDatabase::new(connection);
        let rest_client = RestClient::new();

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
        let call = (
            s!("blockchain.transaction.get"),
            vec![Param::String(txid), Param::Bool(true)],
        );
        Ok(self._electrum_client()?.raw_call(&call)?)
    }

    fn _sync_db_txos(&self) -> Result<(), Error> {
        debug!(self.logger, "Syncing TXOs...");
        self.bdk_wallet
            .sync(self._bdk_blockchain()?, SyncOptions { progress: None })
            .map_err(|e| Error::FailedBdkSync(e.to_string()))?;

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
            .map_err(|e| Error::FailedBroadcast(e.to_string()))?;
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
            return Err(Error::InvalidOnline());
        }
        Ok(())
    }

    fn _check_xprv(&self) -> Result<(), Error> {
        if self.watch_only {
            error!(self.logger, "Invalid operation for a watch only wallet");
            return Err(Error::WatchOnly());
        }
        Ok(())
    }

    fn _get_spendable_bitcoins(&self, unspents: Vec<LocalUnspent>) -> u64 {
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

    fn _handle_expired_transfers(&mut self) -> Result<(), Error> {
        let now = now().unix_timestamp();
        let expired_transfers: Vec<DbTransfer> = self
            .database
            .iter_transfers()?
            .into_iter()
            .filter(|t| t.waiting_counterparty() && t.expiration.unwrap_or(now) < now)
            .collect();
        for transfer in expired_transfers.iter() {
            let updated_transfer = self._refresh_transfer(transfer)?;
            if updated_transfer.is_none() {
                let mut updated_transfer: DbTransferActMod = transfer.clone().into();
                updated_transfer.status = ActiveValue::Set(TransferStatus::Failed);
                self.database.update_transfer(&mut updated_transfer)?;
            }
        }
        Ok(())
    }

    fn _get_utxo(&mut self, online: bool, exclude_utxos: Vec<Outpoint>) -> Result<DbTxo, Error> {
        if online {
            self._sync_db_txos()?;
            self._handle_expired_transfers()?;
        }

        let unspents: Vec<LocalUnspent> = self
            .database
            .get_rgb_allocations(self.database.get_unspent_txos()?, false)?;
        let allocatable: Vec<LocalUnspent> = unspents
            .iter()
            .filter(|u| !exclude_utxos.contains(&u.utxo.outpoint()))
            .filter(|u| {
                (u.rgb_allocations.len() as u32) < MAX_ALLOCATIONS_PER_UTXO && u.utxo.colorable
            })
            .cloned()
            .collect();
        match allocatable.first() {
            Some(u) => Ok(u.clone().utxo),
            None => {
                if self._get_spendable_bitcoins(unspents) < UTXO_SIZE * 2 {
                    Err(Error::InsufficientFunds)
                } else {
                    Err(Error::InsufficientAllocationSlots)
                }
            }
        }
    }

    /// Blind an UTXO and return the resulting [`BlindData`]
    ///
    /// Optional [`Asset`] ID and duration (in secods) can be specified
    pub fn blind(
        &mut self,
        asset_id: Option<String>,
        duration_seconds: Option<u32>,
    ) -> Result<BlindData, Error> {
        info!(
            self.logger,
            "Blinding for asset '{:?}' with duration '{:?}'...", asset_id, duration_seconds
        );
        let asset_id = if let Some(cid) = asset_id {
            Some(self.database.get_asset_or_fail(cid)?.asset_id)
        } else {
            None
        };

        let utxo = self._get_utxo(false, vec![])?;
        debug!(
            self.logger,
            "Blinding outpoint '{}'",
            utxo.outpoint().to_string()
        );

        let seal = seal::Revealed::new(CloseMethod::OpretFirst, OutPoint::from(utxo.clone()));
        let blinded_utxo = seal.to_concealed_seal().to_string();

        let created_at = now().unix_timestamp();
        let expiration = if duration_seconds == Some(0) {
            None
        } else {
            let duration_seconds = duration_seconds.unwrap_or(DURATION_RCV_TRANSFER) as i64;
            Some(created_at + duration_seconds)
        };
        let transfer = DbTransferActMod {
            status: ActiveValue::Set(TransferStatus::WaitingCounterparty),
            user_driven: ActiveValue::Set(true),
            asset_id: ActiveValue::Set(asset_id),
            blinded_utxo: ActiveValue::Set(Some(blinded_utxo.clone())),
            blinding_secret: ActiveValue::Set(Some(seal.blinding.to_string())),
            expiration: ActiveValue::Set(expiration),
            ..Default::default()
        };
        let transfer_idx = self.database.set_transfer(transfer)?;
        let db_coloring = DbColoringActMod {
            txo_idx: ActiveValue::Set(utxo.idx),
            transfer_idx: ActiveValue::Set(transfer_idx),
            coloring_type: ActiveValue::Set(ColoringType::Blind),
            amount: ActiveValue::Set(s!("0")),
            ..Default::default()
        };
        self.database.set_coloring(db_coloring)?;

        Ok(BlindData {
            blinded_utxo,
            blinding_secret: seal.blinding,
            expiration_timestamp: expiration,
        })
    }

    fn _create_split_tx(
        &self,
        inputs: &[OutPoint],
        num_utxos_to_create: u64,
    ) -> Result<PartiallySignedTransaction, bdk::Error> {
        let mut tx_builder = self.bdk_wallet.build_tx();
        tx_builder.add_utxos(inputs)?;
        tx_builder.manually_selected_only();
        for _i in 0..num_utxos_to_create {
            tx_builder.add_recipient(self._get_new_address().script_pubkey(), UTXO_SIZE as u64);
        }
        Ok(tx_builder.finish()?.0)
    }

    fn _create_split_psbt(&self) -> Result<String, Error> {
        let unspents: Vec<LocalUnspent> = self
            .database
            .get_rgb_allocations(self.database.get_unspent_txos()?, true)?;
        let inputs: Vec<OutPoint> = unspents
            .clone()
            .into_iter()
            .filter(|u| !u.utxo.colorable)
            .map(|u| OutPoint::from(u.utxo))
            .collect();
        let inputs: &[OutPoint] = &inputs;
        let new_btc_amount = self._get_spendable_bitcoins(unspents);
        let max_possible_utxos = new_btc_amount / UTXO_SIZE;
        let mut num_utxos_to_create = min(UTXO_NUM as u64, max_possible_utxos);
        while num_utxos_to_create > 0 {
            match self._create_split_tx(inputs, num_utxos_to_create) {
                Ok(_v) => break,
                Err(_e) => num_utxos_to_create -= 1,
            };
        }

        if num_utxos_to_create == 0 {
            Err(Error::InsufficientFunds)
        } else {
            Ok(self
                ._create_split_tx(inputs, num_utxos_to_create)
                .map_err(InternalError::from)?
                .to_string())
        }
    }

    /// Create new UTXOs to hold RGB allocations
    ///
    /// This is the full version, requiring a wallet with private keys and [`Online`] data
    pub fn create_utxos(&mut self, online: Online) -> Result<u64, Error> {
        info!(self.logger, "Creating UTXOs...");
        self._check_xprv()?;

        let unsigned_psbt = self.create_utxos_begin(online.clone())?;

        let mut psbt =
            PartiallySignedTransaction::from_str(&unsigned_psbt).map_err(InternalError::from)?;
        self.bdk_wallet
            .sign(&mut psbt, SignOptions::default())
            .map_err(InternalError::from)?;

        self.create_utxos_end(online, psbt.to_string())
    }

    /// Prepare the PSBT to create new UTXOs to hold RGB allocations
    ///
    /// This is the first half of the partial version, requiring no private keys nor [`Online`] data.
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the `create_utxos_end` function.
    ///
    /// Returns a PSBT ready to be signed
    pub fn create_utxos_begin(&mut self, online: Online) -> Result<String, Error> {
        info!(self.logger, "Creating UTXOs (begin)...");
        self._check_online(online)?;

        match self._get_utxo(true, vec![]) {
            Ok(_a) => Err(Error::AllocationsAlreadyAvailable()),
            Err(e) => match e {
                Error::InsufficientAllocationSlots => self._create_split_psbt(),
                _ => Err(e),
            },
        }
    }

    /// Broadcast the provided PSBT to create new UTXOs to hold RGB allocations
    ///
    /// This is the second half of the partial version, requiring [`Online`] data but no private keys.
    /// The provided PSBT, prepared with the `create_utxos_begin` function, needs to have already
    /// been signed.
    ///
    /// Returns the number of created UTXOs
    pub fn create_utxos_end(&self, online: Online, signed_psbt: String) -> Result<u64, Error> {
        info!(self.logger, "Creating UTXOs (end)...");
        self._check_online(online)?;

        let signed_psbt =
            PartiallySignedTransaction::from_str(&signed_psbt).map_err(Error::InvalidPsbt)?;
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

        Ok(num_utxos_created)
    }

    fn _delete_transfer(&self, transfer: &DbTransfer) -> Result<(), Error> {
        self.database.del_coloring(transfer.idx)?;
        Ok(self.database.del_transfer(transfer)?)
    }

    /// Delete eligible transfers from the databse
    ///
    /// An optional blinded_utxo can be provided to operate on a single transfer
    ///
    /// Eligible transfers are the ones in status [`TransferStatus::Failed`]
    pub fn delete_transfers(&self, blinded_utxo: Option<String>) -> Result<(), Error> {
        info!(self.logger, "Deleting transfer {:?}...", blinded_utxo);
        if let Some(bu) = blinded_utxo {
            let db_transfer = self.database.get_transfer_or_fail(bu.clone())?;
            if db_transfer.status != TransferStatus::Failed {
                return Err(Error::CannotDeleteTransfer(bu));
            }
            self._delete_transfer(&db_transfer)?;
        } else {
            let db_transfers: Vec<DbTransfer> = self
                .database
                .iter_transfers()?
                .into_iter()
                .filter(|t| t.status == TransferStatus::Failed)
                .collect();
            for db_transfer in db_transfers.iter() {
                self._delete_transfer(db_transfer)?
            }
        }

        Ok(())
    }

    /// Send bitcoin funds not used for RGB allocations, or all if `destroy_assets` is specified, to
    /// the provided address
    ///
    /// This is the full version, requiring a wallet with private keys and [`Online`] data
    pub fn drain_to(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
    ) -> Result<String, Error> {
        info!(
            self.logger,
            "Draining to '{}' destroying asset '{}'...", address, destroy_assets
        );
        self._check_xprv()?;

        let unsigned_psbt = self.drain_to_begin(online.clone(), address, destroy_assets)?;

        let mut psbt =
            PartiallySignedTransaction::from_str(&unsigned_psbt).map_err(InternalError::from)?;
        self.bdk_wallet
            .sign(&mut psbt, SignOptions::default())
            .map_err(InternalError::from)?;

        self.drain_to_end(online, psbt.to_string())
    }

    /// Prepare the PSBT to send bitcoin funds not used for RGB allocations, or all if
    /// `destroy_assets` is specified, to the provided address
    ///
    /// This is the first half of the partial version, requiring no private keys nor [`Online`] data.
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the `drain_to_end` function.
    ///
    /// Returns a PSBT ready to be signed
    pub fn drain_to_begin(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
    ) -> Result<String, Error> {
        info!(
            self.logger,
            "Draining (begin) to '{}' destroying asset '{}'...", address, destroy_assets
        );
        self._check_online(online)?;

        let address = Address::from_str(&address).map(|x| x.script_pubkey())?;

        let mut tx_builder = self.bdk_wallet.build_tx();
        tx_builder.drain_wallet().drain_to(address);

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

        Ok(tx_builder
            .finish()
            .map_err(|e| match e {
                bdk::Error::InsufficientFunds { .. } => Error::InsufficientFunds,
                _ => Error::from(InternalError::from(e)),
            })?
            .0
            .to_string())
    }

    /// Broadcast the provided PSBT to send bitcoin funds
    ///
    /// This is the second half of the partial version, requiring [`Online`] data but no private keys.
    /// The provided PSBT, prepared with the `drain_to_begin` function, needs to have already
    /// been signed.
    ///
    /// Returns the txid of the transaction that's been broadcast
    pub fn drain_to_end(&self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger, "Draining (end)...");
        self._check_online(online)?;

        let signed_psbt =
            PartiallySignedTransaction::from_str(&signed_psbt).map_err(Error::InvalidPsbt)?;
        let tx = self._broadcast_psbt(signed_psbt)?;

        Ok(tx.txid().to_string())
    }

    fn _fail_transfer(&self, transfer: &DbTransfer) -> Result<(), Error> {
        let mut updated_transfer: DbTransferActMod = transfer.clone().into();
        updated_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        updated_transfer.expiration = ActiveValue::Set(Some(now().unix_timestamp()));
        self.database.update_transfer(&mut updated_transfer)?;

        Ok(())
    }

    /// Set the status for eligible transfers to [`TransferStatus::Failed`]
    ///
    /// An optional blinded_utxo can be provided to operate on a single transfer
    ///
    /// Eligible transfer are the ones in status [`TransferStatus::WaitingCounterparty`] after a
    /// `refresh` has been performed
    pub fn fail_transfers(
        &mut self,
        online: Online,
        blinded_utxo: Option<String>,
    ) -> Result<(), Error> {
        info!(self.logger, "Failing transfer {:?}...", blinded_utxo);
        self._check_online(online)?;

        if let Some(bu) = blinded_utxo {
            let db_transfer = &mut self.database.get_transfer_or_fail(bu.clone())?;
            if !db_transfer.waiting_counterparty() {
                return Err(Error::CannotFailTransfer(bu));
            }
            let updated_transfer = self._refresh_transfer(db_transfer)?;
            // don't fail transfer if the status changed after a refresh
            if updated_transfer.is_some() {
                return Err(Error::CannotFailTransfer(bu));
            }
            self._fail_transfer(db_transfer)?
        } else {
            // fail all transfers in status WaitingCounterparty
            let mut db_transfers: Vec<DbTransfer> = self
                .database
                .iter_transfers()?
                .into_iter()
                .filter(|t| t.waiting_counterparty())
                .collect();
            for db_transfer in db_transfers.iter_mut() {
                let updated_transfer = self._refresh_transfer(db_transfer)?;
                // fail transfer if the status didn't change after a refresh
                if updated_transfer.is_none() {
                    self._fail_transfer(db_transfer)?
                }
            }
        }

        Ok(())
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
        self._get_new_address().to_string()
    }

    /// Return the balance for the requested asset
    pub fn get_asset_balance(&self, asset_id: String) -> Result<Balance, Error> {
        info!(self.logger, "Getting balance for asset '{}'...", asset_id);
        self.database.get_asset_balance(asset_id)
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
            return Err(Error::Inconsistency(s!(
                "spent bitcoins with another wallet"
            )));
        }

        let asset_ids: Vec<String> = self
            ._rgb_client()?
            .list_contracts()
            .map_err(InternalError::from)?
            .iter()
            .map(|id| id.to_string())
            .collect();
        let db_asset_ids: Vec<String> = self
            .database
            .iter_assets()?
            .into_iter()
            .map(|c| c.asset_id)
            .collect();
        if !db_asset_ids.iter().all(|i| asset_ids.contains(i)) {
            return Err(Error::Inconsistency(s!(
                "DB assets do not match with ones stored in RGB"
            )));
        }

        Ok(())
    }

    fn _go_online(
        &mut self,
        electrum_url: String,
        skip_consistency_check: bool,
    ) -> Result<Online, Error> {
        let online_id = now().unix_timestamp_nanos() as u64;
        let online = Online {
            id: online_id,
            electrum_url: electrum_url.clone(),
        };
        self.online = Some(online.clone());

        // check electrum server
        self.electrum_client = Some(
            ElectrumClient::new(&electrum_url)
                .map_err(|e| Error::InvalidElectrum(e.to_string()))?,
        );
        if self.bitcoin_network != BitcoinNetwork::Regtest {
            self._get_tx_details(get_txid(self.bitcoin_network))
                .map_err(|e| Error::InvalidElectrum(e.to_string()))?;
        }

        // BDK setup
        let config = ElectrumBlockchainConfig {
            url: electrum_url.clone(),
            socks5: None,
            retry: 3,
            timeout: Some(5),
            stop_gap: 20,
        };
        self.bdk_blockchain = Some(
            ElectrumBlockchain::from_config(&config)
                .map_err(|e| Error::InvalidElectrum(e.to_string()))?,
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
    pub fn go_online(
        &mut self,
        electrum_url: String,
        skip_consistency_check: bool,
    ) -> Result<Online, Error> {
        info!(self.logger, "Going online...");
        if let Some(online) = self.online.clone() {
            if electrum_url == online.electrum_url {
                Ok(online)
            } else {
                Err(Error::CannotChangeOnline())
            }
        } else {
            let online = self._go_online(electrum_url, skip_consistency_check);
            if online.is_err() {
                self.online = None;
                self.bdk_blockchain = None;
                self.electrum_client = None;
                self.rgb_client = None;
            }
            online
        }
    }

    /// Issue a new RGB [`Asset`] and return it
    pub fn issue_asset(
        &mut self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amount: u64,
    ) -> Result<Asset, Error> {
        info!(
            self.logger,
            "Issuing asset with ticker '{}' name '{}' precision '{}' amount '{}'...",
            ticker,
            name,
            precision,
            amount
        );
        self._check_online(online)?;

        let utxo = self._get_utxo(true, vec![])?;
        let outpoint = utxo.outpoint().to_string();
        debug!(self.logger, "Issuing asset on outpoint '{}'", outpoint);

        let allocations = vec![RgbOutpointValue::from_str(&format!("{amount}@{outpoint}"))
            .expect("allocation structure should be correct")];
        let asset = Contract::create_rgb20(
            RgbNetwork::from(self.bitcoin_network),
            AsciiString::from_str(&ticker).map_err(|e| Error::InvalidTicker(e.to_string()))?,
            AsciiString::from_str(&name).map_err(|e| Error::InvalidName(e.to_string()))?,
            precision,
            allocations,
            BTreeMap::new(),
            None,
            None,
        );
        let _rgb_asset =
            RgbAsset::try_from(&asset).expect("create_rgb20 does not match RGB20 schema");
        let force = true;
        let status = self
            ._rgb_client()?
            .register_contract(asset.clone(), force, |_| ())
            .map_err(InternalError::from)?;
        if !matches!(status, ContractValidity::Valid) {
            return Err(Error::FailedIssuance(format!("{:?}", status)));
        }

        let db_asset = DbAsset {
            idx: 0,
            asset_id: asset.contract_id().to_string(),
            ticker,
            name,
            precision,
        };
        self.database.set_asset(db_asset.clone())?;
        let transfer = DbTransferActMod {
            status: ActiveValue::Set(TransferStatus::Settled),
            user_driven: ActiveValue::Set(true),
            asset_id: ActiveValue::Set(Some(db_asset.asset_id.clone())),
            ..Default::default()
        };
        let transfer_idx = self.database.set_transfer(transfer)?;
        let db_coloring = DbColoringActMod {
            txo_idx: ActiveValue::Set(utxo.idx),
            transfer_idx: ActiveValue::Set(transfer_idx),
            coloring_type: ActiveValue::Set(ColoringType::Issue),
            amount: ActiveValue::Set(amount.to_string()),
            ..Default::default()
        };
        self.database.set_coloring(db_coloring)?;

        Ok(Asset::from_db_asset(
            db_asset,
            Balance {
                settled: amount,
                future: 0,
            },
        ))
    }

    /// List the [`Asset`]s known by the underlying RGB node
    pub fn list_assets(&self) -> Result<Vec<Asset>, Error> {
        info!(self.logger, "Listing assets...");
        self.database
            .iter_assets()?
            .iter()
            .map(|c| {
                Ok(Asset::from_db_asset(
                    c.clone(),
                    self.database.get_asset_balance(c.asset_id.clone())?,
                ))
            })
            .collect()
    }

    /// List the [`Transfer`]s known to the RGB wallet
    pub fn list_transfers(&self, asset_id: String) -> Result<Vec<Transfer>, Error> {
        info!(self.logger, "Listing transfers for asset '{}'...", asset_id);
        let _db_asset = self.database.get_asset_or_fail(asset_id.clone())?;
        self.database
            .iter_transfers()?
            .iter()
            .filter(|t| t.asset_id.clone() == Some(asset_id.clone()) && t.user_driven)
            .map(|t| {
                Ok(Transfer::from_db_transfer(
                    t.clone(),
                    self.database.get_transfer_data(t)?,
                ))
            })
            .collect()
    }

    /// List the [`Unspent`]s known to the RGB wallet,
    /// if "settled" is true only show settled allocations
    /// if "settled" is false also show pending allocations
    pub fn list_unspents(&self, settled_only: bool) -> Result<Vec<Unspent>, Error> {
        info!(self.logger, "Listing unspents...");

        let mut allocation_txos = self.database.get_unspent_txos()?;
        let spent_txos_ids: Vec<i64> = self
            .database
            .iter_txos()?
            .into_iter()
            .filter(|t| t.spent)
            .map(|u| u.idx)
            .collect();
        let waiting_confs_transfer_ids: Vec<i64> = self
            .database
            .iter_transfers()?
            .into_iter()
            .filter(|t| t.status == TransferStatus::WaitingConfirmations)
            .map(|t| t.idx)
            .collect();
        let almost_spent_txos_ids: Vec<i64> = self
            .database
            .iter_colorings()?
            .into_iter()
            .filter(|c| {
                waiting_confs_transfer_ids.contains(&c.transfer_idx)
                    && spent_txos_ids.contains(&c.txo_idx)
            })
            .map(|c| c.txo_idx)
            .collect();
        let mut spent_txos = self
            .database
            .iter_txos()?
            .into_iter()
            .filter(|t| almost_spent_txos_ids.contains(&t.idx))
            .collect();
        allocation_txos.append(&mut spent_txos);

        Ok(self
            .database
            .get_rgb_allocations(allocation_txos, settled_only)?
            .into_iter()
            .map(Unspent::from)
            .collect())
    }

    fn _get_signed_psbt(&self, transfer_dir: PathBuf) -> Result<PartiallySignedTransaction, Error> {
        let psbt_file = transfer_dir.join(SIGNED_PSBT_FILE);
        let psbt_str = fs::read_to_string(&psbt_file)?;
        PartiallySignedTransaction::from_str(&psbt_str).map_err(Error::InvalidPsbt)
    }

    fn _wait_consignment(&mut self, transfer: &DbTransfer) -> Result<Option<DbTransfer>, Error> {
        debug!(self.logger, "Waiting consignment...");
        let blinded_utxo = transfer
            .blinded_utxo
            .clone()
            .expect("transfer should have a blinded UTXO");

        // check if a consignment has been posted
        let consignment_res = self
            .rest_client
            .clone()
            .get_consignment(blinded_utxo.clone())?;
        debug!(
            self.logger,
            "Consignment GET response: {:?}", consignment_res
        );
        let consignment = if let Some(cons) = consignment_res.consignment {
            cons
        } else {
            return Ok(None);
        };

        let mut updated_transfer: DbTransferActMod = transfer.clone().into();

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

        // validate consignment
        let validation_status = Validator::validate(&consignment, self._electrum_client()?);
        let unmined_txid = validation_status.unmined_endpoint_txids.first();
        if !validation_status.failures.is_empty() || unmined_txid.is_none() {
            debug!(self.logger, "Consignment is invalid");
            let nack_res = self.rest_client.clone().post_nack(blinded_utxo)?;
            debug!(self.logger, "Consignment NACK response: {:?}", nack_res);
            updated_transfer.status = ActiveValue::Set(TransferStatus::Failed);
            return Ok(Some(self.database.update_transfer(&mut updated_transfer)?));
        }
        let transfer_txid = unmined_txid.expect("unmined txid should be there");
        debug!(self.logger, "Consignment is valid");
        let ack_res = self.rest_client.clone().post_ack(blinded_utxo)?;
        debug!(self.logger, "Consignment ACK response: {:?}", ack_res);

        // add asset info to transfer if missing
        if transfer.asset_id.is_none() {
            let contract_id = consignment.contract_id().to_string();
            // save asset in DB if unknown
            if self
                .database
                .get_asset_or_fail(contract_id.clone())
                .is_err()
            {
                // extract asset data from consignment
                let metadata = consignment.genesis().metadata();
                let ticker = metadata
                    .ascii_string(FieldType::Ticker)
                    .first()
                    .expect("valid consignment should contain the asset ticker")
                    .to_string();
                let name = metadata
                    .ascii_string(FieldType::Name)
                    .first()
                    .expect("valid consignment should contain the asset name")
                    .to_string();
                let precision = *metadata
                    .u8(FieldType::Precision)
                    .first()
                    .expect("valid consignment should contain the asset precision");
                let db_asset = DbAsset {
                    idx: 0,
                    asset_id: contract_id.clone(),
                    ticker,
                    name,
                    precision,
                };
                self.database.set_asset(db_asset)?;
            }
            updated_transfer.asset_id = ActiveValue::Set(Some(contract_id));
            self.database.update_transfer(&mut updated_transfer)?;
        }

        // get and update transfer amount
        let anchored_bundles = consignment.anchored_bundles();
        let mut amount = 0;
        for (anchor, transition_bundle) in anchored_bundles {
            if anchor.txid != *transfer_txid {
                continue;
            }
            let known_transitions = transition_bundle.known_transitions();
            for transition in known_transitions {
                let owned_rights = transition.owned_rights();
                for (_owned_right_type, typed_assignment) in owned_rights.iter() {
                    for assignment in typed_assignment.to_value_assignments() {
                        if let Assignment::ConfidentialSeal { seal: _, state } = assignment {
                            amount += state.value;
                        };
                    }
                }
            }
        }
        let transfer_colorings = self
            .database
            .iter_colorings()?
            .into_iter()
            .filter(|c| c.transfer_idx == transfer.idx && c.coloring_type == ColoringType::Blind)
            .collect::<Vec<DbColoring>>()
            .first()
            .cloned();
        let transfer_coloring =
            transfer_colorings.expect("transfer should be connected to at least one coloring");
        let mut updated_coloring: DbColoringActMod = transfer_coloring.into();
        updated_coloring.amount = ActiveValue::Set(amount.to_string());
        self.database.update_coloring(updated_coloring)?;

        updated_transfer.txid = ActiveValue::Set(Some(transfer_txid.to_string()));
        updated_transfer.status = ActiveValue::Set(TransferStatus::WaitingConfirmations);
        Ok(Some(self.database.update_transfer(&mut updated_transfer)?))
    }

    fn _wait_ack(&self, transfer: &DbTransfer) -> Result<Option<DbTransfer>, Error> {
        debug!(self.logger, "Waiting ACK...");
        let ack_res = self.rest_client.clone().get_ack(
            transfer
                .blinded_utxo
                .clone()
                .expect("transfer should have a blinded UTXO"),
        )?;
        debug!(self.logger, "Consignment ACK/NACK response: {:?}", ack_res);

        return match ack_res {
            AckResponse {
                ack: Some(true), ..
            } => {
                let transfer_dir = self
                    .wallet_dir
                    .join(TRANSFER_DIR)
                    .join(transfer.txid.as_ref().expect("transfer should have a txid"));
                let signed_psbt = self._get_signed_psbt(transfer_dir)?;
                self._broadcast_psbt(signed_psbt)?;
                let mut updated_transfer: DbTransferActMod = transfer.clone().into();
                updated_transfer.status = ActiveValue::Set(TransferStatus::WaitingConfirmations);
                Ok(Some(self.database.update_transfer(&mut updated_transfer)?))
            }
            AckResponse {
                nack: Some(true), ..
            } => {
                let mut updated_transfer: DbTransferActMod = transfer.clone().into();
                updated_transfer.status = ActiveValue::Set(TransferStatus::Failed);
                Ok(Some(self.database.update_transfer(&mut updated_transfer)?))
            }
            _ => Ok(None),
        };
    }

    fn _wait_confirmations(&mut self, transfer: &DbTransfer) -> Result<Option<DbTransfer>, Error> {
        debug!(self.logger, "Waiting confirmations...");
        let transfer_txid = transfer.txid.clone().expect("transfer should have a txid");
        let tx_details = match self._get_tx_details(transfer_txid.clone()) {
            Ok(v) => Ok(v),
            Err(e) => {
                if e.to_string()
                    .contains("No such mempool or blockchain transaction")
                {
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

        let transfer_dir = if transfer.incoming() {
            self.wallet_dir.join(TRANSFER_DIR).join(
                transfer
                    .blinded_utxo
                    .clone()
                    .expect("transfer should have a blinded UTXO"),
            )
        } else {
            self.wallet_dir.join(TRANSFER_DIR).join(transfer_txid)
        };

        if !transfer.incoming() {
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

        // accept consignment
        let consignment_path = if transfer.incoming() {
            transfer_dir.join(CONSIGNMENT_RCV_FILE)
        } else {
            transfer_dir.join(CONSIGNMENT_FILE)
        };
        let consignment =
            StateTransfer::strict_file_load(&consignment_path).map_err(InternalError::from)?;
        let reveal = if transfer.incoming() {
            let detailed_transfer = Transfer::from_db_transfer(
                transfer.clone(),
                self.database.get_transfer_data(transfer)?,
            );
            let blinding_factor = detailed_transfer
                .blinding_secret
                .expect("incoming transfer should have a blinding secret");
            let outpoint = OutPoint::from(
                detailed_transfer
                    .unblinded_utxo
                    .expect("incoming transfer should have a unblinded UTXO"),
            );
            Some(Reveal {
                blinding_factor,
                outpoint,
                close_method: CloseMethod::OpretFirst,
            })
        } else {
            None
        };
        let status = self
            ._rgb_client()?
            .consume_transfer(consignment, true, reveal, |_| ())
            .map_err(InternalError::from)?;
        if !matches!(status, ContractValidity::Valid) {
            return Err(InternalError::Unexpected)?;
        }

        let mut updated_transfer: DbTransferActMod = transfer.clone().into();
        updated_transfer.status = ActiveValue::Set(TransferStatus::Settled);
        let updated = self.database.update_transfer(&mut updated_transfer)?;

        Ok(Some(updated))
    }

    fn _wait_counterparty(&mut self, transfer: &DbTransfer) -> Result<Option<DbTransfer>, Error> {
        if transfer.incoming() {
            self._wait_consignment(transfer)
        } else {
            self._wait_ack(transfer)
        }
    }

    fn _refresh_transfer(&mut self, transfer: &DbTransfer) -> Result<Option<DbTransfer>, Error> {
        debug!(self.logger, "Refreshing transfer: {:?}", transfer);
        match transfer.status {
            TransferStatus::WaitingCounterparty => self._wait_counterparty(transfer),
            TransferStatus::WaitingConfirmations => self._wait_confirmations(transfer),
            _ => Ok(None),
        }
    }

    /// Refresh the status of pending transfers, optionally filtered by [`Asset`] ID
    pub fn refresh(&mut self, online: Online, asset_id: Option<String>) -> Result<(), Error> {
        if asset_id.is_some() {
            info!(self.logger, "Refreshing asset {:?}...", asset_id);
        } else {
            info!(self.logger, "Refreshing assets...");
        }
        self._check_online(online)?;

        let mut db_transfers: Vec<DbTransfer> = if asset_id.is_some() {
            self.database
                .iter_transfers()?
                .into_iter()
                .filter(|t| t.asset_id.clone() == asset_id.clone())
                .collect()
        } else {
            self.database.iter_transfers()?
        }
        .into_iter()
        .filter(|t| t.pending())
        .collect();

        for transfer in db_transfers.iter_mut() {
            self._refresh_transfer(transfer)?;
        }

        Ok(())
    }

    /// Send a specified amount of tokens of the given [`Asset`] ID to the provided blinded UTXO
    ///
    /// This is the full version, requiring a wallet with private keys and [`Online`] data
    pub fn send(
        &mut self,
        online: Online,
        asset_id: String,
        blinded_utxo: String,
        amount: u64,
    ) -> Result<String, Error> {
        info!(
            self.logger,
            "Sending {} of asset '{}' to blinded '{}'...", amount, asset_id, blinded_utxo
        );
        self._check_xprv()?;

        let unsigned_psbt = self.send_begin(online.clone(), asset_id, blinded_utxo, amount)?;

        let mut psbt =
            PartiallySignedTransaction::from_str(&unsigned_psbt).map_err(InternalError::from)?;
        self.bdk_wallet
            .sign(&mut psbt, SignOptions::default())
            .map_err(InternalError::from)?;

        self.send_end(online, psbt.to_string())
    }

    /// Prepare the PSBT to send a specified amount of tokens of the given [`Asset`] ID to the
    /// provided blinded UTXO
    ///
    /// This is the first half of the partial version, requiring no private keys nor [`Online`] data.
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the `send_end` function.
    ///
    /// Returns a PSBT ready to be signed
    pub fn send_begin(
        &mut self,
        online: Online,
        asset_id: String,
        blinded_utxo: String,
        amount: u64,
    ) -> Result<String, Error> {
        info!(
            self.logger,
            "Sending (begin) {} of asset '{}' to blinded '{}'...", amount, asset_id, blinded_utxo
        );
        self._check_online(online)?;

        self.database.get_asset_or_fail(asset_id.clone())?;

        if self
            .database
            .iter_transfers()?
            .iter()
            .any(|t| t.blinded_utxo == Some(blinded_utxo.clone()))
        {
            return Err(Error::BlindedUTXOAlreadyUsed)?;
        }

        let transfer_dir = self
            .wallet_dir
            .join(TRANSFER_DIR)
            .join(blinded_utxo.clone());
        if transfer_dir.is_dir() {
            fs::remove_dir_all(transfer_dir.clone())?;
        }
        fs::create_dir_all(transfer_dir.clone())?;

        // input selection
        let input_coloring_ids: Vec<i64> = self
            .database
            .iter_colorings()?
            .into_iter()
            .filter(|c| c.coloring_type == ColoringType::Input)
            .map(|c| c.txo_idx)
            .collect();
        let asset_txos = self
            .database
            .get_asset_utxos(asset_id.clone())?
            .into_iter()
            .filter(|t| !input_coloring_ids.contains(&t.idx))
            .collect();
        let unspents: Vec<LocalUnspent> = self
            .database
            .get_rgb_allocations(asset_txos, false)?
            .into_iter()
            .filter(|u| u.rgb_allocations.iter().all(|a| a.settled))
            .collect();
        let mut input_allocations: HashMap<DbTxo, u64> = HashMap::new();
        let mut amount_input_asset: u64 = 0;
        for unspent in unspents {
            let mut asset_allocations: Vec<RgbAllocation> = unspent
                .rgb_allocations
                .into_iter()
                .filter(|a| a.asset_id == Some(asset_id.clone()))
                .collect();
            asset_allocations.sort();
            let amount_allocation: u64 = asset_allocations.iter().map(|a| a.amount).sum();
            input_allocations.insert(unspent.utxo, amount_allocation);
            amount_input_asset += amount_allocation;
            if amount_input_asset >= amount {
                break;
            }
        }
        if amount_input_asset < amount {
            return Err(Error::InsufficientAssets);
        }
        debug!(self.logger, "Asset input amount {:?}", amount_input_asset);
        let inputs: Vec<DbTxo> = input_allocations.clone().into_keys().collect();
        inputs
            .iter()
            .for_each(|t| debug!(self.logger, "Input outpoint '{}'", t.outpoint().to_string()));
        let input_allocations: HashMap<i64, u64> = input_allocations
            .into_iter()
            .map(|(k, v)| (k.idx, v))
            .collect();

        // RGB node compose
        let input_outpoints: Vec<OutPoint> = inputs.into_iter().map(OutPoint::from).collect();
        let input_outpoints_bt: BTreeSet<OutPoint> = input_outpoints.clone().into_iter().collect();
        let rgb_asset_id = ContractId::from_str(&asset_id).map_err(InternalError::from)?;
        let transfer = self
            ._rgb_client()?
            .consign(rgb_asset_id, vec![], input_outpoints_bt.clone(), |_| ())
            .map_err(InternalError::from)?;
        let consignment_path = transfer_dir.join("compose.rgbc");
        let consignment_file = fs::File::create(consignment_path.clone())?;
        transfer
            .strict_encode(consignment_file)
            .map_err(InternalError::from)?;

        // RGB20 transfer
        let transfer = StateTransfer::strict_file_load(consignment_path.clone())
            .map_err(InternalError::from)?;
        let rgb_asset =
            RgbAsset::try_from(&transfer).expect("to have provided a valid consignment");
        let beneficiaries: Vec<UtxobValue> =
            vec![
                UtxobValue::from_str(&format!("{}@{}", amount, blinded_utxo))
                    .map_err(Error::InvalidBlindedUTXO)?,
            ];
        let change_amount = amount_input_asset - amount;
        debug!(self.logger, "Asset change amount {:?}", change_amount);
        let close_method = "opret1st";
        let change_utxo = self._get_utxo(
            true,
            input_outpoints
                .clone()
                .into_iter()
                .map(|t| t.into())
                .collect(),
        )?;
        debug!(
            self.logger,
            "Change outpoint '{}'",
            change_utxo.outpoint().to_string()
        );
        let change: Vec<AllocatedValue> = vec![AllocatedValue {
            value: change_amount as u64,
            seal: ExplicitSeal::from_str(&format!("{}:{}", close_method, change_utxo.outpoint(),))
                .map_err(InternalError::from)?,
        }];
        let beneficiaries: BTreeMap<SealEndpoint, u64> = beneficiaries
            .into_iter()
            .map(|v| (v.seal_confidential.into(), v.value))
            .collect();
        let revealed_seal = change
            .into_iter()
            .map(|v| (v.into_revealed_seal(), v.value))
            .collect();
        let transition = rgb_asset
            .transfer(
                input_outpoints_bt.clone(),
                beneficiaries.clone(),
                revealed_seal,
            )
            .expect("transfer should succeed");

        // prepare PSBT with bdk
        let change_address = self._get_new_address();
        let input_utxos: &[OutPoint] = &input_outpoints;
        let mut builder = self.bdk_wallet.build_tx();
        builder
            .add_utxos(input_utxos)
            .map_err(InternalError::from)?
            .manually_selected_only()
            .drain_to(change_address.script_pubkey())
            .fee_rate(FeeRate::from_sat_per_vb(1.5));
        let (psbt, _) = builder.finish().map_err(InternalError::from)?;

        // RGB node contract embed
        let node_types: Vec<TransitionType> = vec![];
        let contract = self
            ._rgb_client()?
            .contract(rgb_asset_id, node_types, |_| {})
            .map_err(InternalError::from)?;
        let psbt_bytes = serialize(&psbt);
        let mut psbt =
            <Psbt as BitcoinDeserialize>::deserialize(&psbt_bytes).map_err(InternalError::from)?;
        psbt.set_rgb_contract(contract)
            .map_err(InternalError::from)?;

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
        let outpoints: BTreeSet<_> = psbt
            .inputs
            .iter()
            .map(|input| input.previous_outpoint)
            .collect();
        let state_map = self
            ._rgb_client()?
            .outpoint_state(outpoints, |_| ())
            .map_err(InternalError::from)?;
        let change_outpoint = OutPoint::from(change_utxo.clone());
        let ty = transition
            .owned_right_types()
            .into_iter()
            .next()
            .expect("transition should contain an owned right");
        let new_outpoints: BTreeMap<OwnedRightType, (OutPoint, CloseMethod)> = bmap! {
            ty => (change_outpoint, CloseMethod::OpretFirst)
        };
        let mut auto_allocations: HashMap<String, u64> = HashMap::new();
        for (cid, outpoint_map) in state_map {
            if cid == rgb_asset_id {
                continue;
            }
            let mut moved_amount = 0;
            let blank_bundle = TransitionBundle::blank(&outpoint_map, &new_outpoints)
                .map_err(InternalError::from)?;
            for (transition, indexes) in blank_bundle.revealed_iter() {
                psbt.push_rgb_transition(transition.clone())
                    .map_err(InternalError::from)?;
                let transition_txid = transition
                    .revealed_seals()
                    .map_err(InternalError::from)?
                    .last()
                    .expect("revealed seal should be there")
                    .txid
                    .expect("revealed seal should have a txid");
                for no in indexes {
                    for input in psbt.inputs.iter_mut() {
                        if input.previous_outpoint.txid == transition_txid
                            && input.previous_outpoint.vout == *no as u32
                        {
                            input
                                .set_rgb_consumer(cid, transition.node_id())
                                .map_err(InternalError::from)?;
                        }
                    }
                }
            }
            let known_transitions = blank_bundle.known_transitions();
            for transition in known_transitions {
                let owned_rights = transition.owned_rights();
                for (_owned_right_type, typed_assignment) in owned_rights.iter() {
                    for assignment in typed_assignment.to_value_assignments() {
                        match assignment {
                            Assignment::ConfidentialSeal { seal: _, state } => {
                                moved_amount += state.value;
                            }
                            Assignment::Revealed { seal: _, state } => {
                                moved_amount += state.value;
                            }
                            _ => (),
                        };
                    }
                }
            }
            auto_allocations.insert(cid.to_string(), moved_amount);
        }

        // RGB std PSBT bundle
        let _count = psbt.rgb_bundle_to_lnpbp4().map_err(InternalError::from)?;
        psbt.outputs
            .last_mut()
            .expect("PSBT should have outputs")
            .set_opret_host()
            .expect("given output should be valid");

        // RGB node transfer finalize
        let endseals = beneficiaries.into_iter().map(|b| b.0).collect();
        let consignment =
            StateTransfer::strict_file_load(&consignment_path).map_err(InternalError::from)?;
        let transfer_consignment = self
            ._rgb_client()?
            .transfer(consignment, endseals, psbt.clone(), None, |_| ())
            .map_err(InternalError::from)?;
        let consignment_out = transfer_dir.join(CONSIGNMENT_FILE);
        transfer_consignment
            .consignment
            .strict_file_save(consignment_out)
            .map_err(InternalError::from)?;
        let psbt = transfer_consignment.psbt;
        let psbt_serialized =
            &Vec::<u8>::from_hex(&psbt.to_string()).expect("provided psbt should be valid");
        let psbt: PartiallySignedTransaction =
            deserialize(psbt_serialized).map_err(InternalError::from)?;

        // save transefer data to file (for send_end)
        let info_contents = TransferInfoFile {
            asset_id,
            blinded_utxo,
            change_amount,
            change_txo_idx: change_utxo.idx,
            input_allocations,
            auto_allocations,
        };
        let serialized_info = serde_json::to_string(&info_contents).map_err(InternalError::from)?;
        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        fs::write(info_file, serialized_info)?;

        // rename transfer directory
        let txid = psbt.clone().extract_tx().txid().to_string();
        let transfer_dir_txid = self.wallet_dir.join(TRANSFER_DIR).join(txid);
        fs::rename(transfer_dir, transfer_dir_txid)?;

        Ok(psbt.to_string())
    }

    /// Broadcast the provided PSBT to send tokens to a blinded UTXO
    ///
    /// This is the second half of the partial version, requiring [`Online`] data but no private keys.
    /// The provided PSBT, prepared with the `send_begin` function, needs to have already been
    /// signed.
    ///
    /// Returns the txid of the transaction that's been broadcast
    pub fn send_end(&self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger, "Sending (end)...");
        self._check_online(online)?;

        // save signed PSBT in transfer directory
        let psbt =
            PartiallySignedTransaction::from_str(&signed_psbt).map_err(Error::InvalidPsbt)?;
        let txid = psbt.clone().extract_tx().txid().to_string();
        let transfer_dir = self.wallet_dir.join(TRANSFER_DIR).join(txid.clone());
        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        let serialized_info = fs::read_to_string(info_file)?;
        let info_contents: TransferInfoFile =
            serde_json::from_str(&serialized_info).map_err(InternalError::from)?;
        let psbt_out = transfer_dir.join(SIGNED_PSBT_FILE);
        fs::write(psbt_out, psbt.to_string())?;

        // post consignment
        let consignment_out = transfer_dir.join(CONSIGNMENT_FILE);
        let consignment_res = self
            .rest_client
            .clone()
            .post_consignment(info_contents.blinded_utxo.clone(), consignment_out)?;
        debug!(
            self.logger,
            "Consignment POST response: {:?}", consignment_res
        );

        // save transfer to DB
        let created_at = now().unix_timestamp();
        let expiration = Some(created_at + DURATION_SEND_TRANSFER);
        let transfer = DbTransferActMod {
            status: ActiveValue::Set(TransferStatus::WaitingCounterparty), // waiting ACK from receiver
            user_driven: ActiveValue::Set(true),
            asset_id: ActiveValue::Set(Some(info_contents.asset_id)),
            txid: ActiveValue::Set(Some(txid.clone())),
            blinded_utxo: ActiveValue::Set(Some(info_contents.blinded_utxo.clone())),
            expiration: ActiveValue::Set(expiration),
            ..Default::default()
        };
        let transfer_idx = self.database.set_transfer(transfer)?;
        for (asset_id, amt) in info_contents.auto_allocations {
            let transfer = DbTransferActMod {
                status: ActiveValue::Set(TransferStatus::WaitingCounterparty),
                user_driven: ActiveValue::Set(false),
                asset_id: ActiveValue::Set(Some(asset_id)),
                txid: ActiveValue::Set(Some(txid.clone())),
                blinded_utxo: ActiveValue::Set(Some(info_contents.blinded_utxo.clone())),
                expiration: ActiveValue::Set(expiration),
                ..Default::default()
            };
            let transfer_idx = self.database.set_transfer(transfer)?;
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(info_contents.change_txo_idx),
                transfer_idx: ActiveValue::Set(transfer_idx),
                coloring_type: ActiveValue::Set(ColoringType::Change),
                amount: ActiveValue::Set(amt.to_string()),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }
        for (input, amount) in info_contents.input_allocations.into_iter() {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(input),
                transfer_idx: ActiveValue::Set(transfer_idx),
                coloring_type: ActiveValue::Set(ColoringType::Input),
                amount: ActiveValue::Set(amount.to_string()),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }
        let db_coloring = DbColoringActMod {
            txo_idx: ActiveValue::Set(info_contents.change_txo_idx),
            transfer_idx: ActiveValue::Set(transfer_idx),
            coloring_type: ActiveValue::Set(ColoringType::Change),
            amount: ActiveValue::Set(info_contents.change_amount.to_string()),
            ..Default::default()
        };
        self.database.set_coloring(db_coloring)?;

        Ok(txid)
    }
}

#[cfg(test)]
mod test;
