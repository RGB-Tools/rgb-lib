#![allow(clippy::too_many_arguments)]

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard},
};

use rgb_lib::{
    bdk::BlockTime,
    keys::Keys,
    wallet::{
        Address as RgbLibAddress, AssetCFA, AssetIface, AssetNIA, AssetUDA, Assets, Balance,
        BtcBalance, DatabaseType, EmbeddedMedia, Invoice as RgbLibInvoice, InvoiceData, Media,
        Metadata, Online, Outpoint, ProofOfReserves, ReceiveData, Recipient,
        RecipientInfo as RgbLibRecipientInfo, RefreshFilter, RefreshTransferStatus,
        RefreshedTransfer, RgbAllocation, SendResult, Token, TokenLight, Transaction,
        TransactionType, Transfer, TransferKind, TransferTransportEndpoint,
        TransportEndpoint as RgbLibTransportEndpoint, Unspent, Utxo, Wallet as RgbLibWallet,
        WalletData, WitnessData,
    },
    AssetSchema, BitcoinNetwork, Error as RgbLibError, RecipientType, TransferStatus,
    TransportType,
};

uniffi::include_scaffolding!("rgb-lib");

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

    fn from_invoice_data(invoice_data: InvoiceData) -> Result<Self, RgbLibError> {
        Ok(Invoice {
            invoice: RwLock::new(RgbLibInvoice::from_invoice_data(invoice_data)?),
        })
    }

    fn _get_invoice(&self) -> RwLockReadGuard<RgbLibInvoice> {
        self.invoice.read().expect("invoice")
    }

    fn invoice_data(&self) -> InvoiceData {
        self._get_invoice().invoice_data()
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
        amount: Option<u64>,
        duration_seconds: Option<u32>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, RgbLibError> {
        self._get_wallet().blind_receive(
            asset_id,
            amount,
            duration_seconds,
            transport_endpoints,
            min_confirmations,
        )
    }

    fn witness_receive(
        &self,
        asset_id: Option<String>,
        amount: Option<u64>,
        duration_seconds: Option<u32>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, RgbLibError> {
        self._get_wallet().witness_receive(
            asset_id,
            amount,
            duration_seconds,
            transport_endpoints,
            min_confirmations,
        )
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
        fee_rate: f32,
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
        fee_rate: f32,
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
        fee_rate: f32,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .drain_to(online, address, destroy_assets, fee_rate)
    }

    fn drain_to_begin(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
        fee_rate: f32,
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
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetNIA, RgbLibError> {
        self._get_wallet()
            .issue_asset_nia(online, ticker, name, precision, amounts)
    }

    fn issue_asset_uda(
        &self,
        online: Online,
        ticker: String,
        name: String,
        details: Option<String>,
        precision: u8,
        media_file_path: Option<String>,
        attachments_file_paths: Vec<String>,
    ) -> Result<AssetUDA, RgbLibError> {
        self._get_wallet().issue_asset_uda(
            online,
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
        online: Online,
        name: String,
        details: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, RgbLibError> {
        self._get_wallet()
            .issue_asset_cfa(online, name, details, precision, amounts, file_path)
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
        self._get_wallet().list_transfers(asset_id)
    }

    fn list_unspents(
        &self,
        online: Option<Online>,
        settled_only: bool,
        skip_sync: bool,
    ) -> Result<Vec<Unspent>, RgbLibError> {
        self._get_wallet()
            .list_unspents(online, settled_only, skip_sync)
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
        fee_rate: f32,
        min_confirmations: u8,
        skip_sync: bool,
    ) -> Result<SendResult, RgbLibError> {
        self._get_wallet().send(
            online,
            recipient_map,
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
        fee_rate: f32,
        min_confirmations: u8,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_begin(online, recipient_map, donation, fee_rate, min_confirmations)
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
        fee_rate: f32,
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
        fee_rate: f32,
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

uniffi::deps::static_assertions::assert_impl_all!(Wallet: Sync, Send);
