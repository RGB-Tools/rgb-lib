//! RGB wallet
//!
//! This module defines the online methods of the [`Wallet`] structure and all its related data.

use super::*;

const TRANSFER_DIR: &str = "transfers";

const TRANSFER_DATA_FILE: &str = "transfer_data.txt";
const SIGNED_PSBT_FILE: &str = "signed.psbt";
pub(crate) const CONSIGNMENT_FILE: &str = "consignment_out";
const CONSIGNMENT_RCV_FILE: &str = "rcv_compose.rgbc";

const OPRET_VBYTES: f32 = 43.0;

pub(crate) const UTXO_SIZE: u32 = 1000;
pub(crate) const UTXO_NUM: u8 = 5;

pub(crate) const MAX_ATTACHMENTS: usize = 20;

pub(crate) const MIN_FEE_RATE: f32 = 1.0;
const MAX_FEE_RATE: f32 = 1000.0;

pub(crate) const DURATION_SEND_TRANSFER: i64 = 3600;

pub(crate) const INDEXER_STOP_GAP: usize = 20;
const INDEXER_TIMEOUT: u8 = 4;

const PROXY_PROTOCOL_VERSION: &str = "0.2";

pub(crate) const UDA_FIXED_INDEX: u32 = 0;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AssetSpend {
    txo_map: HashMap<i32, u64>,
    input_outpoints: Vec<BdkOutPoint>,
    change_amount: u64,
}

/// The bitcoin balances (in sats) for the vanilla and colored wallets.
///
/// The settled balances include the confirmed balance.
/// The future balances also include the immature balance and the untrusted and trusted pending
/// balances.
/// The spendable balances include the settled balance and also the untrusted and trusted pending
/// balances.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct BtcBalance {
    /// Funds that will never hold RGB assets
    pub vanilla: Balance,
    /// Funds that may hold RGB assets
    pub colored: Balance,
}

/// The result of a send operation
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SendResult {
    /// ID of the transaction
    pub txid: String,
    /// Batch transfer idx
    pub batch_transfer_idx: i32,
}

#[derive(Debug, Deserialize, Serialize)]
struct BtcChange {
    vout: u32,
    amount: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct InfoBatchTransfer {
    btc_change: Option<BtcChange>,
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

#[non_exhaustive]
pub(crate) enum Indexer {
    #[cfg(feature = "electrum")]
    Electrum(Box<ElectrumClient>),
    #[cfg(feature = "esplora")]
    Esplora(Box<EsploraClient>),
}

impl Indexer {
    fn block_hash(&self, height: usize) -> Result<String, Error> {
        Ok(match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => client.block_header(height)?.block_hash().to_string(),
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => client.get_block_hash(height as u32)?.to_string(),
        })
    }

    pub(crate) fn get_tx_confirmations(&self, txid: &str) -> Result<Option<u64>, Error> {
        Ok(match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                let tx_details = match client.raw_call(
                    "blockchain.transaction.get",
                    vec![Param::String(txid.to_string()), Param::Bool(true)],
                ) {
                    Ok(td) => Ok(td),
                    Err(e) => {
                        if e.to_string()
                            .contains("No such mempool or blockchain transaction")
                        {
                            return Ok(None);
                        } else if e.to_string().contains(
                            "genesis block coinbase is not considered an ordinary transaction",
                        ) {
                            return Ok(Some(u64::MAX));
                        } else {
                            Err(e)
                        }
                    }
                }?;
                if let Some(confirmations) = tx_details.get("confirmations") {
                    Some(
                        confirmations
                            .as_u64()
                            .expect("confirmations to be a valid u64 number"),
                    )
                } else {
                    Some(0)
                }
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => {
                let txid = Txid::from_str(txid).unwrap();
                let tx_status = client.get_tx_status(&txid)?;
                if let Some(tx_height) = tx_status.block_height {
                    let height = client.get_height()?;
                    Some((height - tx_height + 1) as u64)
                } else if client.get_tx(&txid)?.is_none() {
                    None
                } else {
                    Some(0)
                }
            }
        })
    }
}

pub(crate) struct OnlineData {
    id: u64,
    bdk_blockchain: AnyBlockchain,
    pub(crate) indexer_url: String,
    indexer: Indexer,
}

/// A transfer refresh filter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct RefreshFilter {
    /// Transfer status
    pub status: RefreshTransferStatus,
    /// Whether the transfer is incoming
    pub incoming: bool,
}

/// A refreshed transfer
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RefreshedTransfer {
    /// The updated transfer status, if it has changed
    pub updated_status: Option<TransferStatus>,
    /// Optional failure
    pub failure: Option<Error>,
}

/// The result of a refresh operation
pub type RefreshResult = HashMap<i32, RefreshedTransfer>;

pub(crate) trait RefreshResultTrait {
    fn transfers_changed(&self) -> bool;
}

impl RefreshResultTrait for RefreshResult {
    fn transfers_changed(&self) -> bool {
        self.values().any(|rt| rt.updated_status.is_some())
    }
}

/// The pending status of a [`Transfer`] (eligible for refresh).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum RefreshTransferStatus {
    /// Waiting for the counterparty to take action
    WaitingCounterparty = 1,
    /// Waiting for the transfer transaction to reach the minimum number of confirmations
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

impl Wallet {
    pub(crate) fn bdk_blockchain(&self) -> Result<&AnyBlockchain, InternalError> {
        match self.online_data {
            Some(ref x) => Ok(&x.bdk_blockchain),
            None => Err(InternalError::Unexpected),
        }
    }

    pub(crate) fn testnet(&self) -> bool {
        !matches!(self.bitcoin_network(), BitcoinNetwork::Mainnet)
    }

    pub(crate) fn indexer(&self) -> &Indexer {
        &self.online_data.as_ref().unwrap().indexer
    }

    pub(crate) fn blockchain_resolver(&self) -> Result<AnyResolver, Error> {
        let indexer_url = &self.online_data.as_ref().unwrap().indexer_url;
        Ok(match &self.online_data.as_ref().unwrap().indexer {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(_) => AnyResolver::Electrum(Box::new(
                ElectrumResolver::new(indexer_url).map_err(|e| Error::InvalidIndexer {
                    details: e.to_string(),
                })?,
            )),
            #[cfg(feature = "esplora")]
            Indexer::Esplora(_) => AnyResolver::Esplora(Box::new(
                EsploraResolver::new(indexer_url).map_err(|e| Error::InvalidIndexer {
                    details: e.to_string(),
                })?,
            )),
        })
    }

    pub(crate) fn transfers_dir(&self) -> PathBuf {
        self.wallet_dir.join(TRANSFER_DIR)
    }

    fn _check_genesis_hash(
        &self,
        bitcoin_network: &BitcoinNetwork,
        indexer: &Indexer,
    ) -> Result<(), Error> {
        let expected = get_genesis_hash(bitcoin_network);
        let block_hash = indexer.block_hash(0)?;
        if expected != block_hash {
            return Err(Error::InvalidIndexer {
                details: s!("indexer is for a network different from the wallet's one"),
            });
        }

        Ok(())
    }

    fn _check_fee_rate(&self, fee_rate: f32) -> Result<(), Error> {
        #[cfg(test)]
        if skip_check_fee_rate() {
            println!("skipping fee rate check");
            return Ok(());
        };
        if fee_rate < MIN_FEE_RATE {
            return Err(Error::InvalidFeeRate {
                details: format!("value under minimum {MIN_FEE_RATE}"),
            });
        }
        if fee_rate > MAX_FEE_RATE {
            return Err(Error::InvalidFeeRate {
                details: format!("value above maximum {MAX_FEE_RATE}"),
            });
        }
        Ok(())
    }

    pub(crate) fn sync_wallet<D>(&self, wallet: &BdkWallet<D>) -> Result<(), Error>
    where
        D: BatchDatabase,
    {
        self._sync_wallet_with_blockchain(wallet, self.bdk_blockchain()?)?;
        Ok(())
    }

    fn _sync_wallet_with_blockchain<D>(
        &self,
        wallet: &BdkWallet<D>,
        bdk_blockchain: &AnyBlockchain,
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

    fn _sync_db_txos_with_blockchain(&self, bdk_blockchain: &AnyBlockchain) -> Result<(), Error> {
        debug!(self.logger, "Syncing TXOs...");
        self._sync_wallet_with_blockchain(&self.bdk_wallet, bdk_blockchain)?;

        let db_txos = self.database.iter_txos()?;

        let db_outpoints: Vec<String> = db_txos
            .clone()
            .into_iter()
            .filter(|t| !t.spent)
            .map(|u| u.outpoint().to_string())
            .collect();
        let bdk_utxos: Vec<LocalUtxo> = self
            .bdk_wallet
            .list_unspent()
            .map_err(InternalError::from)?;
        let external_bdk_utxos: Vec<LocalUtxo> = bdk_utxos
            .into_iter()
            .filter(|u| u.keychain == KeychainKind::External)
            .collect();

        let new_utxos: Vec<LocalUtxo> = external_bdk_utxos
            .clone()
            .into_iter()
            .filter(|u| !db_outpoints.contains(&u.outpoint.to_string()))
            .collect();

        let pending_witness_scripts: Vec<String> = self
            .database
            .iter_pending_witness_scripts()?
            .into_iter()
            .map(|s| s.script)
            .collect();

        for new_utxo in new_utxos.iter().cloned() {
            let new_db_utxo: DbTxoActMod = new_utxo.clone().into();
            if !pending_witness_scripts.is_empty() {
                let pending_witness_script = new_utxo.txout.script_pubkey.to_hex_string();
                if pending_witness_scripts.contains(&pending_witness_script) {
                    self.database
                        .set_pending_witness_outpoint(DbPendingWitnessOutpointActMod {
                            txid: new_db_utxo.txid.clone(),
                            vout: new_db_utxo.vout.clone(),
                            ..Default::default()
                        })?;
                    self.database
                        .del_pending_witness_script(pending_witness_script)?;
                }
            }
            self.database.set_txo(new_db_utxo)?;
        }

        if external_bdk_utxos.len() - new_utxos.len() > 0 {
            let inexistent_db_utxos: Vec<DbTxo> =
                db_txos.into_iter().filter(|t| !t.exists).collect();
            for inexistent_db_utxo in inexistent_db_utxos {
                if external_bdk_utxos
                    .iter()
                    .any(|u| Outpoint::from(u.outpoint) == inexistent_db_utxo.outpoint())
                {
                    let mut db_txo: DbTxoActMod = inexistent_db_utxo.into();
                    db_txo.exists = ActiveValue::Set(true);
                    self.database.update_txo(db_txo)?;
                }
            }
        }

        Ok(())
    }

    fn _sync_db_txos(&self) -> Result<(), Error> {
        self._sync_db_txos_with_blockchain(self.bdk_blockchain()?)?;
        Ok(())
    }

    fn _broadcast_psbt(&self, signed_psbt: BdkPsbt) -> Result<BdkTransaction, Error> {
        let tx = self.broadcast_tx(signed_psbt.extract_tx())?;

        let internal_unspents_outpoints: Vec<(String, u32)> = self
            .internal_unspents()?
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
                .get_txo(&Outpoint { txid, vout })?
                .expect("outpoint should be in the DB")
                .into();
            db_txo.spent = ActiveValue::Set(true);
            self.database.update_txo(db_txo)?;
        }

        self._sync_db_txos()?;

        Ok(tx)
    }

