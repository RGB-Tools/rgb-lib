#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard};

uniffi_macros::include_scaffolding!("rgb-lib");

type AssetRgb20 = rgb_lib::wallet::AssetRgb20;
type AssetRgb121 = rgb_lib::wallet::AssetRgb121;
type AssetType = rgb_lib::wallet::AssetType;
type Assets = rgb_lib::wallet::Assets;
type Balance = rgb_lib::wallet::Balance;
type InvoiceData = rgb_lib::wallet::InvoiceData;
type BitcoinNetwork = rgb_lib::BitcoinNetwork;
type BlindData = rgb_lib::wallet::BlindData;
type DatabaseType = rgb_lib::wallet::DatabaseType;
type RgbLibInvoice = rgb_lib::wallet::Invoice;
type Keys = rgb_lib::keys::Keys;
type Media = rgb_lib::wallet::Media;
type Metadata = rgb_lib::wallet::Metadata;
type Online = rgb_lib::wallet::Online;
type Outpoint = rgb_lib::wallet::Outpoint;
type Recipient = rgb_lib::wallet::Recipient;
type RefreshFilter = rgb_lib::wallet::RefreshFilter;
type RefreshTransferStatus = rgb_lib::wallet::RefreshTransferStatus;
type RgbAllocation = rgb_lib::wallet::RgbAllocation;
type RgbLibBlindedUTXO = rgb_lib::wallet::BlindedUTXO;
type RgbLibError = rgb_lib::Error;
type RgbLibWallet = rgb_lib::wallet::Wallet;
type Transfer = rgb_lib::wallet::Transfer;
type TransferStatus = rgb_lib::TransferStatus;
type Unspent = rgb_lib::wallet::Unspent;
type Utxo = rgb_lib::wallet::Utxo;
type WalletData = rgb_lib::wallet::WalletData;

fn generate_keys(bitcoin_network: BitcoinNetwork) -> Keys {
    rgb_lib::generate_keys(bitcoin_network)
}

fn restore_keys(bitcoin_network: BitcoinNetwork, mnemonic: String) -> Result<Keys, RgbLibError> {
    rgb_lib::restore_keys(bitcoin_network, mnemonic)
}

struct BlindedUTXO {
    _blinded_utxo: RwLock<RgbLibBlindedUTXO>,
}

impl BlindedUTXO {
    fn new(blinded_utxo: String) -> Result<Self, RgbLibError> {
        Ok(BlindedUTXO {
            _blinded_utxo: RwLock::new(RgbLibBlindedUTXO::new(blinded_utxo)?),
        })
    }
}

struct Invoice {
    invoice: RwLock<RgbLibInvoice>,
}

impl Invoice {
    fn new(bech32_invoice: String) -> Result<Self, RgbLibError> {
        Ok(Invoice {
            invoice: RwLock::new(RgbLibInvoice::new(bech32_invoice)?),
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

    fn bech32_invoice(&self) -> String {
        self._get_invoice().bech32_invoice()
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

    fn blind(
        &self,
        asset_id: Option<String>,
        amount: Option<u64>,
        duration_seconds: Option<u32>,
    ) -> Result<BlindData, RgbLibError> {
        self._get_wallet().blind(asset_id, amount, duration_seconds)
    }

    fn create_utxos(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
    ) -> Result<u8, RgbLibError> {
        self._get_wallet().create_utxos(online, up_to, num, size)
    }

    fn create_utxos_begin(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .create_utxos_begin(online, up_to, num, size)
    }

    fn create_utxos_end(&self, online: Online, signed_psbt: String) -> Result<u8, RgbLibError> {
        self._get_wallet().create_utxos_end(online, signed_psbt)
    }

    fn delete_transfers(
        &self,
        blinded_utxo: Option<String>,
        txid: Option<String>,
        no_asset_only: bool,
    ) -> Result<bool, RgbLibError> {
        self._get_wallet()
            .delete_transfers(blinded_utxo, txid, no_asset_only)
    }

    fn drain_to(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet().drain_to(online, address, destroy_assets)
    }

    fn drain_to_begin(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .drain_to_begin(online, address, destroy_assets)
    }

    fn drain_to_end(&self, online: Online, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().drain_to_end(online, signed_psbt)
    }

    fn fail_transfers(
        &self,
        online: Online,
        blinded_utxo: Option<String>,
        txid: Option<String>,
        no_asset_only: bool,
    ) -> Result<bool, RgbLibError> {
        self._get_wallet()
            .fail_transfers(online, blinded_utxo, txid, no_asset_only)
    }

    fn get_address(&self) -> String {
        self._get_wallet().get_address()
    }

    fn get_asset_balance(&self, asset_id: String) -> Result<Balance, RgbLibError> {
        self._get_wallet().get_asset_balance(asset_id)
    }

    fn get_asset_metadata(
        &self,
        online: Online,
        asset_id: String,
    ) -> Result<Metadata, RgbLibError> {
        self._get_wallet().get_asset_metadata(online, asset_id)
    }

    fn go_online(
        &self,
        skip_consistency_check: bool,
        electrum_url: String,
        proxy_url: String,
    ) -> Result<Online, RgbLibError> {
        self._get_wallet()
            .go_online(skip_consistency_check, electrum_url, proxy_url)
    }

    fn issue_asset_rgb20(
        &self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetRgb20, RgbLibError> {
        self._get_wallet()
            .issue_asset_rgb20(online, ticker, name, precision, amounts)
    }

    fn issue_asset_rgb121(
        &self,
        online: Online,
        name: String,
        description: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        parent_id: Option<String>,
        file_path: Option<String>,
    ) -> Result<AssetRgb121, RgbLibError> {
        self._get_wallet().issue_asset_rgb121(
            online,
            name,
            description,
            precision,
            amounts,
            parent_id,
            file_path,
        )
    }

    fn list_assets(&self, filter_asset_types: Vec<AssetType>) -> Result<Assets, RgbLibError> {
        self._get_wallet().list_assets(filter_asset_types)
    }

    fn list_transfers(&self, asset_id: String) -> Result<Vec<Transfer>, RgbLibError> {
        self._get_wallet().list_transfers(asset_id)
    }

    fn list_unspents(&self, settled_only: bool) -> Result<Vec<Unspent>, RgbLibError> {
        self._get_wallet().list_unspents(settled_only)
    }

    fn refresh(
        &self,
        online: Online,
        asset_id: Option<String>,
        filter: Vec<RefreshFilter>,
    ) -> Result<bool, RgbLibError> {
        self._get_wallet().refresh(online, asset_id, filter)
    }

    fn send(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet().send(online, recipient_map, donation)
    }

    fn send_begin(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_begin(online, recipient_map, donation)
    }

    fn send_end(&self, online: Online, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().send_end(online, signed_psbt)
    }
}

uniffi::deps::static_assertions::assert_impl_all!(Wallet: Sync, Send);
