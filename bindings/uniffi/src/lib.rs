#![allow(clippy::too_many_arguments)]

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard},
};

use rgb_lib::{
    AssetSchema, Assignment as RgbLibAssignment, BitcoinNetwork, Error as RgbLibError,
    RecipientType, TransferStatus, TransportType,
    keys::Keys,
    wallet::{
        Address as RgbLibAddress, AssetCFA, AssetIFA, AssetNIA, AssetUDA, Assets,
        AssignmentsCollection, Balance, BlockTime, BtcBalance, DatabaseType, EmbeddedMedia,
        Invoice as RgbLibInvoice, InvoiceData as RgbLibInvoiceData, Media, Metadata, Online,
        Outpoint, ProofOfReserves, ReceiveData, Recipient as RgbLibRecipient,
        RecipientInfo as RgbLibRecipientInfo, RefreshFilter, RefreshTransferStatus,
        RefreshedTransfer, RgbAllocation as RgbLibRgbAllocation, SendResult, Token, TokenLight,
        Transaction, TransactionType, Transfer as RgbLibTransfer, TransferKind,
        TransferTransportEndpoint, TransportEndpoint as RgbLibTransportEndpoint,
        Unspent as RgbLibUnspent, Utxo, Wallet as RgbLibWallet, WalletData, WitnessData,
    },
};

uniffi::include_scaffolding!("rgb-lib");