    pub(crate) fn check_online(&self, online: Online) -> Result<(), Error> {
        if let Some(online_data) = &self.online_data {
            if online_data.id != online.id || online_data.indexer_url != online.indexer_url {
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

    pub(crate) fn handle_expired_transfers(&self, db_data: &mut DbData) -> Result<(), Error> {
        self._sync_db_txos()?;
        let now = now().unix_timestamp();
        let expired_transfers: Vec<DbBatchTransfer> = db_data
            .batch_transfers
            .clone()
            .into_iter()
            .filter(|t| t.waiting_counterparty() && t.expiration.unwrap_or(now) < now)
            .collect();
        for transfer in expired_transfers.iter() {
            let updated_batch_transfer = self._refresh_transfer(transfer, db_data, &[])?;
            if updated_batch_transfer.is_none() {
                let mut updated_batch_transfer: DbBatchTransferActMod = transfer.clone().into();
                updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
                self.database
                    .update_batch_transfer(&mut updated_batch_transfer)?;
            }
        }
        Ok(())
    }

    fn _get_asset_iface(
        &self,
        contract_id: ContractId,
        runtime: &RgbRuntime,
    ) -> Result<AssetIface, Error> {
        let genesis = runtime.genesis(contract_id)?;
        let schema_id = genesis.schema_id.to_string();
        Ok(match &schema_id[..] {
            SCHEMA_ID_NIA => AssetIface::RGB20,
            SCHEMA_ID_UDA => AssetIface::RGB21,
            SCHEMA_ID_CFA => AssetIface::RGB25,
            _ => return Err(Error::UnknownRgbSchema { schema_id }),
        })
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
            tx_builder.add_recipient(self.get_new_address().script_pubkey(), size as u64);
        }
        Ok(tx_builder.finish()?.0)
    }

    /// Create new UTXOs.
    ///
    /// This calls [`create_utxos_begin`](Wallet::create_utxos_begin), signs the resulting PSBT and
    /// finally calls [`create_utxos_end`](Wallet::create_utxos_end).
    ///
    /// A wallet with private keys is required.
    pub fn create_utxos(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: f32,
    ) -> Result<u8, Error> {
        info!(self.logger, "Creating UTXOs...");
        self._check_xprv()?;

        let unsigned_psbt = self.create_utxos_begin(online.clone(), up_to, num, size, fee_rate)?;

        let psbt = self.sign_psbt(unsigned_psbt, None)?;

        self.create_utxos_end(online, psbt)
    }

    /// Prepare the PSBT to create new UTXOs to hold RGB allocations with the provided `fee_rate`
    /// (in sat/vB).
    ///
    /// If `up_to` is false, just create the required UTXOs, if it is true, create as many UTXOs as
    /// needed to reach the requested number or return an error if none need to be created.
    ///
    /// Providing the optional `num` parameter requests that many UTXOs, if it's not specified the
    /// default number (5<!--UTXO_NUM-->) is used.
    ///
    /// Providing the optional `size` parameter requests that UTXOs be created of that size (in
    /// sats), if it's not specified the default one (1000<!--UTXO_SIZE-->) is used.
    ///
    /// If not enough bitcoin funds are available to create the requested (or default) number of
    /// UTXOs, the number is decremented by one until it is possible to complete the operation. If
    /// the number reaches zero, an error is returned.
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`create_utxos_end`](Wallet::create_utxos_end) function.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed.
    pub fn create_utxos_begin(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: f32,
    ) -> Result<String, Error> {
        info!(self.logger, "Creating UTXOs (begin)...");
        self.check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        self._sync_db_txos()?;

        let unspent_txos = self.database.get_unspent_txos(vec![])?;
        let unspents = self
            .database
            .get_rgb_allocations(unspent_txos, None, None, None)?;

        let mut utxos_to_create = num.unwrap_or(UTXO_NUM);
        if up_to {
            let allocatable = self
                .get_available_allocations(unspents, vec![], None)?
                .len() as u8;
            if allocatable >= utxos_to_create {
                return Err(Error::AllocationsAlreadyAvailable);
            }
            utxos_to_create -= allocatable
        }
        debug!(self.logger, "Will try to create {} UTXOs", utxos_to_create);

        let inputs: Vec<BdkOutPoint> = self.internal_unspents()?.map(|u| u.outpoint).collect();
        let inputs: &[BdkOutPoint] = &inputs;
        let usable_btc_amount = self.get_uncolorable_btc_sum()?;
        let utxo_size = size.unwrap_or(UTXO_SIZE);
        if utxo_size == 0 {
            return Err(Error::InvalidAmountZero);
        }
        let possible_utxos = usable_btc_amount / utxo_size as u64;
        let max_possible_utxos: u8 = if possible_utxos > u8::MAX as u64 {
            u8::MAX
        } else {
            possible_utxos as u8
        };
        let mut btc_needed: u64 = (utxo_size as u64 * utxos_to_create as u64) + 1000;
        let mut btc_available: u64 = 0;
        let mut num_try_creating = min(utxos_to_create, max_possible_utxos);
        while num_try_creating > 0 {
            match self._create_split_tx(inputs, num_try_creating, utxo_size, fee_rate) {
                Ok(_v) => break,
                Err(e) => {
                    (btc_needed, btc_available) = match e {
                        bdk::Error::InsufficientFunds { needed, available } => (needed, available),
                        bdk::Error::OutputBelowDustLimit(_) => {
                            return Err(Error::OutputBelowDustLimit)
                        }
                        _ => {
                            return Err(Error::Internal {
                                details: e.to_string(),
                            })
                        }
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
            let psbt = self
                ._create_split_tx(inputs, num_try_creating, utxo_size, fee_rate)
                .map_err(InternalError::from)?
                .to_string();
            info!(self.logger, "Create UTXOs (begin) completed");
            Ok(psbt)
        }
    }

    /// Broadcast the provided PSBT to create new UTXOs.
    ///
    /// The provided PSBT, prepared with the [`create_utxos_begin`](Wallet::create_utxos_begin)
    /// function, needs to have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns the number of created UTXOs.
    pub fn create_utxos_end(&self, online: Online, signed_psbt: String) -> Result<u8, Error> {
        info!(self.logger, "Creating UTXOs (end)...");
        self.check_online(online)?;

        let signed_psbt = BdkPsbt::from_str(&signed_psbt)?;
        let tx = self._broadcast_psbt(signed_psbt)?;

        self.database
            .set_wallet_transaction(DbWalletTransactionActMod {
                txid: ActiveValue::Set(tx.txid().to_string()),
                r#type: ActiveValue::Set(WalletTransactionType::CreateUtxos),
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

        info!(self.logger, "Create UTXOs (end) completed");
        Ok(num_utxos_created)
    }

    /// Send bitcoin funds to the provided address.
    ///
    /// This calls [`drain_to_begin`](Wallet::drain_to_begin), signs the resulting PSBT and finally
    /// calls [`drain_to_end`](Wallet::drain_to_end).
    ///
    /// A wallet with private keys is required.
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

        let psbt = self.sign_psbt(unsigned_psbt, None)?;

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

    /// Prepare the PSBT to send bitcoin funds not in use for RGB allocations, or all funds if
    /// `destroy_assets` is set to true, to the provided Bitcoin `address` with the provided
    /// `fee_rate` (in sat/vB).
    ///
    /// <div class="warning">Warning: setting <code>destroy_assets</code> to true is dangerous,
    /// only do this if you know what you're doing! After destroying assets the wallet's RGB state
    /// could be compromised and therefore the wallet should not be used anymore.</div>
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`drain_to_end`](Wallet::drain_to_end) function.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed.
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
        self.check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        self._sync_db_txos()?;

        let address = BdkAddress::from_str(&address).map(|x| x.payload.script_pubkey())?;

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
                bdk::Error::OutputBelowDustLimit(_) => Error::OutputBelowDustLimit,
                _ => Error::Internal {
                    details: e.to_string(),
                },
            })?
            .0
            .to_string();

        info!(self.logger, "Drain (begin) completed");
        Ok(psbt)
    }

    /// Broadcast the provided PSBT to send bitcoin funds.
    ///
    /// The provided PSBT, prepared with the [`drain_to_begin`](Wallet::drain_to_begin) function,
    /// needs to have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns the TXID of the transaction that's been broadcast.
    pub fn drain_to_end(&self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger, "Draining (end)...");
        self.check_online(online)?;

        let signed_psbt = BdkPsbt::from_str(&signed_psbt)?;
        let tx = self._broadcast_psbt(signed_psbt)?;

        self.database
            .set_wallet_transaction(DbWalletTransactionActMod {
                txid: ActiveValue::Set(tx.txid().to_string()),
                r#type: ActiveValue::Set(WalletTransactionType::Drain),
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
        &self,
        batch_transfer: &DbBatchTransfer,
        throw_err: bool,
        db_data: &mut DbData,
    ) -> Result<(), Error> {
        let updated_batch_transfer = match self._refresh_transfer(batch_transfer, db_data, &[]) {
            Err(Error::MinFeeNotMet { txid: _ }) => Ok(None),
            Err(e) => Err(e),
            Ok(v) => Ok(v),
        }?;
        // fail transfer if the status didn't change after a refresh
        if updated_batch_transfer.is_none() {
            self._fail_batch_transfer(batch_transfer)?;
        } else if throw_err {
            return Err(Error::CannotFailBatchTransfer);
        }

        Ok(())
    }

    /// Set the status for eligible transfers to [`TransferStatus::Failed`] and return true if any
    /// transfer has changed.
    ///
    /// An optional `batch_transfer_idx` can be provided to operate on a single batch transfer.
    ///
    /// If a `batch_transfer_idx` is provided and `no_asset_only` is true, transfers with an
    /// associated asset ID will not be failed and instead return an error.
    ///
    /// If no `batch_transfer_idx` is provided, only expired transfers will be failed,
    /// and if `no_asset_only` is true transfers with an associated asset ID will be skipped.
    ///
    /// Transfers are eligible if they remain in status [`TransferStatus::WaitingCounterparty`]
    /// after a `refresh` has been performed.
    pub fn fail_transfers(
        &self,
        online: Online,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        info!(
            self.logger,
            "Failing batch transfer with idx {:?}...", batch_transfer_idx
        );
        self.check_online(online)?;

        let mut db_data = self.database.get_db_data(false)?;
        let mut transfers_changed = false;

        if let Some(batch_transfer_idx) = batch_transfer_idx {
            let batch_transfer = &self
                .database
                .get_batch_transfer_or_fail(batch_transfer_idx, &db_data.batch_transfers)?;

            if !batch_transfer.waiting_counterparty() {
                return Err(Error::CannotFailBatchTransfer);
            }

            if no_asset_only {
                let asset_transfers =
                    batch_transfer.get_asset_transfers(&db_data.asset_transfers)?;
                let connected_assets = asset_transfers.iter().any(|t| t.asset_id.is_some());
                if connected_assets {
                    return Err(Error::CannotFailBatchTransfer);
                }
            }

            transfers_changed = true;
            self._try_fail_batch_transfer(batch_transfer, true, &mut db_data)?
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

    /// Return the [`BtcBalance`] of the internal Bitcoin wallets.
    pub fn get_btc_balance(&self, online: Online) -> Result<BtcBalance, Error> {
        info!(self.logger, "Getting BTC balance...");
        self.check_online(online)?;

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

        self.sync_wallet(&bdk_wallet_keychain_1)?;
        self.sync_wallet(&bdk_wallet_keychain_9)?;

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

    fn _get_uda_attachments(&self, contract: ContractIface) -> Result<Vec<Attachment>, Error> {
        let mut uda_attachments = vec![];
        if let Ok(tokens) = contract.global("tokens") {
            if tokens.is_empty() {
                return Ok(uda_attachments);
            }
            let val = &tokens[0];

            if let Some(attachment) = val
                .unwrap_struct("media")
                .unwrap_option()
                .map(Attachment::from_strict_val_unchecked)
            {
                uda_attachments.push(attachment)
            }

            match val.unwrap_struct("attachments") {
                StrictVal::Map(fields) => {
                    for (_, attachment_struct) in fields {
                        let attachment = Attachment::from_strict_val_unchecked(attachment_struct);
                        uda_attachments.push(attachment)
                    }
                }
                _ => return Err(InternalError::Unexpected.into()),
            };
        }
        Ok(uda_attachments)
    }

    fn _check_consistency(
        &self,
        bdk_blockchain: &AnyBlockchain,
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
            .filter(|t| !t.spent && t.exists)
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

        let medias = self.database.iter_media()?;
        let media_dir = self.media_dir();
        for media in medias {
            if !media_dir.join(media.digest).exists() {
                return Err(Error::Inconsistency {
                    details: s!("DB media do not match with the ones stored in media directory"),
                });
            }
        }

        info!(self.logger, "Consistency check completed");
        Ok(())
    }

    fn _connect_to_indexer(
        &self,
        indexer_url: String,
    ) -> Result<Option<(Indexer, AnyBlockchainConfig)>, Error> {
        #[cfg(feature = "electrum")]
        {
            let electrum_config = ConfigBuilder::new().timeout(Some(INDEXER_TIMEOUT)).build();
            if let Ok(client) = ElectrumClient::from_config(&indexer_url, electrum_config) {
                let electrum_config = ElectrumBlockchainConfig {
                    url: indexer_url,
                    socks5: None,
                    retry: 3,
                    timeout: Some(INDEXER_TIMEOUT),
                    stop_gap: INDEXER_STOP_GAP,
                    validate_domain: true,
                };
                let indexer = Indexer::Electrum(Box::new(client));
                let config = AnyBlockchainConfig::Electrum(electrum_config);
                return Ok(Some((indexer, config)));
            }
        }
        if cfg!(feature = "esplora") {
            #[cfg(feature = "esplora")]
            {
                let esplora_config = EsploraBlockchainConfig {
                    base_url: indexer_url,
                    proxy: None,
                    concurrency: None,
                    timeout: Some(INDEXER_TIMEOUT as u64),
                    stop_gap: INDEXER_STOP_GAP,
                };
                let esplora_client = EsploraClient::from_config(&esplora_config).unwrap();
                let indexer = Indexer::Esplora(Box::new(esplora_client));
                let config = AnyBlockchainConfig::Esplora(esplora_config);
                return Ok(Some((indexer, config)));
            }
        }
        Ok(None)
    }

    fn _go_online(&self, indexer_url: String) -> Result<(Online, OnlineData), Error> {
        let online_id = now().unix_timestamp_nanos() as u64;
        let online = Online {
            id: online_id,
            indexer_url: indexer_url.clone(),
        };

        // detect indexer type
        let indexer_info = self._connect_to_indexer(indexer_url.clone())?;
        let mut invalid_indexer = true;
        if let Some((ref indexer, _)) = indexer_info {
            invalid_indexer = indexer.block_hash(0).is_err();
        }
        if invalid_indexer {
            return Err(Error::InvalidIndexer {
                details: s!("not a valid electrum nor esplora server"),
            });
        }
        let (indexer, indexer_config) = indexer_info.unwrap();

        // check the indexer server is for the correct network
        let bitcoin_network = self.bitcoin_network();
        self._check_genesis_hash(&bitcoin_network, &indexer)?;

        #[cfg(feature = "electrum")]
        if matches!(indexer, Indexer::Electrum(_)) {
            // check the electrum server has the required functionality (verbose transactions)
            indexer
                .get_tx_confirmations(&get_valid_txid_for_network(&bitcoin_network))
                .map_err(|_| Error::InvalidElectrum {
                    details: s!("verbose transactions are currently unsupported"),
                })?;
        }

        // BDK setup
        let bdk_blockchain =
            AnyBlockchain::from_config(&indexer_config).map_err(|e| Error::InvalidIndexer {
                details: e.to_string(),
            })?;

        let online_data = OnlineData {
            id: online.id,
            bdk_blockchain,
            indexer_url,
            indexer,
        };

        Ok((online, online_data))
    }

    /// Return the existing or freshly generated set of wallet [`Online`] data.
    ///
    /// Setting `skip_consistency_check` to false runs a check on UTXOs (BDK vs rgb-lib DB) and
    /// assets (RGB vs rgb-lib DB) to try and detect possible inconsistencies in the wallet.
    /// Setting `skip_consistency_check` to true bypasses the check and allows operating an
    /// inconsistent wallet.
    ///
    /// <div class="warning">Warning: setting `skip_consistency_check` to true is dangerous, only
    /// do this if you know what you're doing!</div>
    pub fn go_online(
        &mut self,
        skip_consistency_check: bool,
        indexer_url: String,
    ) -> Result<Online, Error> {
        info!(self.logger, "Going online...");

        let online = if let Some(online_data) = &self.online_data {
            let online = Online {
                id: online_data.id,
                indexer_url,
            };
            if online_data.indexer_url != online.indexer_url {
                let (online, online_data) = self._go_online(online.indexer_url)?;
                self.online_data = Some(online_data);
                info!(self.logger, "Went online with new indexer URL");
                online
            } else {
                self.check_online(online.clone())?;
                online
            }
        } else {
            let (online, online_data) = self._go_online(indexer_url)?;
            self.online_data = Some(online_data);
            online
        };

        if !skip_consistency_check {
            let runtime = self.rgb_runtime()?;
            self._check_consistency(self.bdk_blockchain()?, &runtime)?;
        }

        info!(self.logger, "Go online completed");
        Ok(online)
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
            Ok(match acc.checked_add(*x) {
                None => return Err(Error::TooHighIssuanceAmounts),
                Some(sum) => sum,
            })
        })
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
        let digest = file_hash.to_byte_array();
        let mime = tree_magic::from_filepath(original_file_path.as_ref());
        let media_ty: &'static str = Box::leak(mime.clone().into_boxed_str());
        let media_type = MediaType::with(media_ty);
        let file_path = self
            .media_dir()
            .join(hex::encode(digest))
            .to_string_lossy()
            .to_string();
        Ok((
            Attachment {
                ty: media_type,
                digest,
            },
            Media { mime, file_path },
        ))
    }

    pub(crate) fn copy_media_and_save<P: AsRef<Path>>(
        &self,
        original_file_path: P,
        media: &Media,
    ) -> Result<i32, Error> {
        fs::copy(original_file_path, media.clone().file_path)?;
        self.get_or_insert_media(media.get_digest(), media.mime.clone())
    }

    pub(crate) fn new_asset_terms(
        &self,
        text: RicardianContract,
        media: Option<Attachment>,
    ) -> AssetTerms {
        AssetTerms { text, media }
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
        online: Online,
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
        self.check_online(online)?;

        let settled = self._get_total_issue_amount(&amounts)?;

        let mut db_data = self.database.get_db_data(false)?;
        self.handle_expired_transfers(&mut db_data)?;

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
            Rgb20::iface(),
            NonInflatableAsset::schema(),
            NonInflatableAsset::issue_impl(),
            self.testnet(),
        )
        .unwrap()
        .add_global_state("spec", spec.clone())
        .expect("invalid spec")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_global_state("issuedSupply", Amount::from(settled))
        .expect("invalid issuedSupply");

        let mut issue_utxos: HashMap<DbTxo, u64> = HashMap::new();
        for amount in &amounts {
            let exclude_outpoints: Vec<Outpoint> =
                issue_utxos.keys().map(|txo| txo.outpoint()).collect();
            let utxo = self.get_utxo(exclude_outpoints, Some(unspents.clone()), false)?;
            issue_utxos.insert(utxo.clone(), *amount);

            let blind_seal =
                BlindSeal::opret_first_rand(BpTxid::from_str(&utxo.txid).unwrap(), utxo.vout);
            let genesis_seal = GenesisSeal::from(blind_seal);
            let seal: XChain<BlindSeal<BpTxid>> = XChain::with(Layer1::Bitcoin, genesis_seal);

            builder = builder
                .add_fungible_state("assetOwner", BuilderSeal::from(seal), *amount)
                .expect("invalid global state data");
        }
        debug!(self.logger, "Issuing on UTXOs: {issue_utxos:?}");

        let contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = contract.contract_id().to_string();
        let validated_contract = contract
            .validate(&mut self.blockchain_resolver()?, self.testnet())
            .expect("internal error: failed validating self-issued contract");
        runtime
            .import_contract(validated_contract, &mut self.blockchain_resolver()?)
            .expect("failure importing issued contract");

        let asset = self.add_asset_to_db(
            asset_id.clone(),
            &AssetSchema::Nia,
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
            amount: ActiveValue::Set(settled.to_string()),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        for (utxo, amount) in issue_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                r#type: ActiveValue::Set(ColoringType::Issue),
                amount: ActiveValue::Set(amount.to_string()),
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
        online: Online,
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
        self.check_online(online)?;

        if attachments_file_paths.len() > MAX_ATTACHMENTS {
            return Err(Error::InvalidAttachments {
                details: format!("no more than {MAX_ATTACHMENTS} attachments are supported"),
            });
        }

        let settled = 1;

        let mut db_data = self.database.get_db_data(false)?;
        self.handle_expired_transfers(&mut db_data)?;

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
        let text = RicardianContract::default();
        let terms = AssetTerms { text, media: None };

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

        let issue_utxo = self.get_utxo(vec![], Some(unspents.clone()), false)?;
        debug!(self.logger, "Issuing on UTXO: {issue_utxo:?}");

        let blind_seal = BlindSeal::opret_first_rand(
            BpTxid::from_str(&issue_utxo.txid).unwrap(),
            issue_utxo.vout,
        );
        let genesis_seal = GenesisSeal::from(blind_seal);
        let seal: XChain<BlindSeal<BpTxid>> = XChain::with(Layer1::Bitcoin, genesis_seal);

        let index = TokenIndex::from_inner(UDA_FIXED_INDEX);

        let fraction = OwnedFraction::from_inner(1);
        let allocation = Allocation::with(index, fraction);

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

        let token = TokenLight {
            index: UDA_FIXED_INDEX,
            media: media_data.as_ref().map(|(_, media)| media.clone()),
            attachments: media_attachments.clone(),
            ..Default::default()
        };

        let mut runtime = self.rgb_runtime()?;
        let builder =
            ContractBuilder::with(Rgb21::iface(), uda_schema(), uda_rgb21(), self.testnet())
                .unwrap()
                .add_global_state("spec", spec)
                .expect("invalid spec")
                .add_global_state("terms", terms)
                .expect("invalid terms")
                .add_data("assetOwner", BuilderSeal::from(seal), allocation)
                .expect("invalid global state data")
                .add_global_state("tokens", token_data)
                .expect("invalid tokens");

        let contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = contract.contract_id().to_string();
        let validated_contract = contract
            .clone()
            .validate(&mut self.blockchain_resolver()?, self.testnet())
            .expect("internal error: failed validating self-issued contract");
        runtime
            .import_contract(validated_contract, &mut self.blockchain_resolver()?)
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
            &AssetSchema::Uda,
            Some(created_at),
            details.clone(),
            settled as u64,
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
            amount: ActiveValue::Set(settled.to_string()),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        let db_coloring = DbColoringActMod {
            txo_idx: ActiveValue::Set(issue_utxo.idx),
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            r#type: ActiveValue::Set(ColoringType::Issue),
            amount: ActiveValue::Set(settled.to_string()),
            ..Default::default()
        };
        self.database.set_coloring(db_coloring)?;
        let db_token = DbTokenActMod {
            asset_idx: ActiveValue::Set(asset.idx),
            index: ActiveValue::Set(UDA_FIXED_INDEX),
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
        online: Online,
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
        self.check_online(online)?;

        let settled = self._get_total_issue_amount(&amounts)?;

        let mut db_data = self.database.get_db_data(false)?;
        self.handle_expired_transfers(&mut db_data)?;

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
        let text = RicardianContract::default();
        let media_data = if let Some(file_path) = &file_path {
            Some(self._file_details(file_path)?)
        } else {
            None
        };
        let terms = AssetTerms {
            text,
            media: media_data
                .as_ref()
                .map(|(attachment, _)| attachment.clone()),
        };
        let precision_state = self._check_precision(precision)?;
        let name_state = self._check_name(name.clone())?;

        let mut runtime = self.rgb_runtime()?;
        let mut builder =
            ContractBuilder::with(Rgb25::iface(), cfa_schema(), cfa_rgb25(), self.testnet())
                .unwrap()
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
        for amount in &amounts {
            let exclude_outpoints: Vec<Outpoint> =
                issue_utxos.keys().map(|txo| txo.outpoint()).collect();
            let utxo = self.get_utxo(exclude_outpoints, Some(unspents.clone()), false)?;
            issue_utxos.insert(utxo.clone(), *amount);

            let blind_seal =
                BlindSeal::opret_first_rand(BpTxid::from_str(&utxo.txid).unwrap(), utxo.vout);
            let genesis_seal = GenesisSeal::from(blind_seal);
            let seal: XChain<BlindSeal<BpTxid>> = XChain::with(Layer1::Bitcoin, genesis_seal);

            builder = builder
                .add_fungible_state("assetOwner", BuilderSeal::from(seal), *amount)
                .expect("invalid global state data");
        }
        debug!(self.logger, "Issuing on UTXOs: {issue_utxos:?}");

        let contract = builder.issue_contract().expect("failure issuing contract");
        let asset_id = contract.contract_id().to_string();
        let validated_contract = contract
            .validate(&mut self.blockchain_resolver()?, self.testnet())
            .expect("internal error: failed validating self-issued contract");
        runtime
            .import_contract(validated_contract, &mut self.blockchain_resolver()?)
            .expect("failure importing issued contract");

        let media_idx = if let Some(file_path) = file_path {
            let (_, media) = media_data.unwrap();
            Some(self.copy_media_and_save(file_path, &media)?)
        } else {
            None
        };

        let asset = self.add_asset_to_db(
            asset_id.clone(),
            &AssetSchema::Cfa,
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
            amount: ActiveValue::Set(settled.to_string()),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        self.database.set_transfer(transfer)?;
        for (utxo, amount) in issue_utxos {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo.idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                r#type: ActiveValue::Set(ColoringType::Issue),
                amount: ActiveValue::Set(amount.to_string()),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }

        let asset = AssetCFA::get_asset_details(self, &asset, None, None, None, None, None, None)?;

        self.update_backup_info(false)?;

        info!(self.logger, "Issue asset CFA completed");
        Ok(asset)
    }

    fn _get_asset_medias(
        &self,
        media_idx: Option<i32>,
        token: Option<TokenLight>,
    ) -> Result<Vec<Media>, Error> {
        let mut asset_medias = vec![];
        if let Some(token) = token {
            if let Some(token_media) = token.media {
                asset_medias.push(token_media);
            }
            for (_, attachment_media) in token.attachments {
                asset_medias.push(attachment_media);
            }
        } else if let Some(media_idx) = media_idx {
            let db_media = self.database.get_media(media_idx)?.unwrap();
            asset_medias.push(Media::from_db_media(&db_media, self.media_dir()))
        }
        Ok(asset_medias)
    }

    fn _get_signed_psbt(&self, transfer_dir: PathBuf) -> Result<BdkPsbt, Error> {
        let psbt_file = transfer_dir.join(SIGNED_PSBT_FILE);
        let psbt_str = fs::read_to_string(psbt_file)?;
        Ok(BdkPsbt::from_str(&psbt_str)?)
    }

    fn _fail_batch_transfer_if_no_endpoints(
        &self,
        batch_transfer: &DbBatchTransfer,
        transfer_transport_endpoints_data: &[(DbTransferTransportEndpoint, DbTransportEndpoint)],
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
        debug!(
            self.logger,
            "Refusing invalid consignment for {recipient_id}"
        );
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

    pub(crate) fn get_consignment(
        &self,
        proxy_url: &str,
        recipient_id: String,
    ) -> Result<GetConsignmentResponse, Error> {
        let consignment_res = self
            .rest_client
            .clone()
            .get_consignment(proxy_url, recipient_id);

        if consignment_res.is_err() || consignment_res.as_ref().unwrap().result.as_ref().is_none() {
            debug!(
                self.logger,
                "Consignment GET response error: {:?}", &consignment_res
            );
            return Err(Error::NoConsignment);
        }

        let consignment_res = consignment_res.unwrap().result.unwrap();
        #[cfg(test)]
        debug!(
            self.logger,
            "Consignment GET response: {:?}", consignment_res
        );

        Ok(consignment_res)
    }

    pub(crate) fn extract_received_amount(
        &self,
        consignment: &RgbTransfer,
        txid: String,
        vout: Option<u32>,
        known_concealed: Option<XChain<SecretSeal>>,
    ) -> (u64, bool) {
        let mut amount = 0;
        let mut not_opret = false;
        if let Some(anchored_bundle) = consignment
            .clone()
            .bundles
            .into_iter()
            .find(|ab| ab.anchor.witness_id_unchecked().to_string() == format!("bc:{txid}"))
        {
            'outer: for transition in anchored_bundle.bundle.known_transitions.values() {
                for assignment in transition.assignments.values() {
                    for fungible_assignment in assignment.as_fungible() {
                        if let Assign::ConfidentialSeal { seal, state, .. } = fungible_assignment {
                            if Some(*seal) == known_concealed {
                                amount = state.value.as_u64();
                                break 'outer;
                            }
                        };
                        if let Assign::Revealed { seal, state, .. } = fungible_assignment {
                            let seal = seal.as_reduced_unsafe();
                            if seal.txid == TxPtr::WitnessTx && Some(seal.vout.into_u32()) == vout {
                                if seal.method != CloseMethod::OpretFirst {
                                    not_opret = true;
                                    break 'outer;
                                }
                                amount = state.value.as_u64();
                                break 'outer;
                            }
                        };
                    }
                    for structured_assignment in assignment.as_structured() {
                        if let Assign::ConfidentialSeal { seal, .. } = structured_assignment {
                            if Some(*seal) == known_concealed {
                                amount = 1;
                                break 'outer;
                            }
                        }
                        if let Assign::Revealed { seal, .. } = structured_assignment {
                            let seal = seal.as_reduced_unsafe();
                            if seal.txid == TxPtr::WitnessTx && Some(seal.vout.into_u32()) == vout {
                                if seal.method != CloseMethod::OpretFirst {
                                    not_opret = true;
                                    break 'outer;
                                }
                                amount = 1;
                                break 'outer;
                            }
                        };
                    }
                }
            }
        }

        (amount, not_opret)
    }

    fn _wait_consignment(
        &self,
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
        debug!(self.logger, "Recipient ID: {recipient_id}");

        // check if a consignment has been posted
        let tte_data = self
            .database
            .get_transfer_transport_endpoints_data(transfer.idx)?;
        if self._fail_batch_transfer_if_no_endpoints(batch_transfer, &tte_data)? {
            return Ok(None);
        }
        let mut proxy_res = None;
        for (transfer_transport_endpoint, transport_endpoint) in tte_data {
            let result =
                match self.get_consignment(&transport_endpoint.endpoint, recipient_id.clone()) {
                    Err(Error::NoConsignment) => {
                        info!(
                            self.logger,
                            "Skipping transport endpoint: {:?}", &transport_endpoint
                        );
                        continue;
                    }
                    Err(e) => return Err(e),
                    Ok(r) => r,
                };

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

        let (consignment, proxy_url, txid, vout) = if let Some(res) = proxy_res {
            (res.0, res.1, res.2, res.3)
        } else {
            return Ok(None);
        };

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();

        // write consignment
        let transfer_dir = self.transfers_dir().join(&recipient_id);
        let consignment_path = transfer_dir.join(CONSIGNMENT_RCV_FILE);
        fs::create_dir_all(transfer_dir)?;
        let consignment_bytes = general_purpose::STANDARD
            .decode(consignment)
            .map_err(InternalError::from)?;
        fs::write(consignment_path.clone(), consignment_bytes).expect("Unable to write file");

        let mut runtime = self.rgb_runtime()?;
        let consignment = RgbTransfer::load_file(consignment_path).map_err(InternalError::from)?;
        let contract_id = consignment.contract_id();
        let asset_id = contract_id.to_string();

        // validate consignment
        if let Some(aid) = asset_transfer.asset_id.clone() {
            // check if asset transfer is connected to the asset we are actually receiving
            if aid != asset_id {
                error!(
                    self.logger,
                    "Received a different asset than the expected one"
                );
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
            .validate(&mut self.blockchain_resolver()?, self.testnet())
        {
            Ok(consignment) => consignment,
            Err(consignment) => consignment,
        };
        let validation_status = validated_consignment.into_validation_status().unwrap();
        let validity = validation_status.validity();
        debug!(self.logger, "Consignment validity: {:?}", validity);

        if ![
            Validity::Valid,
            Validity::UnminedTerminals,
            Validity::UnresolvedTransactions,
        ]
        .contains(&validity)
        {
            error!(self.logger, "Consignment has an invalid status: {validity}");
            return self._refuse_consignment(proxy_url, recipient_id, &mut updated_batch_transfer);
        }

        let schema_id = consignment.schema_id().to_string();
        let asset_schema = AssetSchema::from_schema_id(schema_id)?;

        // add asset info to transfer if missing
        if asset_transfer.asset_id.is_none() {
            // check if asset is known
            let exists_check = self.database.check_asset_exists(asset_id.clone());
            if exists_check.is_err() {
                // unknown asset
                debug!(self.logger, "Registering contract...");
                let mut minimal_contract = consignment.clone().into_contract();
                minimal_contract.bundles = none!();
                minimal_contract.terminals = none!();
                let minimal_contract_validated = match minimal_contract
                    .clone()
                    .validate(&mut self.blockchain_resolver()?, self.testnet())
                {
                    Ok(consignment) => consignment,
                    Err(consignment) => consignment,
                };
                runtime
                    .import_contract(minimal_contract_validated, &mut self.blockchain_resolver()?)
                    .expect("failure importing received contract");
                debug!(self.logger, "Contract registered");

                let contract_iface =
                    self.get_contract_iface(&mut runtime, &asset_schema, contract_id)?;

                let mut attachments = vec![];
                match asset_schema {
                    AssetSchema::Nia => {
                        let iface_nia = Rgb20::from(contract_iface);
                        if let Some(attachment) = iface_nia.contract_terms().media {
                            attachments.push(attachment)
                        }
                    }
                    AssetSchema::Uda => {
                        let uda_attachments = self._get_uda_attachments(contract_iface)?;
                        attachments.extend(uda_attachments)
                    }
                    AssetSchema::Cfa => {
                        let iface_cfa = Rgb25::from(contract_iface);
                        if let Some(attachment) = iface_cfa.contract_terms().media {
                            attachments.push(attachment)
                        }
                    }
                };
                for attachment in attachments {
                    let digest = hex::encode(attachment.digest);
                    let media_path = self.media_dir().join(&digest);
                    // download media only if file not already present
                    if !media_path.exists() {
                        let media_res = self
                            .rest_client
                            .clone()
                            .get_media(&proxy_url, digest.clone())?;
                        #[cfg(test)]
                        debug!(self.logger, "Media GET response: {:?}", media_res);
                        if let Some(media_res) = media_res.result {
                            let file_bytes = general_purpose::STANDARD
                                .decode(media_res)
                                .map_err(InternalError::from)?;
                            let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
                            let actual_digest = hex::encode(file_hash.to_byte_array());
                            if digest != actual_digest {
                                error!(
                                    self.logger,
                                    "Attached file has a different hash than the one in the contract"
                                );
                                return self._refuse_consignment(
                                    proxy_url,
                                    recipient_id,
                                    &mut updated_batch_transfer,
                                );
                            }
                            fs::write(&media_path, file_bytes)?;
                        } else {
                            error!(
                                self.logger,
                                "Cannot find the media file but the contract defines one"
                            );
                            return self._refuse_consignment(
                                proxy_url,
                                recipient_id,
                                &mut updated_batch_transfer,
                            );
                        }
                    }
                }

                drop(runtime);

                self.save_new_asset(&asset_schema, contract_id, Some(minimal_contract))?;
            }

            let mut updated_asset_transfer: DbAssetTransferActMod = asset_transfer.clone().into();
            updated_asset_transfer.asset_id = ActiveValue::Set(Some(asset_id.clone()));
            self.database
                .update_asset_transfer(&mut updated_asset_transfer)?;
        }

        let known_concealed = if transfer.recipient_type == Some(RecipientType::Blind) {
            let beneficiary = XChainNet::<Beneficiary>::from_str(&recipient_id)
                .expect("saved recipient ID is invalid");
            match beneficiary.into_inner() {
                Beneficiary::BlindedSeal(secret_seal) => {
                    Some(XChain::with(Layer1::Bitcoin, secret_seal))
                }
                _ => unreachable!("beneficiary is blinded"),
            }
        } else {
            None
        };

        let (amount, not_opret) =
            self.extract_received_amount(&consignment, txid.clone(), vout, known_concealed);

        if not_opret {
            error!(self.logger, "Found a non opret seal");
            return self._refuse_consignment(proxy_url, recipient_id, &mut updated_batch_transfer);
        }

        if amount == 0 {
            error!(
                self.logger,
                "Cannot find any receiving allocation with positive amount"
            );
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
                    c.asset_transfer_idx == asset_transfer.idx && c.r#type == ColoringType::Receive
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
                let recipient_id = transfer
                    .recipient_id
                    .clone()
                    .expect("transfer should have a recipient ID");
                debug!(self.logger, "Recipient ID: {recipient_id}");
                let ack_res = self.rest_client.clone().get_ack(&proxy_url, recipient_id)?;
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
            let transfer_dir = self.transfers_dir().join(
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
        &self,
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
        let confirmations = self.indexer().get_tx_confirmations(&txid)?;
        debug!(self.logger, "Confirmations: {:?}", confirmations);

        if let Some(confirmations) = confirmations {
            if confirmations < batch_transfer.min_confirmations as u64 {
                return Ok(None);
            }
        } else {
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
            debug!(self.logger, "Recipient ID: {recipient_id}");
            let transfer_dir = self.transfers_dir().join(recipient_id);
            let consignment_path = transfer_dir.join(CONSIGNMENT_RCV_FILE);
            let consignment =
                RgbTransfer::load_file(consignment_path).map_err(InternalError::from)?;

            if transfer.recipient_type == Some(RecipientType::Witness) {
                self._sync_db_txos()?;
                let outpoint = Outpoint {
                    txid,
                    vout: transfer.vout.unwrap(),
                };
                let utxo = self
                    .database
                    .get_txo(&outpoint)?
                    .expect("outpoint should be in the DB");

                let db_coloring = DbColoringActMod {
                    txo_idx: ActiveValue::Set(utxo.idx),
                    asset_transfer_idx: ActiveValue::Set(asset_transfer.idx),
                    r#type: ActiveValue::Set(ColoringType::Receive),
                    amount: ActiveValue::Set(transfer.amount),
                    ..Default::default()
                };
                self.database.set_coloring(db_coloring)?;

                self.database.del_pending_witness_outpoint(outpoint)?;
            }

            // accept consignment
            let consignment = consignment
                .validate(&mut self.blockchain_resolver()?, self.testnet())
                .unwrap_or_else(|c| c);
            let mut runtime = self.rgb_runtime()?;
            let force = false;
            let validation_status =
                runtime.accept_transfer(consignment, &mut self.blockchain_resolver()?, force)?;
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
        &self,
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
        &self,
        transfer: &DbBatchTransfer,
        db_data: &mut DbData,
        filter: &[RefreshFilter],
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

    /// Update pending RGB transfers, based on their current status, and return a
    /// [`RefreshResult`].
    ///
    /// An optional `asset_id` can be provided to refresh transfers related to a specific asset.
    ///
    /// Each item in the [`RefreshFilter`] vector defines transfers to be refreshed. Transfers not
    /// matching any provided filter are skipped. If the vector is empty, all transfers are
    /// refreshed.
    pub fn refresh(
        &self,
        online: Online,
        asset_id: Option<String>,
        filter: Vec<RefreshFilter>,
    ) -> Result<RefreshResult, Error> {
        if let Some(aid) = asset_id.clone() {
            info!(self.logger, "Refreshing asset {}...", aid);
            self.database.check_asset_exists(aid)?;
        } else {
            info!(self.logger, "Refreshing assets...");
        }
        self.check_online(online)?;

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

        let mut refresh_result = HashMap::new();
        for transfer in db_data.batch_transfers.clone().into_iter() {
            let mut failure = None;
            let mut updated_status = None;
            match self._refresh_transfer(&transfer, &mut db_data, &filter) {
                Ok(Some(updated_transfer)) => updated_status = Some(updated_transfer.status),
                Err(e) => failure = Some(e),
                _ => {}
            }
            refresh_result.insert(
                transfer.idx,
                RefreshedTransfer {
                    updated_status,
                    failure,
                },
            );
        }

        if refresh_result.transfers_changed() {
            self.update_backup_info(false)?;
        }

        info!(self.logger, "Refresh completed");
        Ok(refresh_result)
    }

    fn _select_rgb_inputs(
        &self,
        asset_id: String,
        amount_needed: u64,
        unspents: Vec<LocalUnspent>,
        transfers: Option<Vec<DbTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
    ) -> Result<AssetSpend, Error> {
        fn cmp_localunspent_allocation_sum(a: &LocalUnspent, b: &LocalUnspent) -> Ordering {
            let a_sum: u64 = a.rgb_allocations.iter().map(|a| a.amount).sum();
            let b_sum: u64 = b.rgb_allocations.iter().map(|a| a.amount).sum();
            a_sum.cmp(&b_sum)
        }

        debug!(self.logger, "Selecting inputs for asset '{}'...", asset_id);
        let mut input_allocations: HashMap<DbTxo, u64> = HashMap::new();
        let mut amount_input_asset: u64 = 0;
        let mut mut_unspents = unspents;
        mut_unspents.sort_by(cmp_localunspent_allocation_sum);
        for unspent in mut_unspents {
            let asset_allocations: Vec<LocalRgbAllocation> = unspent
                .rgb_allocations
                .into_iter()
                .filter(|a| a.asset_id == Some(asset_id.clone()) && a.status.settled())
                .collect();
            if asset_allocations.is_empty() {
                continue;
            }
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
                transfers,
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
        witness_recipients: &Vec<(ScriptBuf, u64)>,
        fee_rate: f32,
    ) -> Result<(BdkPsbt, Option<BtcChange>), Error> {
        let mut builder = self.bdk_wallet.build_tx();
        builder
            .add_utxos(&input_outpoints)
            .map_err(InternalError::from)?
            .manually_selected_only()
            .fee_rate(FeeRate::from_sat_per_vb(fee_rate))
            .ordering(bdk::wallet::tx_builder::TxOrdering::Untouched);
        for (script_buf, amount_sat) in witness_recipients {
            builder.add_recipient(script_buf.clone(), *amount_sat);
        }
        let change_addr = self.get_new_address().script_pubkey();
        builder.drain_to(change_addr.clone()).add_data(&[]);

        let (psbt, _) = builder.finish().map_err(|e| match e {
            bdk::Error::InsufficientFunds { needed, available } => {
                Error::InsufficientBitcoins { needed, available }
            }
            bdk::Error::OutputBelowDustLimit(_) => Error::OutputBelowDustLimit,
            _ => Error::Internal {
                details: e.to_string(),
            },
        })?;

        let btc_change = psbt
            .unsigned_tx
            .output
            .iter()
            .enumerate()
            .find(|(_, o)| o.script_pubkey == change_addr)
            .map(|(i, o)| BtcChange {
                vout: i as u32,
                amount: o.value,
            });

        Ok((psbt, btc_change))
    }

    fn _try_prepare_psbt(
        &self,
        input_unspents: &[LocalUnspent],
        all_inputs: &mut Vec<BdkOutPoint>,
        witness_recipients: &Vec<(ScriptBuf, u64)>,
        fee_rate: f32,
    ) -> Result<(BdkPsbt, Option<BtcChange>), Error> {
        Ok(loop {
            break match self._prepare_psbt(all_inputs.clone(), witness_recipients, fee_rate) {
                Ok(res) => res,
                Err(Error::InsufficientBitcoins { .. }) => {
                    let used_txos: Vec<Outpoint> =
                        all_inputs.clone().into_iter().map(|o| o.into()).collect();
                    let mut free_utxos = self.get_available_allocations(
                        input_unspents.to_vec(),
                        used_txos.clone(),
                        Some(0),
                    )?;
                    // sort UTXOs by BTC amount
                    if !free_utxos.is_empty() {
                        // pre-parse BTC amounts to make sure no one will fail
                        for u in &free_utxos {
                            u.utxo
                                .btc_amount
                                .parse::<u64>()
                                .map_err(|e| Error::Internal {
                                    details: e.to_string(),
                                })?;
                        }
                        free_utxos.sort_by_key(|u| u.utxo.btc_amount.parse::<u64>().unwrap());
                    }
                    if let Some(a) = free_utxos.pop() {
                        all_inputs.push(a.utxo.into());
                        continue;
                    }
                    return Err(self.detect_btc_unspendable_err()?);
                }
                Err(e) => return Err(e),
            };
        })
    }

    fn _get_change_seal(
        &self,
        btc_change: &Option<BtcChange>,
        change_utxo_option: &mut Option<DbTxo>,
        change_utxo_idx: &mut Option<i32>,
        input_outpoints: Vec<OutPoint>,
        unspents: Vec<LocalUnspent>,
    ) -> Result<XChain<BlindSeal<TxPtr>>, Error> {
        let graph_seal = if let Some(btc_change) = btc_change {
            GraphSeal::new_random_vout(CloseMethod::OpretFirst, btc_change.vout)
        } else {
            if change_utxo_option.is_none() {
                let change_utxo = self.get_utxo(
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
            let change_utxo = change_utxo_option.clone().unwrap();
            let blind_seal = BlindSeal::opret_first_rand(
                BpTxid::from_str(&change_utxo.txid).unwrap(),
                change_utxo.vout,
            );
            GraphSeal::from(blind_seal)
        };
        Ok(XChain::with(Layer1::Bitcoin, graph_seal))
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
        btc_change: Option<BtcChange>,
    ) -> Result<(), Error> {
        let mut change_utxo_option = None;
        let mut change_utxo_idx = None;

        let prev_outputs = psbt
            .unsigned_tx
            .input
            .iter()
            .map(|txin| txin.previous_output)
            .map(|outpoint| {
                XChain::with(
                    Layer1::Bitcoin,
                    ExplicitSeal::new(CloseMethod::OpretFirst, Outpoint::from(outpoint).into()),
                )
            })
            .collect::<HashSet<XOutputSeal>>();

        let mut all_transitions: HashMap<ContractId, Transition> = HashMap::new();
        let mut asset_beneficiaries: BTreeMap<
            String,
            Vec<BuilderSeal<ChainBlindSeal<CloseMethod>>>,
        > = bmap![];
        let assignment_name = FieldName::from("assetOwner");
        for (asset_id, transfer_info) in transfer_info_map.clone() {
            let change_amount = transfer_info.asset_spend.change_amount;
            let iface = transfer_info.asset_iface.to_typename();
            let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
            let mut asset_transition_builder =
                runtime.transition_builder(contract_id, iface.clone(), None::<&str>)?;
            let assignment_id = asset_transition_builder
                .assignments_type(&assignment_name)
                .ok_or(InternalError::Unexpected)?;

            let mut uda_state = None;
            for ((opout, _), state) in
                runtime.state_for_outpoints(contract_id, prev_outputs.iter().copied())?
            {
                // there can be only a single state when contract is UDA
                uda_state = Some(state.clone());
                asset_transition_builder = asset_transition_builder.add_input(opout, state)?;
            }

            if change_amount > 0 {
                let seal = self._get_change_seal(
                    &btc_change,
                    &mut change_utxo_option,
                    &mut change_utxo_idx,
                    input_outpoints.clone(),
                    unspents.clone(),
                )?;
                asset_transition_builder = asset_transition_builder.add_fungible_state_raw(
                    assignment_id,
                    seal,
                    change_amount,
                    BlindingFactor::random(),
                )?;
            };

            let mut beneficiaries: Vec<BuilderSeal<ChainBlindSeal<CloseMethod>>> = vec![];
            for recipient in transfer_info.recipients.clone() {
                let seal: BuilderSeal<GraphSeal> = match recipient.local_recipient_data {
                    LocalRecipientData::Blind(secret_seal) => {
                        BuilderSeal::Concealed(XChain::with(Layer1::Bitcoin, secret_seal))
                    }
                    LocalRecipientData::Witness(witness_data) => {
                        let graph_seal = if let Some(blinding) = witness_data.blinding {
                            GraphSeal::with_blinded_vout(
                                CloseMethod::OpretFirst,
                                witness_data.vout,
                                blinding,
                            )
                        } else {
                            GraphSeal::new_random_vout(CloseMethod::OpretFirst, witness_data.vout)
                        };
                        BuilderSeal::Revealed(XChain::with(Layer1::Bitcoin, graph_seal))
                    }
                };

                beneficiaries.push(seal);
                match transfer_info.asset_iface {
                    AssetIface::RGB20 | AssetIface::RGB25 => {
                        asset_transition_builder = asset_transition_builder
                            .add_fungible_state_raw(
                                assignment_id,
                                seal,
                                recipient.amount,
                                BlindingFactor::random(),
                            )?;
                    }
                    AssetIface::RGB21 => {
                        asset_transition_builder = asset_transition_builder
                            .add_owned_state_raw(assignment_id, seal, uda_state.clone().unwrap())
                            .map_err(Error::from)?;
                    }
                }
            }

            let transition = asset_transition_builder.complete_transition()?;
            all_transitions.insert(contract_id, transition);
            asset_beneficiaries.insert(asset_id.clone(), beneficiaries);

            let asset_transfer_dir = transfer_dir.join(&asset_id);
            if asset_transfer_dir.is_dir() {
                fs::remove_dir_all(&asset_transfer_dir)?;
            }
            fs::create_dir_all(&asset_transfer_dir)?;

            // save asset transfer data to file (for send_end)
            let serialized_info =
                serde_json::to_string(&transfer_info).map_err(InternalError::from)?;
            let info_file = asset_transfer_dir.join(TRANSFER_DATA_FILE);
            fs::write(info_file, serialized_info)?;
        }

        let mut contract_inputs = HashMap::<ContractId, Vec<XOutputSeal>>::new();
        let mut blank_state =
            HashMap::<ContractId, BTreeMap<(Opout, XOutputSeal), PersistedState>>::new();
        for output in prev_outputs {
            for id in runtime.contracts_by_outputs([output])? {
                contract_inputs.entry(id).or_default().push(output);
                let cid_str = id.to_string();
                if transfer_info_map.contains_key(&cid_str) {
                    continue;
                }
                blank_state
                    .entry(id)
                    .or_default()
                    .extend(runtime.state_for_outpoints(id, [output])?);
            }
        }

        let mut blank_allocations: HashMap<String, u64> = HashMap::new();
        for (cid, opouts) in blank_state {
            let asset_iface = self._get_asset_iface(cid, runtime)?;
            let iface = asset_iface.to_typename();
            let mut blank_builder = runtime.blank_builder(cid, iface.clone())?;
            let mut moved_amount = 0;

            for ((opout, _output), state) in opouts {
                if let PersistedState::Amount(amt, _, _) = &state {
                    moved_amount += amt.value()
                }
                let seal = self._get_change_seal(
                    &btc_change,
                    &mut change_utxo_option,
                    &mut change_utxo_idx,
                    input_outpoints.clone(),
                    unspents.clone(),
                )?;
                blank_builder = blank_builder
                    .add_input(opout, state.clone())?
                    .add_owned_state_raw(opout.ty, seal, state)?;
            }
            let blank_transition = blank_builder.complete_transition()?;
            all_transitions.insert(cid, blank_transition);
            blank_allocations.insert(cid.to_string(), moved_amount);
        }

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
        opreturn_output.set_opret_host();

        for (id, transition) in all_transitions {
            let inputs = contract_inputs.remove(&id).unwrap_or_default();
            for (input, txin) in psbt.inputs.iter_mut().zip(&psbt.unsigned_tx.input) {
                let prevout = txin.previous_output;
                let outpoint = RgbOutpoint::new(prevout.txid.to_byte_array().into(), prevout.vout);
                let output = XChain::with(
                    Layer1::Bitcoin,
                    ExplicitSeal::new(CloseMethod::OpretFirst, outpoint),
                );
                if inputs.contains(&output) {
                    input.set_rgb_consumer(id, transition.id())?;
                }
            }
            psbt.push_rgb_transition(transition, CloseMethodSet::OpretFirst)?;
        }

        let mut rgb_psbt = RgbPsbt::from_str(&psbt.to_string()).unwrap();
        rgb_psbt.complete_construction();
        let fascia = rgb_psbt
            .rgb_commit()
            .map_err(|_| InternalError::Unexpected)?;

        let witness_txid = rgb_psbt.txid();

        runtime.consume(fascia)?;

        for (asset_id, _transfer_info) in transfer_info_map {
            let asset_transfer_dir = transfer_dir.join(&asset_id);
            let consignment_path = asset_transfer_dir.join(CONSIGNMENT_FILE);
            let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
            let beneficiaries = asset_beneficiaries[&asset_id].clone();
            let mut beneficiaries_outputs = vec![];
            let mut beneficiaries_secret_seals = vec![];
            for beneficiary in beneficiaries {
                match beneficiary {
                    BuilderSeal::Revealed(seal) => {
                        beneficiaries_outputs.push(XChain::Bitcoin(ExplicitSeal::new(
                            CloseMethod::OpretFirst,
                            RgbOutpoint::new(
                                witness_txid.to_byte_array().into(),
                                seal.as_reduced_unsafe().vout,
                            ),
                        )))
                    }
                    BuilderSeal::Concealed(seal) => beneficiaries_secret_seals.push(seal),
                };
            }
            let mut transfer = runtime.transfer(
                contract_id,
                beneficiaries_outputs,
                beneficiaries_secret_seals,
            )?;

            let mut terminals = transfer.terminals.to_inner();
            for (bundle_id, terminal) in terminals.iter_mut() {
                let Some(ab) = transfer.anchored_bundle(*bundle_id) else {
                    continue;
                };
                if ab.anchor.witness_id_unchecked() == WitnessId::Bitcoin(witness_txid) {
                    terminal.witness_tx = Some(XChain::Bitcoin(rgb_psbt.to_unsigned_tx().into()));
                }
            }
            transfer.terminals = Confined::from_collection_unsafe(terminals);

            transfer.save_file(&consignment_path)?;
        }

        *psbt = PartiallySignedTransaction::from_str(&rgb_psbt.to_string()).unwrap();

        // save batch transfer data to file (for send_end)
        let info_contents = InfoBatchTransfer {
            btc_change,
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
        txid: String,
        medias: Vec<Media>,
    ) -> Result<(), Error> {
        let consignment_path = asset_transfer_dir.join(CONSIGNMENT_FILE);
        for recipient in recipients {
            let recipient_id = &recipient.recipient_id;
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
                debug!(
                    self.logger,
                    "Posting consignment for recipient ID: {recipient_id}"
                );
                match self.post_consignment(
                    &proxy_url,
                    recipient_id.clone(),
                    &consignment_path,
                    txid.clone(),
                    recipient.local_recipient_data.vout(),
                ) {
                    Err(Error::RecipientIDAlreadyUsed) => {
                        return Err(Error::RecipientIDAlreadyUsed)
                    }
                    Err(_) => continue,
                    Ok(()) => {}
                }

                for media in &medias {
                    let media_res = self.rest_client.clone().post_media(
                        &proxy_url,
                        media.get_digest(),
                        &media.file_path,
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
        btc_change: Option<BtcChange>,
        broadcasted: bool,
        status: TransferStatus,
        min_confirmations: u8,
    ) -> Result<i32, Error> {
        let created_at = now().unix_timestamp();
        let expiration = Some(created_at + DURATION_SEND_TRANSFER);

        let batch_transfer = DbBatchTransferActMod {
            txid: ActiveValue::Set(Some(txid.clone())),
            status: ActiveValue::Set(status),
            expiration: ActiveValue::Set(expiration),
            created_at: ActiveValue::Set(created_at),
            min_confirmations: ActiveValue::Set(min_confirmations),
            ..Default::default()
        };
        let batch_transfer_idx = self.database.set_batch_transfer(batch_transfer)?;

        let change_utxo_idx = if let Some(btc_change) = btc_change {
            Some(if broadcasted {
                let db_txo = self
                    .database
                    .get_txo(&Outpoint {
                        txid: txid.clone(),
                        vout: btc_change.vout,
                    })?
                    .expect("outpoint should be in the DB");
                db_txo.idx
            } else {
                let db_utxo = DbTxoActMod {
                    txid: ActiveValue::Set(txid.clone()),
                    vout: ActiveValue::Set(btc_change.vout),
                    btc_amount: ActiveValue::Set(btc_change.amount.to_string()),
                    spent: ActiveValue::Set(false),
                    exists: ActiveValue::Set(false),
                    ..Default::default()
                };
                self.database.set_txo(db_utxo)?
            })
        } else {
            change_utxo_idx
        };

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
                    r#type: ActiveValue::Set(ColoringType::Input),
                    amount: ActiveValue::Set(amount.to_string()),
                    ..Default::default()
                };
                self.database.set_coloring(db_coloring)?;
            }
            if asset_spend.change_amount > 0 {
                let db_coloring = DbColoringActMod {
                    txo_idx: ActiveValue::Set(change_utxo_idx.unwrap()),
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    r#type: ActiveValue::Set(ColoringType::Change),
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
                    recipient_id: ActiveValue::Set(Some(recipient.recipient_id.clone())),
                    ..Default::default()
                };
                let transfer_idx = self.database.set_transfer(transfer)?;
                for transport_endpoint in recipient.transport_endpoints {
                    self.save_transfer_transport_endpoint(transfer_idx, &transport_endpoint)?;
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
                r#type: ActiveValue::Set(ColoringType::Change),
                amount: ActiveValue::Set(amt.to_string()),
                ..Default::default()
            };
            self.database.set_coloring(db_coloring)?;
        }

        Ok(batch_transfer_idx)
    }

    pub(crate) fn get_input_unspents(
        &self,
        unspents: &[LocalUnspent],
    ) -> Result<Vec<LocalUnspent>, Error> {
        let pending_witness_outpoints: Vec<Outpoint> = self
            .database
            .iter_pending_witness_outpoints()?
            .iter()
            .map(|o| o.outpoint())
            .collect();
        let mut input_unspents = unspents.to_vec();
        // consider the following UTXOs unspendable:
        // - incoming and pending
        // - outgoing and in waiting counterparty status
        // - pending incoming witness
        // - inexistent
        input_unspents.retain(|u| {
            !((u.rgb_allocations
                .iter()
                .any(|a| a.incoming && a.status.pending()))
                || (u
                    .rgb_allocations
                    .iter()
                    .any(|a| !a.incoming && a.status.waiting_counterparty()))
                || (!pending_witness_outpoints.is_empty()
                    && pending_witness_outpoints.contains(&u.outpoint()))
                || !u.utxo.exists)
        });
        Ok(input_unspents)
    }

    /// Send RGB assets.
    ///
    /// This calls [`send_begin`](Wallet::send_begin), signs the resulting PSBT and finally calls
    /// [`send_end`](Wallet::send_end).
    ///
    /// A wallet with private keys is required.
    pub fn send(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: f32,
        min_confirmations: u8,
    ) -> Result<SendResult, Error> {
        info!(self.logger, "Sending to: {:?}...", recipient_map);
        self._check_xprv()?;

        let unsigned_psbt = self.send_begin(
            online.clone(),
            recipient_map,
            donation,
            fee_rate,
            min_confirmations,
        )?;

        let psbt = self.sign_psbt(unsigned_psbt, None)?;

        self.send_end(online, psbt)
    }

    /// Prepare the PSBT to send RGB assets according to the given recipient map, with the provided
    /// `fee_rate` (in sat/vB).
    ///
    /// The `recipient_map` maps asset IDs to a vector of [`Recipient`]s. When multiple recipients
    /// are provided, a batch transfer will be performed, meaning a single Bitcoin transaction will
    /// be used to move all assets to the respective recipients. Each asset being sent will result
    /// in the creation of a single consignment, which will then be posted to the RGB proxy server
    /// for each of its recipients.
    ///
    /// If `donation` is true, the resulting transaction will be broadcast (by
    /// [`send_end`](Wallet::send_end)) as soon as it's ready, without the need for recipients to
    /// ACK the transfer.
    /// If `donation` is false, all recipients will need to ACK the transfer before the transaction
    /// is broadcast.
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`send_end`](Wallet::send_end) function for broadcasting.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed.
    pub fn send_begin(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: f32,
        min_confirmations: u8,
    ) -> Result<String, Error> {
        info!(self.logger, "Sending (begin) to: {:?}...", recipient_map);
        self.check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        let mut db_data = self.database.get_db_data(false)?;
        self.handle_expired_transfers(&mut db_data)?;

        let receive_ids: Vec<String> = recipient_map
            .values()
            .flatten()
            .map(|r| r.recipient_id.clone())
            .collect();
        let mut receive_ids_dedup = receive_ids.clone();
        receive_ids_dedup.sort();
        receive_ids_dedup.dedup();
        if receive_ids.len() != receive_ids_dedup.len() {
            return Err(Error::RecipientIDDuplicated);
        }
        let mut hasher = DefaultHasher::new();
        receive_ids.hash(&mut hasher);
        let transfer_dir = self.transfers_dir().join(hasher.finish().to_string());
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

        #[cfg(test)]
        let input_unspents = mock_input_unspents(self, &unspents);
        #[cfg(not(test))]
        let input_unspents = self.get_input_unspents(&unspents)?;

        let mut runtime = self.rgb_runtime()?;
        let chainnet: ChainNet = self.bitcoin_network().into();
        let mut witness_recipients: Vec<(ScriptBuf, u64)> = vec![];
        let mut recipient_vout = 0;
        let mut transfer_info_map: BTreeMap<String, InfoAssetTransfer> = BTreeMap::new();
        for (asset_id, recipients) in recipient_map {
            self.database.check_asset_exists(asset_id.clone())?;

            let mut local_recipients: Vec<LocalRecipient> = vec![];
            for recipient in recipients.clone() {
                self.check_transport_endpoints(&recipient.transport_endpoints)?;
                if recipient.amount == 0 {
                    return Err(Error::InvalidAmountZero);
                }

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

                let xchainnet_beneficiary =
                    XChainNet::<Beneficiary>::from_str(&recipient.recipient_id)
                        .map_err(|_| Error::InvalidRecipientID)?;

                if xchainnet_beneficiary.chain_network() != chainnet {
                    return Err(Error::InvalidRecipientNetwork);
                }

                let local_recipient_data = match xchainnet_beneficiary.into_inner() {
                    Beneficiary::BlindedSeal(secret_seal) => {
                        if recipient.witness_data.is_some() {
                            return Err(Error::InvalidRecipientData {
                                details: s!("cannot provide witness data for a blinded recipient"),
                            });
                        }
                        LocalRecipientData::Blind(secret_seal)
                    }
                    Beneficiary::WitnessVout(address_payload) => {
                        if let Some(ref witness_data) = recipient.witness_data {
                            let script_buf =
                                ScriptBuf::from_hex(&address_payload.script_pubkey().to_hex())
                                    .unwrap();
                            witness_recipients.push((script_buf.clone(), witness_data.amount_sat));
                            let local_witness_data = LocalWitnessData {
                                amount_sat: witness_data.amount_sat,
                                blinding: witness_data.blinding,
                                vout: recipient_vout,
                            };
                            recipient_vout += 1;
                            LocalRecipientData::Witness(local_witness_data)
                        } else {
                            return Err(Error::InvalidRecipientData {
                                details: s!("missing witness data for a witness recipient"),
                            });
                        }
                    }
                };

                local_recipients.push(LocalRecipient {
                    recipient_id: recipient.recipient_id,
                    local_recipient_data,
                    amount: recipient.amount,
                    transport_endpoints,
                })
            }

            let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
            let asset_iface = self._get_asset_iface(contract_id, &runtime)?;
            let amount: u64 = recipients.iter().map(|a| a.amount).sum();
            let asset_spend = self._select_rgb_inputs(
                asset_id.clone(),
                amount,
                input_unspents.clone(),
                Some(db_data.transfers.clone()),
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
        let (psbt, _) = self._try_prepare_psbt(
            &input_unspents,
            &mut all_inputs,
            &witness_recipients,
            fee_rate,
        )?;
        let vbytes = psbt.extract_tx().vsize() as f32;
        let updated_fee_rate = ((vbytes + OPRET_VBYTES) / vbytes) * fee_rate;
        let (psbt, btc_change) = self._try_prepare_psbt(
            &input_unspents,
            &mut all_inputs,
            &witness_recipients,
            updated_fee_rate,
        )?;
        let mut psbt = PartiallySignedTransaction::from_str(&psbt.to_string()).unwrap();
        let all_inputs: Vec<OutPoint> = all_inputs
            .iter()
            .map(|i| OutPoint {
                txid: Txid::from_byte_array(*i.txid.as_ref()),
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
            btc_change,
        )?;

        // rename transfer directory
        let txid = psbt.clone().extract_tx().txid().to_string();
        let new_transfer_dir = self.transfers_dir().join(txid);
        fs::rename(transfer_dir, new_transfer_dir)?;

        info!(self.logger, "Send (begin) completed");
        Ok(psbt.to_string())
    }

    /// Complete the send operation by saving the PSBT to disk, POSTing consignments to the RGB
    /// proxy server, saving the transfer to DB and broadcasting the provided PSBT, if appropriate.
    ///
    /// The provided PSBT, prepared with the [`send_begin`](Wallet::send_begin) function, needs to
    /// have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a [`SendResult`].
    pub fn send_end(&self, online: Online, signed_psbt: String) -> Result<SendResult, Error> {
        info!(self.logger, "Sending (end)...");
        self.check_online(online)?;

        // save signed PSBT
        let psbt = BdkPsbt::from_str(&signed_psbt)?;
        let txid = psbt.clone().extract_tx().txid().to_string();
        let transfer_dir = self.transfers_dir().join(&txid);
        let psbt_out = transfer_dir.join(SIGNED_PSBT_FILE);
        fs::write(psbt_out, psbt.to_string())?;

        // restore transfer data
        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        let serialized_info = fs::read_to_string(info_file)?;
        let info_contents: InfoBatchTransfer =
            serde_json::from_str(&serialized_info).map_err(InternalError::from)?;
        let mut medias = None;
        let mut tokens = None;
        let mut token_medias = None;
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
            let asset = self.database.get_asset(asset_id.clone())?.unwrap();
            let token = match asset.schema {
                AssetSchema::Uda => {
                    if medias.clone().is_none() {
                        medias = Some(self.database.iter_media()?);
                        tokens = Some(self.database.iter_tokens()?);
                        token_medias = Some(self.database.iter_token_medias()?);
                    }
                    self.get_asset_token(
                        asset.idx,
                        medias.as_ref().unwrap(),
                        tokens.as_ref().unwrap(),
                        token_medias.as_ref().unwrap(),
                    )?
                }
                AssetSchema::Nia | AssetSchema::Cfa => None,
            };

            // post consignment(s) and optional media(s)
            self._post_transfer_data(
                &mut info_contents.recipients,
                asset_transfer_dir,
                txid.clone(),
                self._get_asset_medias(asset.media_idx, token)?,
            )?;

            transfer_info_map.insert(asset_id, info_contents.clone());
        }

        // broadcast PSBT if donation and finally save transfer to DB
        let status = if info_contents.donation {
            self._broadcast_psbt(psbt)?;
            TransferStatus::WaitingConfirmations
        } else {
            TransferStatus::WaitingCounterparty
        };
        let batch_transfer_idx = self._save_transfers(
            txid.clone(),
            transfer_info_map,
            info_contents.blank_allocations,
            info_contents.change_utxo_idx,
            info_contents.btc_change,
            info_contents.donation,
            status,
            info_contents.min_confirmations,
        )?;

        self.update_backup_info(false)?;

        info!(self.logger, "Send (end) completed");
        Ok(SendResult {
            txid,
            batch_transfer_idx,
        })
    }

    /// Send bitcoins using the vanilla wallet.
    ///
    /// This calls [`send_btc_begin`](Wallet::send_btc_begin), signs the resulting PSBT and finally
    /// calls [`send_btc_end`](Wallet::send_btc_end).
    ///
    /// A wallet with private keys and [`Online`] data are required.
    pub fn send_btc(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: f32,
    ) -> Result<String, Error> {
        info!(self.logger, "Sending BTC...");
        self._check_xprv()?;

        let unsigned_psbt = self.send_btc_begin(online.clone(), address, amount, fee_rate)?;

        let psbt = self.sign_psbt(unsigned_psbt, None)?;

        self.send_btc_end(online, psbt)
    }

    /// Prepare the PSBT to send the specified `amount` of bitcoins (in sats) using the vanilla
    /// wallet to the specified Bitcoin `address` with the specified `fee_rate` (in sat/vB).
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`send_btc_end`](Wallet::send_btc_end) function.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed.
    pub fn send_btc_begin(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: f32,
    ) -> Result<String, Error> {
        info!(self.logger, "Sending BTC (begin)...");
        self.check_online(online)?;
        self._check_fee_rate(fee_rate)?;

        self._sync_db_txos()?;

        let address = BdkAddress::from_str(&address)?;
        if !address.is_valid_for_network(self.bitcoin_network().into()) {
            return Err(Error::InvalidAddress {
                details: s!("belongs to another network"),
            });
        }

        let unspendable = self._get_unspendable_bdk_outpoints()?;

        let mut tx_builder = self.bdk_wallet.build_tx();
        tx_builder
            .unspendable(unspendable)
            .add_recipient(address.payload.script_pubkey(), amount)
            .fee_rate(FeeRate::from_sat_per_vb(fee_rate));

        let psbt = tx_builder
            .finish()
            .map_err(|e| match e {
                bdk::Error::InsufficientFunds { needed, available } => {
                    Error::InsufficientBitcoins { needed, available }
                }
                bdk::Error::OutputBelowDustLimit(_) => Error::OutputBelowDustLimit,
                _ => Error::Internal {
                    details: e.to_string(),
                },
            })?
            .0;

        info!(self.logger, "Send BTC (begin) completed");
        Ok(psbt.to_string())
    }

    /// Broadcast the provided PSBT to send bitcoins using the vanilla wallet.
    ///
    /// The provided PSBT, prepared with the [`send_btc_begin`](Wallet::send_btc_begin) function,
    /// needs to have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns the TXID of the broadcasted transaction.
    pub fn send_btc_end(&self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger, "Sending BTC (end)...");
        self.check_online(online)?;

        let signed_psbt = BdkPsbt::from_str(&signed_psbt)?;
        let tx = self._broadcast_psbt(signed_psbt)?;

        info!(self.logger, "Send BTC (end) completed");
        Ok(tx.txid().to_string())
    }
}
