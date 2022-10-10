use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

uniffi_macros::include_scaffolding!("rgb-lib");

type Asset = rgb_lib::wallet::Asset;
type Balance = rgb_lib::wallet::Balance;
type BitcoinNetwork = rgb_lib::BitcoinNetwork;
type BlindData = rgb_lib::wallet::BlindData;
type DatabaseType = rgb_lib::wallet::DatabaseType;
type Keys = rgb_lib::keys::Keys;
type Online = rgb_lib::wallet::Online;
type Outpoint = rgb_lib::wallet::Outpoint;
type Recipient = rgb_lib::wallet::Recipient;
type RgbAllocation = rgb_lib::wallet::RgbAllocation;
type RgbLibError = rgb_lib::Error;
type RgbLibWallet = rgb_lib::wallet::Wallet;
type Transfer = rgb_lib::wallet::Transfer;
type TransferStatus = rgb_lib::wallet::TransferStatus;
type Unspent = rgb_lib::wallet::Unspent;
type Utxo = rgb_lib::wallet::Utxo;
type WalletData = rgb_lib::wallet::WalletData;

fn generate_keys(bitcoin_network: BitcoinNetwork) -> Keys {
    rgb_lib::generate_keys(bitcoin_network)
}

fn restore_keys(bitcoin_network: BitcoinNetwork, mnemonic: String) -> Result<Keys, RgbLibError> {
    rgb_lib::restore_keys(bitcoin_network, mnemonic)
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
        duration_seconds: Option<u32>,
    ) -> Result<BlindData, RgbLibError> {
        self._get_wallet().blind(asset_id, duration_seconds)
    }

    fn create_utxos(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
    ) -> Result<u8, RgbLibError> {
        self._get_wallet().create_utxos(online, up_to, num)
    }

    fn create_utxos_begin(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
    ) -> Result<String, RgbLibError> {
        self._get_wallet().create_utxos_begin(online, up_to, num)
    }

    fn create_utxos_end(&self, online: Online, signed_psbt: String) -> Result<u8, RgbLibError> {
        self._get_wallet().create_utxos_end(online, signed_psbt)
    }

    fn delete_transfers(
        &self,
        blinded_utxo: Option<String>,
        txid: Option<String>,
    ) -> Result<(), RgbLibError> {
        self._get_wallet().delete_transfers(blinded_utxo, txid)
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
    ) -> Result<(), RgbLibError> {
        self._get_wallet()
            .fail_transfers(online, blinded_utxo, txid)
    }

    fn get_address(&self) -> String {
        self._get_wallet().get_address()
    }

    fn get_asset_balance(&self, asset_id: String) -> Result<Balance, RgbLibError> {
        self._get_wallet().get_asset_balance(asset_id)
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

    fn issue_asset(
        &self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<Asset, RgbLibError> {
        self._get_wallet()
            .issue_asset(online, ticker, name, precision, amounts)
    }

    fn list_assets(&self) -> Result<Vec<Asset>, RgbLibError> {
        self._get_wallet().list_assets()
    }

    fn list_transfers(&self, asset_id: String) -> Result<Vec<Transfer>, RgbLibError> {
        self._get_wallet().list_transfers(asset_id)
    }

    fn list_unspents(&self, settled_only: bool) -> Result<Vec<Unspent>, RgbLibError> {
        self._get_wallet().list_unspents(settled_only)
    }

    fn refresh(&self, online: Online, asset_id: Option<String>) -> Result<(), RgbLibError> {
        self._get_wallet().refresh(online, asset_id)
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
        self._get_wallet().send(online, recipient_map, donation)
    }

    fn send_end(&self, online: Online, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().send_end(online, signed_psbt)
    }
}

uniffi::deps::static_assertions::assert_impl_all!(Wallet: Sync, Send);