// temporary solution needed because the Enum attribute doesn't support the Remote one
pub enum Assignment {
    Fungible { amount: u64 },
    NonFungible,
    InflationRight { amount: u64 },
    ReplaceRight,
    Any,
}
impl From<RgbLibAssignment> for Assignment {
    fn from(orig: RgbLibAssignment) -> Self {
        match orig {
            RgbLibAssignment::Fungible(amount) => Assignment::Fungible { amount },
            RgbLibAssignment::NonFungible => Assignment::NonFungible,
            RgbLibAssignment::InflationRight(amount) => Assignment::InflationRight { amount },
            RgbLibAssignment::ReplaceRight => Assignment::ReplaceRight,
            RgbLibAssignment::Any => Assignment::Any,
        }
    }
}
impl From<Assignment> for RgbLibAssignment {
    fn from(orig: Assignment) -> Self {
        match orig {
            Assignment::Fungible { amount } => RgbLibAssignment::Fungible(amount),
            Assignment::NonFungible => RgbLibAssignment::NonFungible,
            Assignment::InflationRight { amount } => RgbLibAssignment::InflationRight(amount),
            Assignment::ReplaceRight => RgbLibAssignment::ReplaceRight,
            Assignment::Any => RgbLibAssignment::Any,
        }
    }
}
pub struct InvoiceData {
    pub recipient_id: String,
    pub asset_schema: Option<AssetSchema>,
    pub asset_id: Option<String>,
    pub assignment: Assignment,
    pub assignment_name: Option<String>,
    pub network: BitcoinNetwork,
    pub expiration_timestamp: Option<i64>,
    pub transport_endpoints: Vec<String>,
}
impl From<RgbLibInvoiceData> for InvoiceData {
    fn from(orig: RgbLibInvoiceData) -> Self {
        Self {
            recipient_id: orig.recipient_id,
            asset_schema: orig.asset_schema,
            asset_id: orig.asset_id,
            assignment: orig.assignment.into(),
            assignment_name: orig.assignment_name,
            network: orig.network,
            expiration_timestamp: orig.expiration_timestamp,
            transport_endpoints: orig.transport_endpoints,
        }
    }
}
impl From<InvoiceData> for RgbLibInvoiceData {
    fn from(orig: InvoiceData) -> Self {
        RgbLibInvoiceData {
            recipient_id: orig.recipient_id,
            asset_schema: orig.asset_schema,
            asset_id: orig.asset_id,
            assignment: orig.assignment.into(),
            assignment_name: orig.assignment_name,
            network: orig.network,
            expiration_timestamp: orig.expiration_timestamp,
            transport_endpoints: orig.transport_endpoints,
        }
    }
}
pub struct Recipient {
    pub recipient_id: String,
    pub witness_data: Option<WitnessData>,
    pub assignment: Assignment,
    pub transport_endpoints: Vec<String>,
}
impl From<RgbLibRecipient> for Recipient {
    fn from(orig: RgbLibRecipient) -> Self {
        Self {
            recipient_id: orig.recipient_id,
            witness_data: orig.witness_data,
            assignment: orig.assignment.into(),
            transport_endpoints: orig.transport_endpoints,
        }
    }
}
impl From<Recipient> for RgbLibRecipient {
    fn from(orig: Recipient) -> Self {
        Self {
            recipient_id: orig.recipient_id,
            witness_data: orig.witness_data,
            assignment: orig.assignment.into(),
            transport_endpoints: orig.transport_endpoints,
        }
    }
}
pub struct RgbAllocation {
    pub asset_id: Option<String>,
    pub assignment: Assignment,
    pub settled: bool,
}
impl From<RgbLibRgbAllocation> for RgbAllocation {
    fn from(orig: RgbLibRgbAllocation) -> Self {
        Self {
            asset_id: orig.asset_id,
            assignment: orig.assignment.into(),
            settled: orig.settled,
        }
    }
}
impl From<RgbAllocation> for RgbLibRgbAllocation {
    fn from(orig: RgbAllocation) -> Self {
        Self {
            asset_id: orig.asset_id,
            assignment: orig.assignment.into(),
            settled: orig.settled,
        }
    }
}
pub struct Transfer {
    pub idx: i32,
    pub batch_transfer_idx: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: TransferStatus,
    pub requested_assignment: Option<Assignment>,
    pub assignments: Vec<Assignment>,
    pub kind: TransferKind,
    pub txid: Option<String>,
    pub recipient_id: Option<String>,
    pub receive_utxo: Option<Outpoint>,
    pub change_utxo: Option<Outpoint>,
    pub expiration: Option<i64>,
    pub transport_endpoints: Vec<TransferTransportEndpoint>,
    pub invoice_string: Option<String>,
}
impl From<RgbLibTransfer> for Transfer {
    fn from(orig: RgbLibTransfer) -> Self {
        Self {
            idx: orig.idx,
            batch_transfer_idx: orig.batch_transfer_idx,
            created_at: orig.created_at,
            updated_at: orig.updated_at,
            status: orig.status,
            requested_assignment: orig.requested_assignment.map(|a| a.into()),
            assignments: orig.assignments.into_iter().map(|a| a.into()).collect(),
            kind: orig.kind,
            txid: orig.txid,
            recipient_id: orig.recipient_id,
            receive_utxo: orig.receive_utxo,
            change_utxo: orig.change_utxo,
            expiration: orig.expiration,
            transport_endpoints: orig.transport_endpoints,
            invoice_string: orig.invoice_string.clone(),
        }
    }
}
impl From<Transfer> for RgbLibTransfer {
    fn from(orig: Transfer) -> Self {
        Self {
            idx: orig.idx,
            batch_transfer_idx: orig.batch_transfer_idx,
            created_at: orig.created_at,
            updated_at: orig.updated_at,
            status: orig.status,
            requested_assignment: orig.requested_assignment.map(|a| a.into()),
            assignments: orig.assignments.into_iter().map(|a| a.into()).collect(),
            kind: orig.kind,
            txid: orig.txid,
            recipient_id: orig.recipient_id,
            receive_utxo: orig.receive_utxo,
            change_utxo: orig.change_utxo,
            expiration: orig.expiration,
            transport_endpoints: orig.transport_endpoints,
            invoice_string: orig.invoice_string.clone(),
        }
    }
}
pub struct Unspent {
    pub utxo: Utxo,
    pub rgb_allocations: Vec<RgbAllocation>,
    pub pending_blinded: u32,
}
impl From<RgbLibUnspent> for Unspent {
    fn from(orig: RgbLibUnspent) -> Self {
        Self {
            utxo: orig.utxo,
            rgb_allocations: orig.rgb_allocations.into_iter().map(|a| a.into()).collect(),
            pending_blinded: orig.pending_blinded,
        }
    }
}
impl From<Unspent> for RgbLibUnspent {
    fn from(orig: Unspent) -> Self {
        Self {
            utxo: orig.utxo,
            rgb_allocations: orig.rgb_allocations.into_iter().map(|a| a.into()).collect(),
            pending_blinded: orig.pending_blinded,
        }
    }
}

