#![allow(clippy::too_many_arguments)]

use rgb_lib::{ScriptBuf, SecretSeal};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard};

uniffi::include_scaffolding!("rgb-lib");

type AssetCFA = rgb_lib::wallet::AssetCFA;
type AssetIface = rgb_lib::wallet::AssetIface;
type AssetNIA = rgb_lib::wallet::AssetNIA;
type AssetSchema = rgb_lib::AssetSchema;
type Assets = rgb_lib::wallet::Assets;
type Balance = rgb_lib::wallet::Balance;
type BitcoinNetwork = rgb_lib::BitcoinNetwork;
type BlockTime = rgb_lib::wallet::BlockTime;
type BtcBalance = rgb_lib::wallet::BtcBalance;
type DatabaseType = rgb_lib::wallet::DatabaseType;
type InvoiceData = rgb_lib::wallet::InvoiceData;
type Keys = rgb_lib::keys::Keys;
type Media = rgb_lib::wallet::Media;
type Metadata = rgb_lib::wallet::Metadata;
type Online = rgb_lib::wallet::Online;
type Outpoint = rgb_lib::wallet::Outpoint;
type ReceiveData = rgb_lib::wallet::ReceiveData;
type RecipientData = rgb_lib::wallet::RecipientData;
type RefreshFilter = rgb_lib::wallet::RefreshFilter;
type RefreshTransferStatus = rgb_lib::wallet::RefreshTransferStatus;
type RgbAllocation = rgb_lib::wallet::RgbAllocation;
type RgbLibBlindedUTXO = rgb_lib::wallet::BlindedUTXO;
type RgbLibError = rgb_lib::Error;
type RgbLibInvoice = rgb_lib::wallet::Invoice;
type RgbLibRecipient = rgb_lib::wallet::Recipient;
type RgbLibTransportEndpoint = rgb_lib::wallet::TransportEndpoint;
type RgbLibWallet = rgb_lib::wallet::Wallet;
type Transaction = rgb_lib::wallet::Transaction;
type TransactionType = rgb_lib::wallet::TransactionType;
type Transfer = rgb_lib::wallet::Transfer;
type TransferKind = rgb_lib::wallet::TransferKind;
type TransferStatus = rgb_lib::TransferStatus;
type TransferTransportEndpoint = rgb_lib::wallet::TransferTransportEndpoint;
type TransportType = rgb_lib::TransportType;
type Unspent = rgb_lib::wallet::Unspent;
type Utxo = rgb_lib::wallet::Utxo;
type WalletData = rgb_lib::wallet::WalletData;

pub struct Recipient {
    /// Blinded UTXO
    pub blinded_utxo: Option<String>,
    /// Script data
    pub script_data: Option<ScriptData>,
    /// RGB amount
    pub amount: u64,
    /// Transport endpoints
    pub transport_endpoints: Vec<String>,
}

pub struct ScriptData {
    /// The script
    script: String,
    /// The Bitcoin amount
    amount_sat: u64,
    /// An optional blinding
    blinding: Option<u64>,
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

    fn backup(&self, backup_path: String, password: String) -> Result<(), RgbLibError> {
        self._get_wallet().backup(&backup_path, &password)
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
        self._get_wallet().sign_psbt(unsigned_psbt)
    }

    fn create_utxos(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: f32,
    ) -> Result<u8, RgbLibError> {
        self._get_wallet()
            .create_utxos(online, up_to, num, size, fee_rate)
    }

    fn create_utxos_begin(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: f32,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .create_utxos_begin(online, up_to, num, size, fee_rate)
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

    fn get_btc_balance(&self, online: Online) -> Result<BtcBalance, RgbLibError> {
        self._get_wallet().get_btc_balance(online)
    }

    fn get_asset_metadata(&self, asset_id: String) -> Result<Metadata, RgbLibError> {
        self._get_wallet().get_asset_metadata(asset_id)
    }

    fn go_online(
        &self,
        skip_consistency_check: bool,
        electrum_url: String,
    ) -> Result<Online, RgbLibError> {
        self._get_wallet()
            .go_online(skip_consistency_check, electrum_url)
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

    fn issue_asset_cfa(
        &self,
        online: Online,
        name: String,
        description: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, RgbLibError> {
        self._get_wallet()
            .issue_asset_cfa(online, name, description, precision, amounts, file_path)
    }

    fn list_assets(&self, filter_asset_schemas: Vec<AssetSchema>) -> Result<Assets, RgbLibError> {
        self._get_wallet().list_assets(filter_asset_schemas)
    }

    fn list_transactions(&self, online: Option<Online>) -> Result<Vec<Transaction>, RgbLibError> {
        self._get_wallet().list_transactions(online)
    }

    fn list_transfers(&self, asset_id: Option<String>) -> Result<Vec<Transfer>, RgbLibError> {
        self._get_wallet().list_transfers(asset_id)
    }

    fn list_unspents(
        &self,
        online: Option<Online>,
        settled_only: bool,
    ) -> Result<Vec<Unspent>, RgbLibError> {
        self._get_wallet().list_unspents(online, settled_only)
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
        fee_rate: f32,
        min_confirmations: u8,
    ) -> Result<String, RgbLibError> {
        self._get_wallet().send(
            online,
            _convert_recipient_map(recipient_map)?,
            donation,
            fee_rate,
            min_confirmations,
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
        self._get_wallet().send_begin(
            online,
            _convert_recipient_map(recipient_map)?,
            donation,
            fee_rate,
            min_confirmations,
        )
    }

    fn send_end(&self, online: Online, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().send_end(online, signed_psbt)
    }

    fn send_btc(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: f32,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_btc(online, address, amount, fee_rate)
    }
}

fn _convert_recipient_map(
    recipient_map: HashMap<String, Vec<Recipient>>,
) -> Result<HashMap<String, Vec<RgbLibRecipient>>, RgbLibError> {
    let mut updated_map = HashMap::new();
    for (k, v) in recipient_map {
        let updated_v: Result<Vec<RgbLibRecipient>, RgbLibError> = v
            .iter()
            .map(|r| {
                if r.script_data.is_some() && r.blinded_utxo.is_some() {
                    return Err(RgbLibError::InvalidRecipientID);
                }
                let recipient_data = if let Some(script_data) = &r.script_data {
                    let script_buf = ScriptBuf::from_hex(&script_data.script).map_err(|e| {
                        RgbLibError::InvalidScript {
                            details: e.to_string(),
                        }
                    })?;
                    RecipientData::WitnessData {
                        script_buf,
                        amount_sat: script_data.amount_sat,
                        blinding: script_data.blinding,
                    }
                } else if let Some(blinded_utxo) = &r.blinded_utxo {
                    let secret_seal = SecretSeal::from_str(blinded_utxo).map_err(|e| {
                        RgbLibError::InvalidBlindedUTXO {
                            details: e.to_string(),
                        }
                    })?;
                    RecipientData::BlindedUTXO(secret_seal)
                } else {
                    return Err(RgbLibError::InvalidRecipientID);
                };
                Ok(RgbLibRecipient {
                    recipient_data,
                    amount: r.amount,
                    transport_endpoints: r.transport_endpoints.clone(),
                })
            })
            .collect();
        updated_map.insert(k, updated_v?);
    }
    Ok(updated_map)
}

uniffi::deps::static_assertions::assert_impl_all!(Wallet: Sync, Send);