fn generate_keys(bitcoin_network: BitcoinNetwork) -> Keys {
    rgb_lib::generate_keys(bitcoin_network)
}

fn restore_keys(bitcoin_network: BitcoinNetwork, mnemonic: String) -> Result<Keys, RgbLibError> {
    rgb_lib::restore_keys(bitcoin_network, mnemonic)
}

fn restore_backup(
    backup_path: String,
    password: String,
    data_dir: String,
) -> Result<(), RgbLibError> {
    rgb_lib::restore_backup(&backup_path, &password, &data_dir)
}

struct RecipientInfo {
    recipient_info: RwLock<RgbLibRecipientInfo>,
}

impl RecipientInfo {
    fn new(recipient_id: String) -> Result<Self, RgbLibError> {
        Ok(RecipientInfo {
            recipient_info: RwLock::new(RgbLibRecipientInfo::new(recipient_id)?),
        })
    }

    fn _get_recipient_info(&self) -> RwLockReadGuard<RgbLibRecipientInfo> {
        self.recipient_info.read().expect("recipient_info")
    }

    fn network(&self) -> BitcoinNetwork {
        self._get_recipient_info().network
    }

    fn recipient_type(&self) -> RecipientType {
        self._get_recipient_info().recipient_type
    }
}

struct TransportEndpoint {
    transport_endpoint: RwLock<RgbLibTransportEndpoint>,
}

impl TransportEndpoint {
    fn new(transport_endpoint: String) -> Result<Self, RgbLibError> {
        Ok(TransportEndpoint {
            transport_endpoint: RwLock::new(RgbLibTransportEndpoint::new(transport_endpoint)?),
        })
    }

    fn _get_transport_endpoint(&self) -> RwLockReadGuard<RgbLibTransportEndpoint> {
        self.transport_endpoint.read().expect("transport_endpoint")
    }

    fn transport_type(&self) -> TransportType {
        self._get_transport_endpoint().transport_type()
    }
}

struct Address {
    _address: RwLock<RgbLibAddress>,
}

impl Address {
    fn new(address_string: String, bitcoin_network: BitcoinNetwork) -> Result<Self, RgbLibError> {
        Ok(Address {
            _address: RwLock::new(RgbLibAddress::new(address_string, bitcoin_network)?),
        })
    }
}

struct Invoice {
    invoice: RwLock<RgbLibInvoice>,
}

impl Invoice {
    fn new(invoice_string: String) -> Result<Self, RgbLibError> {
        Ok(Invoice {
            invoice: RwLock::new(RgbLibInvoice::new(invoice_string)?),
        })
    }

    fn _get_invoice(&self) -> RwLockReadGuard<RgbLibInvoice> {
        self.invoice.read().expect("invoice")
    }

    fn invoice_data(&self) -> InvoiceData {
        self._get_invoice().invoice_data().into()
    }

    fn invoice_string(&self) -> String {
        self._get_invoice().invoice_string()
    }
}

struct Wallet {
    wallet_mutex: Mutex<RgbLibWallet>,
}

impl Wallet {
    fn new(wallet_data: WalletData) -> Result<Self, RgbLibError> {
        Ok(Wallet {
            wallet_mutex: Mutex::new(RgbLibWallet::new(wallet_data)?),
        })
    }

    fn _get_wallet(&self) -> MutexGuard<RgbLibWallet> {
        self.wallet_mutex.lock().expect("wallet")
    }

    fn get_wallet_data(&self) -> WalletData {
        self._get_wallet().get_wallet_data()
    }

    fn get_wallet_dir(&self) -> String {
        self._get_wallet()
            .get_wallet_dir()
            .to_string_lossy()
            .to_string()
    }

    fn get_media_dir(&self) -> String {
        self._get_wallet()
            .get_media_dir()
            .to_string_lossy()
            .to_string()
    }

    fn backup(&self, backup_path: String, password: String) -> Result<(), RgbLibError> {
        self._get_wallet().backup(&backup_path, &password)
    }

    fn backup_info(&self) -> Result<bool, RgbLibError> {
        self._get_wallet().backup_info()
    }

    fn blind_receive(
        &self,
        asset_id: Option<String>,
        assignment: Assignment,
        duration_seconds: Option<u32>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, RgbLibError> {
        self._get_wallet().blind_receive(
            asset_id,
            assignment.into(),
            duration_seconds,
            transport_endpoints,
            min_confirmations,
        )
    }

    fn witness_receive(
        &self,
        asset_id: Option<String>,
        assignment: Assignment,
        duration_seconds: Option<u32>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, RgbLibError> {
        self._get_wallet().witness_receive(
            asset_id,
            assignment.into(),
            duration_seconds,
            transport_endpoints,
            min_confirmations,
        )
    }

    fn finalize_psbt(&self, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().finalize_psbt(signed_psbt, None)
    }

    fn sign_psbt(&self, unsigned_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().sign_psbt(unsigned_psbt, None)
    }

    fn create_utxos(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<u8, RgbLibError> {
        self._get_wallet()
            .create_utxos(online, up_to, num, size, fee_rate, skip_sync)
    }

    fn create_utxos_begin(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .create_utxos_begin(online, up_to, num, size, fee_rate, skip_sync)
    }

    fn create_utxos_end(
        &self,
        online: Online,
        signed_psbt: String,
        skip_sync: bool,
    ) -> Result<u8, RgbLibError> {
        self._get_wallet()
            .create_utxos_end(online, signed_psbt, skip_sync)
    }

    fn delete_transfers(
        &self,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
    ) -> Result<bool, RgbLibError> {
        self._get_wallet()
            .delete_transfers(batch_transfer_idx, no_asset_only)
    }

    fn drain_to(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
        fee_rate: u64,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .drain_to(online, address, destroy_assets, fee_rate)
    }

    fn drain_to_begin(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
        fee_rate: u64,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .drain_to_begin(online, address, destroy_assets, fee_rate)
    }

    fn drain_to_end(&self, online: Online, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().drain_to_end(online, signed_psbt)
    }

    fn fail_transfers(
        &self,
        online: Online,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
        skip_sync: bool,
    ) -> Result<bool, RgbLibError> {
        self._get_wallet()
            .fail_transfers(online, batch_transfer_idx, no_asset_only, skip_sync)
    }

    fn get_address(&self) -> Result<String, RgbLibError> {
        self._get_wallet().get_address()
    }

    fn get_asset_balance(&self, asset_id: String) -> Result<Balance, RgbLibError> {
        self._get_wallet().get_asset_balance(asset_id)
    }

    fn get_btc_balance(
        &self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<BtcBalance, RgbLibError> {
        self._get_wallet().get_btc_balance(online, skip_sync)
    }

    fn get_asset_metadata(&self, asset_id: String) -> Result<Metadata, RgbLibError> {
        self._get_wallet().get_asset_metadata(asset_id)
    }

    fn get_fee_estimation(&self, online: Online, blocks: u16) -> Result<f64, RgbLibError> {
        self._get_wallet().get_fee_estimation(online, blocks)
    }

    fn go_online(
        &self,
        skip_consistency_check: bool,
        indexer_url: String,
    ) -> Result<Online, RgbLibError> {
        self._get_wallet()
            .go_online(skip_consistency_check, indexer_url)
    }

    fn issue_asset_nia(
        &self,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetNIA, RgbLibError> {
        self._get_wallet()
            .issue_asset_nia(ticker, name, precision, amounts)
    }

    fn issue_asset_uda(
        &self,
        ticker: String,
        name: String,
        details: Option<String>,
        precision: u8,
        media_file_path: Option<String>,
        attachments_file_paths: Vec<String>,
    ) -> Result<AssetUDA, RgbLibError> {
        self._get_wallet().issue_asset_uda(
            ticker,
            name,
            details,
            precision,
            media_file_path,
            attachments_file_paths,
        )
    }

    fn issue_asset_cfa(
        &self,
        name: String,
        details: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, RgbLibError> {
        self._get_wallet()
            .issue_asset_cfa(name, details, precision, amounts, file_path)
    }

    fn issue_asset_ifa(
        &self,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
        inflation_amounts: Vec<u64>,
        replace_rights_num: u8,
    ) -> Result<AssetIFA, RgbLibError> {
        self._get_wallet().issue_asset_ifa(
            ticker,
            name,
            precision,
            amounts,
            inflation_amounts,
            replace_rights_num,
        )
    }

    fn list_assets(&self, filter_asset_schemas: Vec<AssetSchema>) -> Result<Assets, RgbLibError> {
        self._get_wallet().list_assets(filter_asset_schemas)
    }

    fn list_transactions(
        &self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<Vec<Transaction>, RgbLibError> {
        self._get_wallet().list_transactions(online, skip_sync)
    }

    fn list_transfers(&self, asset_id: Option<String>) -> Result<Vec<Transfer>, RgbLibError> {
        Ok(self
            ._get_wallet()
            .list_transfers(asset_id)?
            .into_iter()
            .map(|t| t.into())
            .collect())
    }

    fn list_unspents(
        &self,
        online: Option<Online>,
        settled_only: bool,
        skip_sync: bool,
    ) -> Result<Vec<Unspent>, RgbLibError> {
        Ok(self
            ._get_wallet()
            .list_unspents(online, settled_only, skip_sync)?
            .into_iter()
            .map(|u| u.into())
            .collect())
    }

    fn refresh(
        &self,
        online: Online,
        asset_id: Option<String>,
        filter: Vec<RefreshFilter>,
        skip_sync: bool,
    ) -> Result<HashMap<i32, RefreshedTransfer>, RgbLibError> {
        self._get_wallet()
            .refresh(online, asset_id, filter, skip_sync)
    }

    fn send(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        skip_sync: bool,
    ) -> Result<SendResult, RgbLibError> {
        self._get_wallet().send(
            online,
            _convert_recipient_map(recipient_map),
            donation,
            fee_rate,
            min_confirmations,
            skip_sync,
        )
    }

    fn send_begin(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
    ) -> Result<String, RgbLibError> {
        self._get_wallet().send_begin(
            online,
            _convert_recipient_map(recipient_map),
            donation,
            fee_rate,
            min_confirmations,
        )
    }

    fn send_end(
        &self,
        online: Online,
        signed_psbt: String,
        skip_sync: bool,
    ) -> Result<SendResult, RgbLibError> {
        self._get_wallet().send_end(online, signed_psbt, skip_sync)
    }

    fn send_btc(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_btc(online, address, amount, fee_rate, skip_sync)
    }

    fn send_btc_begin(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_btc_begin(online, address, amount, fee_rate, skip_sync)
    }

    fn send_btc_end(
        &self,
        online: Online,
        signed_psbt: String,
        skip_sync: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_btc_end(online, signed_psbt, skip_sync)
    }

    fn sync(&self, online: Online) -> Result<(), RgbLibError> {
        self._get_wallet().sync(online)
    }
}

fn _convert_recipient_map(
    recipient_map: HashMap<String, Vec<Recipient>>,
) -> HashMap<String, Vec<RgbLibRecipient>> {
    recipient_map
        .into_iter()
        .map(|(key, recipients)| {
            let new_recipients = recipients.into_iter().map(Into::into).collect();
            (key, new_recipients)
        })
        .collect()
}

uniffi::deps::static_assertions::assert_impl_all!(Wallet: Sync, Send);
