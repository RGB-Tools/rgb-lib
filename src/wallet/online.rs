//! Online functionality.
//!
//! This module defines the online wallet methods.

use super::*;

const SCHEMAS_SUPPORTING_BURN: [database::enums::AssetSchema; 1] = [AssetSchema::Ifa];
const SCHEMAS_SUPPORTING_INFLATION: [database::enums::AssetSchema; 1] = [AssetSchema::Ifa];

const SIGNED_PSBT_FILE: &str = "signed.psbt";

const MIN_FEE_RATE: u64 = 1;

pub(crate) const UTXO_SIZE: u32 = 1000;
pub(crate) const UTXO_NUM: u8 = 5;

pub(crate) const MIN_BLOCK_ESTIMATION: u16 = 1;
pub(crate) const MAX_BLOCK_ESTIMATION: u16 = 1008;

pub trait WalletOnline: WalletOffline {
    fn blockchain_resolver(&self) -> &AnyResolver {
        &self.online_data().as_ref().unwrap().resolver
    }

    fn check_fee_rate(&self, fee_rate: u64) -> Result<FeeRate, Error> {
        #[cfg(test)]
        if skip_check_fee_rate() {
            return Ok(FeeRate::from_sat_per_vb(fee_rate).unwrap());
        };
        if fee_rate < MIN_FEE_RATE {
            return Err(Error::InvalidFeeRate {
                details: format!("value under minimum {MIN_FEE_RATE}"),
            });
        }
        let Some(fee_rate) = FeeRate::from_sat_per_vb(fee_rate) else {
            return Err(Error::InvalidFeeRate {
                details: s!("value overflows"),
            });
        };
        Ok(fee_rate)
    }

    fn sync_impl(&mut self, txn: &DbTxn, options: SyncOptions) -> Result<(), Error> {
        self.sync_bdk_and_db_txos(txn, options, false)
    }

    fn broadcast_tx(&self, tx: BdkTransaction) -> Result<BdkTransaction, Error> {
        let txid = tx.compute_txid().to_string();
        let indexer = self.indexer();
        match indexer.broadcast(&tx) {
            Ok(_) => {
                debug!(self.logger(), "Broadcasted TX with ID '{}'", txid);
                Ok(tx)
            }
            Err(e) => {
                match e {
                    #[cfg(feature = "electrum")]
                    IndexerError::Electrum(ref e) => {
                        let err_str = e.to_string();
                        if err_str.contains("min relay fee not met")
                            || err_str.contains("mempool min fee not met")
                        {
                            return Err(Error::MinFeeNotMet { txid: txid.clone() });
                        } else if err_str.contains("Fee exceeds maximum configured") {
                            return Err(Error::MaxFeeExceeded { txid: txid.clone() });
                        }
                    }
                    #[cfg(feature = "esplora")]
                    IndexerError::Esplora(ref e) => {
                        if let EsploraError::HttpResponse { message, .. } = e {
                            if message.contains("min relay fee not met") {
                                return Err(Error::MinFeeNotMet { txid: txid.clone() });
                            } else if message.contains("Fee exceeds maximum configured") {
                                return Err(Error::MaxFeeExceeded { txid: txid.clone() });
                            }
                        }
                    }
                }
                if indexer.get_tx_confirmations(&txid)?.is_none() {
                    return Err(Error::FailedBroadcast {
                        details: e.to_string(),
                    });
                }
                Ok(tx)
            }
        }
    }

    fn broadcast_psbt(&mut self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<BdkTransaction, Error> {
        let tx = self.broadcast_tx(
            signed_psbt
                .clone()
                .extract_tx()
                .map_err(InternalError::from)?,
        )?;

        // apply the broadcast TX into BDK directly so its outputs are immediately visible
        // (revealed change SPKs match without needing a wallet sync)
        let seen_at = now().unix_timestamp() as u64;
        let (bdk_wallet, bdk_db) = self.bdk_wallet_db_mut();
        bdk_wallet.apply_unconfirmed_txs([(tx.clone(), seen_at)]);
        bdk_wallet.persist(bdk_db)?;

        // promote any newly-known colored UTXOs (e.g. the change output) from
        // exists=false to exists=true in the rgb_lib DB
        self.update_db_colored_txos_from_bdk(txn, false)?;

        for input in tx.clone().input {
            let txid = input.previous_output.txid.to_string();
            let vout = input.previous_output.vout;
            if let Some(db_txo) = txn.get_txo(&Outpoint { txid, vout })? {
                let mut db_txo: DbTxoActMod = db_txo.into();
                db_txo.spent = ActiveValue::Set(true);
                txn.update_txo(db_txo)?;
            }
        }

        Ok(tx)
    }

    fn reserve_vanilla_txos(
        &self,
        txn: &DbTxn,
        psbt: &Psbt,
        r#type: WalletTransactionType,
    ) -> Result<(), Error> {
        let txid = psbt.unsigned_tx.compute_txid().to_string();
        let wt_idx = txn.set_wallet_transaction(DbWalletTransactionActMod {
            txid: ActiveValue::Set(txid),
            r#type: ActiveValue::Set(r#type),
            ..Default::default()
        })?;
        let reservations: Vec<DbReservedTxoActMod> = psbt
            .unsigned_tx
            .input
            .iter()
            .map(|i| DbReservedTxoActMod {
                txid: ActiveValue::Set(i.previous_output.txid.to_string()),
                vout: ActiveValue::Set(i.previous_output.vout),
                reserved_for: ActiveValue::Set(Some(wt_idx)),
                ..Default::default()
            })
            .collect();
        txn.set_reserved_txos(reservations)?;
        Ok(())
    }

    fn finalize_vanilla_wallet_transaction(
        &self,
        txn: &DbTxn,
        psbt: &Psbt,
        r#type: WalletTransactionType,
    ) -> Result<(), Error> {
        let txid = psbt.unsigned_tx.compute_txid().to_string();
        match txn.get_wallet_transaction_with_reserved_txos_by_txid(&txid)? {
            Some((_wt, reservations)) => {
                txn.del_reserved_txos(&reservations)?;
            }
            None => {
                txn.set_wallet_transaction(DbWalletTransactionActMod {
                    txid: ActiveValue::Set(txid),
                    r#type: ActiveValue::Set(r#type),
                    ..Default::default()
                })?;
            }
        }
        Ok(())
    }

    fn broadcast_and_update_rgb(
        &mut self,
        txn: &DbTxn,
        runtime: &mut RgbRuntime,
        signed_psbt: &Psbt,
        fascia: Fascia,
    ) -> Result<BdkTransaction, Error> {
        let tx = self.broadcast_psbt(txn, signed_psbt)?;
        runtime.consume_fascia(fascia, None)?;
        Ok(tx)
    }

    fn create_split_tx(
        &mut self,
        inputs: &[BdkOutPoint],
        addresses: &Vec<ScriptBuf>,
        size: u32,
        fee_rate: FeeRate,
    ) -> Result<Psbt, bdk_wallet::error::CreateTxError> {
        let mut tx_builder = self.bdk_wallet_mut().build_tx();
        tx_builder
            .add_utxos(inputs)
            .map_err(|_| bdk_wallet::error::CreateTxError::UnknownUtxo)?
            .manually_selected_only()
            .fee_rate(fee_rate);
        for address in addresses {
            tx_builder.add_recipient(address.clone(), BdkAmount::from_sat(size as u64));
        }
        tx_builder.finish()
    }

    fn create_utxos_begin_impl(
        &mut self,
        txn: &DbTxn,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
        dry_run: bool,
    ) -> Result<Psbt, Error> {
        let fee_rate_checked = self.check_fee_rate(fee_rate)?;

        if !skip_sync {
            self.sync_wallet(
                txn,
                SyncOptions {
                    keychain: SyncKeychain::Vanilla {
                        lookback: self.vanilla_sync_lookback(),
                    },
                    strategy: SyncStrategy::FastSync,
                },
                false,
            )?;
        }

        let unspent_txos = txn.get_unspent_txos(vec![])?;
        let unspents = txn.get_rgb_allocations(unspent_txos, None, None, None, None)?;

        let mut utxos_to_create = num.unwrap_or(UTXO_NUM);
        if up_to {
            let allocatable = self.get_available_allocations(unspents, &[], None)?.len() as u8;
            if allocatable >= utxos_to_create {
                return Err(Error::AllocationsAlreadyAvailable);
            }
            utxos_to_create -= allocatable
        }
        debug!(
            self.logger(),
            "Will try to create {} UTXOs", utxos_to_create
        );

        let reserved: HashSet<BdkOutPoint> = self
            .get_reserved_vanilla_outpoints(txn)?
            .into_iter()
            .collect();
        let (inputs, usable_btc_amount) = self.internal_unspents().fold(
            (Vec::new(), 0u64),
            |(mut inputs, usable_btc_amount), u| {
                let outpoint = u.outpoint;
                let value = u.txout.value.to_sat();
                if reserved.contains(&outpoint) {
                    (inputs, usable_btc_amount)
                } else {
                    inputs.push(outpoint);
                    (inputs, usable_btc_amount + value)
                }
            },
        );
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
        let num_try_creating = min(utxos_to_create, max_possible_utxos);
        let mut addresses = vec![];
        for _i in 0..num_try_creating {
            addresses.push(self.get_new_address()?.script_pubkey());
        }
        while !addresses.is_empty() {
            match self.create_split_tx(&inputs, &addresses, utxo_size, fee_rate_checked) {
                Ok(psbt) => {
                    if !dry_run {
                        self.reserve_vanilla_txos(txn, &psbt, WalletTransactionType::CreateUtxos)?;
                    }
                    return Ok(psbt);
                }
                Err(e) => {
                    (btc_needed, btc_available) = match e {
                        bdk_wallet::error::CreateTxError::CoinSelection(InsufficientFunds {
                            needed,
                            available,
                        }) => (needed.to_sat(), available.to_sat()),
                        bdk_wallet::error::CreateTxError::OutputBelowDustLimit(_) => {
                            return Err(Error::OutputBelowDustLimit);
                        }
                        _ => {
                            return Err(Error::Internal {
                                details: e.to_string(),
                            });
                        }
                    };
                    addresses.pop()
                }
            };
        }
        Err(Error::InsufficientBitcoins {
            needed: btc_needed,
            available: btc_available,
        })
    }

    fn create_utxos_end_impl(&mut self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<u8, Error> {
        self.finalize_vanilla_wallet_transaction(
            txn,
            signed_psbt,
            WalletTransactionType::CreateUtxos,
        )?;

        let tx = self.broadcast_psbt(txn, signed_psbt)?;

        let mut num_utxos_created = 0;
        let bdk_utxos: Vec<LocalOutput> = self.bdk_wallet().list_unspent().collect();
        let txid = tx.compute_txid();
        for utxo in bdk_utxos.into_iter() {
            if utxo.outpoint.txid == txid && utxo.keychain == KeychainKind::External {
                num_utxos_created += 1
            }
        }

        Ok(num_utxos_created)
    }

    fn drain_to_begin_impl(
        &mut self,
        txn: &DbTxn,
        address: String,
        fee_rate: u64,
        dry_run: bool,
    ) -> Result<Psbt, Error> {
        let fee_rate_checked = self.check_fee_rate(fee_rate)?;

        self.sync_wallet(
            txn,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
            false,
        )?;
        self.sync_wallet(
            txn,
            SyncOptions {
                keychain: SyncKeychain::Vanilla {
                    lookback: self.vanilla_sync_lookback(),
                },
                strategy: SyncStrategy::FastSync,
            },
            false,
        )?;

        let script_pubkey = self.get_script_pubkey(&address)?;

        let mut tx_builder = self.bdk_wallet_mut().build_tx();
        tx_builder
            .drain_wallet()
            .drain_to(script_pubkey)
            .fee_rate(fee_rate_checked);

        let psbt = tx_builder.finish().map_err(|e| match e {
            bdk_wallet::error::CreateTxError::CoinSelection(InsufficientFunds {
                needed,
                available,
            }) => Error::InsufficientBitcoins {
                needed: needed.to_sat(),
                available: available.to_sat(),
            },
            bdk_wallet::error::CreateTxError::OutputBelowDustLimit(_) => {
                Error::OutputBelowDustLimit
            }
            _ => Error::Internal {
                details: e.to_string(),
            },
        })?;

        if !dry_run {
            self.reserve_vanilla_txos(txn, &psbt, WalletTransactionType::Drain)?;
        }

        Ok(psbt)
    }

    fn drain_to_end_impl(
        &mut self,
        txn: &DbTxn,
        signed_psbt: &Psbt,
    ) -> Result<BdkTransaction, Error> {
        self.finalize_vanilla_wallet_transaction(txn, signed_psbt, WalletTransactionType::Drain)?;
        let tx = self.broadcast_psbt(txn, signed_psbt)?;
        Ok(tx)
    }

    fn get_hub_fail_status(&self, _batch_transfer_idx: i32) -> Result<bool, Error> {
        Ok(false)
    }

    fn set_hub_accept_status(&self, _batch_transfer_idx: i32) -> Result<Option<bool>, Error> {
        Ok(Some(true))
    }

    fn set_hub_fail_status(&self, _batch_transfer_idx: i32) -> Result<(), Error> {
        Ok(())
    }

    fn fail_batch_transfer(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<DbBatchTransfer, Error> {
        self.set_hub_fail_status(batch_transfer.idx)?;
        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        txn.update_batch_transfer(&mut updated_batch_transfer)
    }

    fn try_fail_batch_transfer(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<TryFailBatchTransferOutcome, Error> {
        let updated_batch_transfer =
            match self.refresh_transfer(txn, batch_transfer, db_data, &[], true) {
                Err(Error::MinFeeNotMet { txid: _ }) | Err(Error::MaxFeeExceeded { txid: _ }) => {
                    Ok(None)
                }
                Err(e) => Err(e),
                Ok(v) => Ok(v),
            }?;
        // fail transfer if the status didn't change after a refresh
        if updated_batch_transfer.is_none() {
            self.fail_batch_transfer(txn, batch_transfer)?;
            Ok(TryFailBatchTransferOutcome::Failed)
        } else {
            Ok(TryFailBatchTransferOutcome::Refreshed)
        }
    }

    fn fail_transfers_impl(
        &mut self,
        txn: &DbTxn,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
        skip_sync: bool,
    ) -> Result<FailTransfersOutcome, Error> {
        if !skip_sync {
            self.sync_wallet(
                txn,
                SyncOptions {
                    keychain: SyncKeychain::Colored,
                    strategy: SyncStrategy::FastSync,
                },
                false,
            )?;
        }

        let db_data = txn.get_db_data(false)?;
        let mut transfers_changed = false;
        let mut cannot_fail = false;

        if let Some(batch_transfer_idx) = batch_transfer_idx {
            let batch_transfer =
                txn.get_batch_transfer_or_fail(batch_transfer_idx, &db_data.batch_transfers)?;

            if !batch_transfer.is_fallible() {
                return Ok(FailTransfersOutcome {
                    transfers_changed: false,
                    cannot_fail: true,
                });
            }

            if no_asset_only {
                let asset_transfers = batch_transfer.get_asset_transfers(&db_data.asset_transfers);
                let connected_assets = asset_transfers.iter().any(|t| t.asset_id.is_some());
                if connected_assets {
                    return Ok(FailTransfersOutcome {
                        transfers_changed: false,
                        cannot_fail: true,
                    });
                }
            }

            transfers_changed = true;
            if let TryFailBatchTransferOutcome::Refreshed =
                self.try_fail_batch_transfer(txn, &batch_transfer, &db_data)?
            {
                cannot_fail = true;
            }
        } else {
            // fail all expired transfers that are in a fallible status
            let now = now().unix_timestamp();
            for batch_transfer in db_data.batch_transfers.iter().filter(|t| {
                let expired = t.expiration.unwrap_or(now) < now;
                expired && t.is_fallible()
            }) {
                if no_asset_only {
                    let connected_assets = batch_transfer
                        .get_asset_transfers(&db_data.asset_transfers)
                        .iter()
                        .any(|t| t.asset_id.is_some());
                    if connected_assets {
                        continue;
                    }
                }
                transfers_changed = true;
                self.try_fail_batch_transfer(txn, batch_transfer, &db_data)?;
            }
        }

        Ok(FailTransfersOutcome {
            transfers_changed,
            cannot_fail,
        })
    }

    fn wallet_specific_consistency_checks(&mut self, _txn: &DbTxn) -> Result<(), Error>;

    fn check_consistency(&mut self, txn: &DbTxn, runtime: &RgbRuntime) -> Result<(), Error> {
        info!(self.logger(), "Doing a consistency check...");

        self.wallet_specific_consistency_checks(txn)?;

        let asset_ids: Vec<String> = runtime
            .contracts()?
            .iter()
            .map(|c| c.id.to_string())
            .collect();
        let db_asset_ids: Vec<String> = txn.get_asset_ids()?;
        if !db_asset_ids.iter().all(|i| asset_ids.contains(i)) {
            return Err(Error::Inconsistency {
                details: s!("DB assets do not match with ones stored in RGB"),
            });
        }

        let medias = txn.iter_media()?;
        let media_dir = self.media_dir();
        for media in medias {
            if !media_dir.join(media.digest).exists() {
                return Err(Error::Inconsistency {
                    details: s!("DB media do not match with the ones stored in media directory"),
                });
            }
        }

        info!(self.logger(), "Consistency check completed");
        Ok(())
    }

    fn get_fee_estimation_impl(&self, blocks: u16) -> Result<f64, Error> {
        if !(MIN_BLOCK_ESTIMATION..=MAX_BLOCK_ESTIMATION).contains(&blocks) {
            return Err(Error::InvalidEstimationBlocks);
        }
        self.indexer().fee_estimation(blocks)
    }

    fn get_online_data(
        &self,
        online_options: &OnlineOptions,
    ) -> Result<(Online, OnlineData), Error> {
        let id = now().unix_timestamp_nanos() as u64;
        let online = Online { id };

        let (indexer, resolver) =
            get_indexer_and_resolver(&online_options.indexer_url, self.bitcoin_network())?;
        indexer.populate_tx_cache(self.bdk_wallet());

        let online_data = OnlineData {
            id: online.id,
            indexer_url: online_options.indexer_url.to_string(),
            indexer,
            resolver,
            hub_client: None,
            user_role: None,
            vanilla_sync_lookback: online_options.vanilla_sync_lookback,
        };

        Ok((online, online_data))
    }

    fn go_online_impl(&mut self, online_options: &OnlineOptions) -> Result<Online, Error> {
        let indexer_url = &online_options.indexer_url;
        let online = if let Some(online_data) = self.online_data().as_ref() {
            let online = Online { id: online_data.id };
            if online_data.indexer_url != *indexer_url {
                let (online, online_data) = self.get_online_data(online_options)?;
                *self.online_data_mut() = Some(online_data);
                info!(self.logger(), "Went online with new indexer URL");
                online
            } else {
                self.check_online(online)?;
                online
            }
        } else {
            let (online, online_data) = self.get_online_data(online_options)?;
            *self.online_data_mut() = Some(online_data);
            online
        };

        if !online_options.skip_consistency_check {
            let txn = self.database().begin_transaction()?;
            let runtime = self.rgb_runtime()?;
            self.check_consistency(&txn, &runtime)?;
            txn.commit()?;
        }

        Ok(online)
    }

    fn get_asset_medias(
        &self,
        txn: &DbTxn,
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
            let db_media = txn.get_media(media_idx)?.unwrap();
            asset_medias.push(Media::from_db_media(&db_media, self.media_dir()))
        }
        Ok(asset_medias)
    }

    fn get_signed_psbt(&self, transfer_dir: &Path) -> Result<Psbt, Error> {
        let psbt_file = transfer_dir.join(SIGNED_PSBT_FILE);
        let psbt_str = fs::read_to_string(psbt_file)?;
        Ok(Psbt::from_str(&psbt_str)?)
    }

    fn fail_batch_transfer_if_no_endpoints(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        transfer_transport_endpoints_data: &[(DbTransferTransportEndpoint, DbTransportEndpoint)],
    ) -> Result<Option<DbBatchTransfer>, Error> {
        if transfer_transport_endpoints_data.is_empty() {
            Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?))
        } else {
            Ok(None)
        }
    }

    fn refuse_consignment(
        &self,
        txn: &DbTxn,
        proxy_url: String,
        recipient_id: String,
        updated_batch_transfer: &mut DbBatchTransferActMod,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(
            self.logger(),
            "Refusing invalid consignment for {recipient_id}"
        );
        let proxy_client = ProxyClient::new(&proxy_url)?;
        match proxy_client.post_ack(&recipient_id, false) {
            Ok(r) => {
                debug!(self.logger(), "Consignment NACK response: {:?}", r);
            }
            Err(e) if e.to_string().contains("Cannot change ACK") => {
                warn!(self.logger(), "Found an ACK when trying NACK");
            }
            Err(e) => {
                error!(self.logger(), "Failed to post NACK: {e}");
                return Err(e);
            }
        };
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        Ok(Some(txn.update_batch_transfer(updated_batch_transfer)?))
    }

    fn get_consignment(
        &self,
        proxy_url: &str,
        recipient_id: String,
    ) -> Result<GetConsignmentResponse, Error> {
        let proxy_client = ProxyClient::new(proxy_url)?;
        let consignment_res = proxy_client.get_consignment(&recipient_id);
        if consignment_res.is_err() || consignment_res.as_ref().unwrap().result.as_ref().is_none() {
            debug!(
                self.logger(),
                "Consignment GET response error: {:?}", &consignment_res
            );
            return Err(Error::NoConsignment);
        }

        let consignment_res = consignment_res.unwrap().result.unwrap();
        #[cfg(test)]
        debug!(
            self.logger(),
            "Consignment GET response: {:?}", consignment_res
        );

        Ok(consignment_res)
    }

    fn extract_received_assignments(
        &self,
        consignment: &RgbTransfer,
        witness_id: RgbTxid,
        vout: Option<u32>,
        known_concealed: Option<SecretSeal>,
    ) -> HashMap<Opout, Assignment> {
        let mut received = HashMap::new();
        if let Some(bundle) = consignment
            .bundles
            .iter()
            .find(|ab| ab.witness_id() == witness_id)
        {
            for KnownTransition { transition, opid } in bundle.bundle.known_transitions.iter() {
                for (ass_type, typed_assigns) in transition.assignments.iter() {
                    for (no, fungible_assignment) in typed_assigns.as_fungible().iter().enumerate()
                    {
                        let opout = Opout::new(*opid, *ass_type, no as u16);
                        if let Assign::ConfidentialSeal { seal, state, .. } = fungible_assignment
                            && Some(*seal) == known_concealed
                        {
                            match *ass_type {
                                OS_ASSET => {
                                    received.insert(opout, Assignment::Fungible(state.as_u64()));
                                }
                                OS_INFLATION => {
                                    received
                                        .insert(opout, Assignment::InflationRight(state.as_u64()));
                                }
                                _ => {}
                            }
                        };
                        if let Assign::Revealed { seal, state, .. } = fungible_assignment
                            && seal.txid == TxPtr::WitnessTx
                            && Some(seal.vout.into_u32()) == vout
                        {
                            match *ass_type {
                                OS_ASSET => {
                                    received.insert(opout, Assignment::Fungible(state.as_u64()));
                                }
                                OS_INFLATION => {
                                    received
                                        .insert(opout, Assignment::InflationRight(state.as_u64()));
                                }
                                _ => {}
                            }
                        };
                    }
                    for (no, structured_assignment) in
                        typed_assigns.as_structured().iter().enumerate()
                    {
                        let opout = Opout::new(*opid, *ass_type, no as u16);
                        if let Assign::ConfidentialSeal { seal, .. } = structured_assignment
                            && Some(*seal) == known_concealed
                        {
                            received.insert(opout, Assignment::NonFungible);
                        }
                        if let Assign::Revealed { seal, .. } = structured_assignment
                            && seal.txid == TxPtr::WitnessTx
                            && Some(seal.vout.into_u32()) == vout
                        {
                            received.insert(opout, Assignment::NonFungible);
                        };
                    }
                }
            }
        }

        received
    }

    fn get_reject_list(
        &self,
        reject_list_url: &str,
    ) -> Result<(HashSet<Opout>, HashSet<Opout>), Error> {
        let reject_list_client = RejectListClient::new(reject_list_url)?;
        let list = reject_list_client.get_reject_list()?;
        let reject_list = list.trim();
        let mut opout_map = HashMap::with_capacity(reject_list.lines().count());
        for line in reject_list.lines() {
            let (is_allow, opout_str) = line.strip_prefix("!").map_or((false, line), |s| (true, s));
            let opout = match Opout::from_str(opout_str) {
                Ok(o) => o,
                Err(_) => {
                    warn!(
                        self.logger(),
                        "Ignoring invalid opout in reject list: {line}"
                    );
                    continue;
                }
            };
            opout_map.insert(opout, is_allow);
        }
        let (allow_opouts, reject_opouts) = opout_map.into_iter().fold(
            (HashSet::new(), HashSet::new()),
            |(mut allow, mut reject), (o, allowed)| {
                if allowed {
                    allow.insert(o);
                } else {
                    reject.insert(o);
                }
                (allow, reject)
            },
        );

        Ok((reject_opouts, allow_opouts))
    }

    fn extract_attachments(
        &self,
        valid_contract: &ValidContract,
        asset_schema: AssetSchema,
    ) -> Vec<Attachment> {
        let mut attachments = vec![];
        match asset_schema {
            AssetSchema::Nia => {
                let contract_data = valid_contract.contract_data();
                let contract = NiaWrapper::with(contract_data);
                if let Some(attachment) = contract.contract_terms().media {
                    attachments.push(attachment)
                }
            }
            AssetSchema::Uda => {
                let contract_data = valid_contract.contract_data();
                let contract = UdaWrapper::with(contract_data);
                let token_data = contract.token_data();
                if let Some(media) = token_data.media {
                    attachments.push(media)
                }
                attachments.extend(
                    token_data
                        .attachments
                        .to_unconfined()
                        .values()
                        .cloned()
                        .collect::<Vec<Attachment>>(),
                )
            }
            AssetSchema::Cfa => {
                let contract_data = valid_contract.contract_data();
                let contract = CfaWrapper::with(contract_data);
                if let Some(attachment) = contract.contract_terms().media {
                    attachments.push(attachment)
                }
            }
            AssetSchema::Ifa => {
                let contract_data = valid_contract.contract_data();
                let contract = IfaWrapper::with(contract_data);
                if let Some(attachment) = contract.contract_terms().media {
                    attachments.push(attachment)
                }
            }
        };
        attachments
    }

    fn safe_height(&self, min_confirmations: u8) -> Result<u32, Error> {
        Ok(self
            .indexer()
            .get_latest_block_height()?
            .saturating_sub(min_confirmations as u32)
            + 1)
    }

    fn collect_unsafe_history_txids(warnings: &[Warning], exclude_txid: &str) -> HashSet<String> {
        let mut txids = HashSet::new();
        for warning in warnings {
            if let Warning::UnsafeHistory(unsafe_history) = warning {
                for other_txids in unsafe_history.values() {
                    for txid in other_txids {
                        let txid = txid.to_string();
                        if txid != *exclude_txid {
                            txids.insert(txid);
                        }
                    }
                }
            }
        }
        txids
    }

    fn ack_consignment(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        recipient_id: String,
        updated_batch_transfer: &mut DbBatchTransferActMod,
        proxy_url: String,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger(), "ACKing consignment...");

        match self.set_hub_accept_status(batch_transfer.idx)? {
            Some(true) => {}
            Some(false) => return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?)),
            None => return Ok(None),
        }

        let proxy_client = ProxyClient::new(&proxy_url)?;
        match proxy_client.post_ack(&recipient_id, true) {
            Ok(r) => {
                debug!(self.logger(), "Consignment ACK response: {:?}", r);
            }
            Err(e) if e.to_string().contains("Cannot change ACK") => {
                warn!(self.logger(), "Found an NACK when trying ACK");
            }
            Err(e) => {
                error!(self.logger(), "Failed to post ACK: {e}");
                return Err(e);
            }
        };

        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::WaitingConfirmations);

        Ok(Some(txn.update_batch_transfer(updated_batch_transfer)?))
    }

    fn wait_consignment(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger(), "Waiting consignment...");

        let (asset_transfer, transfer) =
            batch_transfer.get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;
        let recipient_id = transfer
            .recipient_id
            .clone()
            .expect("transfer should have a recipient ID");
        debug!(self.logger(), "Recipient ID: {recipient_id}");

        let tte_data = txn.get_transfer_transport_endpoints_data(transfer.idx)?;
        if let Some(updated_transfer) =
            self.fail_batch_transfer_if_no_endpoints(txn, batch_transfer, &tte_data)?
        {
            return Ok(Some(updated_transfer));
        }

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();

        // if we already downloaded the consignment and its metadata in a
        // previous attempt that failed during validation for a transient
        // reason (e.g. network error), reuse them instead of hitting the proxy
        // again; the endpoint we used is recoverable from the DB via the
        // `used` flag on the transfer transport endpoint
        let consignment_path = self.get_receive_consignment_path(&recipient_id);
        let consignment_meta_path = self.get_receive_consignment_meta_path(&recipient_id);
        let (proxy_url, txid, vout) = if consignment_path.exists()
            && consignment_meta_path.exists()
            && let Some(cached_proxy_url) = tte_data
                .iter()
                .find(|(tte, _)| tte.used)
                .map(|(_, te)| te.endpoint.clone())
            && let Ok(meta_str) = fs::read_to_string(&consignment_meta_path)
            && let Ok(meta) = serde_json::from_str::<ReceivedConsignmentMeta>(&meta_str)
        {
            debug!(
                self.logger(),
                "Reusing previously-downloaded consignment for {recipient_id}"
            );
            (cached_proxy_url, meta.txid, meta.vout)
        } else {
            // download consignment and its metadata
            let mut proxy_res = None;
            for (transfer_transport_endpoint, transport_endpoint) in tte_data {
                let result = match self
                    .get_consignment(&transport_endpoint.endpoint, recipient_id.clone())
                {
                    Err(Error::NoConsignment) => {
                        info!(
                            self.logger(),
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
                txn.update_transfer_transport_endpoint(&mut updated_transfer_transport_endpoint)?;
                break;
            }
            let (consignment_b64, proxy_url, txid, vout) = if let Some(res) = proxy_res {
                (res.0, res.1, res.2, res.3)
            } else {
                return Ok(None);
            };

            // write consignment
            let transfer_dir = consignment_path.parent().unwrap();
            fs::create_dir_all(transfer_dir)?;
            let consignment_bytes = match general_purpose::STANDARD.decode(consignment_b64) {
                Ok(b) => b,
                Err(e) => {
                    error!(self.logger(), "Failed to decode consignment bytes: {e}");
                    return self.refuse_consignment(
                        txn,
                        proxy_url,
                        recipient_id,
                        &mut updated_batch_transfer,
                    );
                }
            };
            fs::write(&consignment_path, consignment_bytes).expect("Unable to write file");

            // write consignment metadata
            let meta = ReceivedConsignmentMeta {
                txid: txid.clone(),
                vout,
            };
            let meta_str = serde_json::to_string(&meta).map_err(InternalError::from)?;
            fs::write(&consignment_meta_path, meta_str)?;

            (proxy_url, txid, vout)
        };

        let mut runtime = self.rgb_runtime()?;
        let consignment = match RgbTransfer::load_file(&consignment_path) {
            Ok(c) => c,
            Err(e) => {
                error!(self.logger(), "Failed to load consignment file: {e}");
                return self.refuse_consignment(
                    txn,
                    proxy_url,
                    recipient_id,
                    &mut updated_batch_transfer,
                );
            }
        };
        let contract_id = consignment.contract_id();
        let asset_id = contract_id.to_string();
        let asset_schema: AssetSchema = consignment.schema_id().try_into()?;

        // check if the received schema is supported
        if !self.supports_schema(&asset_schema) {
            error!(
                self.logger(),
                "The wallet doesn't support the provided schema: {}", asset_schema
            );
            return self.refuse_consignment(
                txn,
                proxy_url,
                recipient_id,
                &mut updated_batch_transfer,
            );
        }

        // check if DB transfer is connected to an asset
        if let Some(aid) = asset_transfer.asset_id.clone() {
            // check if asset transfer is connected to the asset we are actually receiving
            if aid != asset_id {
                error!(
                    self.logger(),
                    "Received a different asset than the expected one"
                );
                return self.refuse_consignment(
                    txn,
                    proxy_url,
                    recipient_id,
                    &mut updated_batch_transfer,
                );
            }
        }

        // check if the TXID posted to the proxy is valid
        let witness_id = match RgbTxid::from_str(&txid) {
            Ok(txid) => txid,
            Err(_) => {
                error!(self.logger(), "Received an invalid TXID from the proxy");
                return self.refuse_consignment(
                    txn,
                    proxy_url,
                    recipient_id,
                    &mut updated_batch_transfer,
                );
            }
        };

        // validate consignment
        debug!(self.logger(), "Validating consignment...");
        let safe_height = NonZeroU32::new(self.safe_height(batch_transfer.min_confirmations)?);
        let validation_config = ValidationConfig {
            chain_net: self.chain_net(),
            trusted_typesystem: asset_schema.types(),
            build_opouts_dag: true,
            safe_height,
        };
        let resolver = OffchainResolver {
            witness_id,
            consignment: &consignment,
            fallback: self.blockchain_resolver(),
        };
        let valid_consignment = match consignment.clone().validate(&resolver, &validation_config) {
            Ok(consignment) => consignment,
            Err(ValidationError::InvalidConsignment(e)) => {
                error!(self.logger(), "Consignment is invalid: {}", e);
                return self.refuse_consignment(
                    txn,
                    proxy_url,
                    recipient_id,
                    &mut updated_batch_transfer,
                );
            }
            Err(ValidationError::ResolverError(e)) => {
                warn!(self.logger(), "Network error during consignment validation");
                return Err(Error::Network {
                    details: e.to_string(),
                });
            }
        };
        let validation_status = valid_consignment.validation_status();
        let validity = validation_status.validity();
        debug!(self.logger(), "Consignment validity: {:?}", validity);

        // find the bundle for the witness ID posted on the proxy
        let Some(anchored_bundle) = consignment
            .bundles
            .iter()
            .find(|ab| ab.witness_id() == witness_id)
        else {
            error!(
                self.logger(),
                "Cannot find the provided TXID in the consignment"
            );
            return self.refuse_consignment(
                txn,
                proxy_url,
                recipient_id,
                &mut updated_batch_transfer,
            );
        };

        // check the info provided via the proxy is correct
        if let Some(RecipientTypeFull::Witness { .. }) = transfer.recipient_type {
            if let Some(vout) = vout {
                if let PubWitness::Tx(tx) = &anchored_bundle.pub_witness {
                    if let Some(output) = tx.output.get(vout as usize) {
                        let script_pubkey =
                            script_buf_from_recipient_id(recipient_id.clone())?.unwrap();
                        if output.script_pubkey != script_pubkey {
                            error!(
                                self.logger(),
                                "The provided vout pays an incorrect script pubkey"
                            );
                            return self.refuse_consignment(
                                txn,
                                proxy_url,
                                recipient_id,
                                &mut updated_batch_transfer,
                            );
                        }
                    } else {
                        error!(self.logger(), "Cannot find the expected outpoint");
                        return self.refuse_consignment(
                            txn,
                            proxy_url,
                            recipient_id,
                            &mut updated_batch_transfer,
                        );
                    }
                } else {
                    error!(self.logger(), "Consignment is missing the witness TX");
                    return self.refuse_consignment(
                        txn,
                        proxy_url,
                        recipient_id,
                        &mut updated_batch_transfer,
                    );
                }
            } else {
                error!(
                    self.logger(),
                    "The vout should be provided when receiving via witness"
                );
                return self.refuse_consignment(
                    txn,
                    proxy_url,
                    recipient_id,
                    &mut updated_batch_transfer,
                );
            }
        }

        let known_concealed = if let Some(RecipientTypeFull::Blind { .. }) = transfer.recipient_type
        {
            let beneficiary = XChainNet::<Beneficiary>::from_str(&recipient_id)
                .expect("saved recipient ID is invalid");
            match beneficiary.into_inner() {
                Beneficiary::BlindedSeal(secret_seal) => Some(secret_seal),
                _ => unreachable!("beneficiary is blinded"),
            }
        } else {
            None
        };
        let receiving =
            self.extract_received_assignments(&consignment, witness_id, vout, known_concealed);
        if receiving.is_empty() {
            error!(self.logger(), "Cannot find any receiving assignment");
            return self.refuse_consignment(
                txn,
                proxy_url,
                recipient_id,
                &mut updated_batch_transfer,
            );
        };

        if asset_schema == AssetSchema::Ifa {
            let url = if let Ok(ass) = txn.check_asset_exists(asset_id.clone()) {
                ass.reject_list_url
            } else {
                let contract = IfaWrapper::with(valid_consignment.contract_data());
                contract.reject_list_url().map(|u| u.to_string())
            };
            if let Some(url) = &url {
                let (reject_opouts, allow_opouts) = self.get_reject_list(url)?;

                let to_reject = self.check_dag(
                    validation_status
                        .dag_data_opt
                        .as_ref()
                        .expect("build_opouts_dag is true"),
                    &reject_opouts,
                    &allow_opouts,
                    &receiving.clone().into_keys().collect(),
                )?;

                if !to_reject.is_empty() {
                    error!(
                        self.logger(),
                        "Found {} opout(s) that must be rejected",
                        to_reject.len()
                    );
                    return self.refuse_consignment(
                        txn,
                        proxy_url,
                        recipient_id,
                        &mut updated_batch_transfer,
                    );
                } else {
                    info!(
                        self.logger(),
                        "Didn't find any opout(s) that should be rejected"
                    );
                }
            }
        }

        if asset_transfer.asset_id.is_none() {
            if txn.check_asset_exists(asset_id.clone()).is_err() {
                // unknown asset
                debug!(self.logger(), "Receiving unknown contract...");
                let valid_contract = valid_consignment.clone().into_valid_contract();

                let attachments = self.extract_attachments(&valid_contract, asset_schema);
                let mut saved_media_paths = vec![];
                for attachment in attachments {
                    let digest = hex::encode(attachment.digest);
                    let media_path = self.media_dir().join(&digest);
                    // download media only if file not already present
                    if !media_path.exists() {
                        let proxy_client = ProxyClient::new(&proxy_url)?;
                        let media_res = proxy_client.get_media(&digest)?;
                        #[cfg(test)]
                        debug!(self.logger(), "Media GET response: {:?}", media_res);
                        if let Some(media_res) = media_res.result {
                            let file_bytes = general_purpose::STANDARD
                                .decode(media_res)
                                .map_err(InternalError::from)?;
                            let actual_digest = hash_bytes_hex(&file_bytes);
                            if digest != actual_digest {
                                error!(
                                    self.logger(),
                                    "Attached file has a different hash than the one in the contract"
                                );
                                return self.refuse_consignment(
                                    txn,
                                    proxy_url,
                                    recipient_id,
                                    &mut updated_batch_transfer,
                                );
                            }
                            fs::write(&media_path, file_bytes)?;
                            saved_media_paths.push(media_path);
                        } else {
                            error!(
                                self.logger(),
                                "Cannot find the media file but the contract defines one"
                            );
                            for path in saved_media_paths {
                                fs::remove_file(path)?;
                            }
                            return self.refuse_consignment(
                                txn,
                                proxy_url,
                                recipient_id,
                                &mut updated_batch_transfer,
                            );
                        }
                    }
                }

                runtime
                    .import_contract(valid_contract.clone(), self.blockchain_resolver())
                    .expect("failure importing received contract");
                debug!(self.logger(), "Contract registered");
                self.save_new_asset_internal(
                    txn,
                    &runtime,
                    contract_id,
                    asset_schema,
                    valid_contract,
                    Some(valid_consignment.clone()),
                )?;
            }

            // add asset info to transfer if missing
            let mut updated_asset_transfer: DbAssetTransferActMod = asset_transfer.clone().into();
            updated_asset_transfer.asset_id = ActiveValue::Set(Some(asset_id.clone()));
            txn.update_asset_transfer(&mut updated_asset_transfer)?;
        }

        // save validated consignment
        let valid_consignment_path = self.get_receive_valid_consignment_path(&consignment_path);
        valid_consignment.save_file(&valid_consignment_path)?;

        debug!(
            self.logger(),
            "Consignment is valid. Receiving '{:?}' of contract '{}'", receiving, asset_id
        );

        updated_batch_transfer.txid = ActiveValue::Set(Some(txid.clone()));

        let utxo_idx = match transfer.recipient_type {
            Some(RecipientTypeFull::Blind { ref unblinded_utxo }) => {
                txn.get_txo(unblinded_utxo)?.expect("utxo must exist").idx
            }
            Some(RecipientTypeFull::Witness { .. }) => {
                let mut updated_transfer: DbTransferActMod = transfer.clone().into();
                updated_transfer.recipient_type =
                    ActiveValue::Set(Some(RecipientTypeFull::Witness { vout }));
                txn.update_transfer(&mut updated_transfer)?;
                let db_utxo = DbTxoActMod {
                    txid: ActiveValue::Set(txid.clone()),
                    vout: ActiveValue::Set(vout.unwrap()),
                    btc_amount: ActiveValue::Set(s!("0")),
                    spent: ActiveValue::Set(false),
                    exists: ActiveValue::Set(false),
                    pending_witness: ActiveValue::Set(true),
                    ..Default::default()
                };
                txn.set_txo(db_utxo)?
            }
            _ => return Err(InternalError::Unexpected.into()),
        };
        for assignment in receiving.into_values() {
            let db_coloring = DbColoringActMod {
                txo_idx: ActiveValue::Set(utxo_idx),
                asset_transfer_idx: ActiveValue::Set(asset_transfer.idx),
                r#type: ActiveValue::Set(ColoringType::Receive),
                assignment: ActiveValue::Set(assignment),
                ..Default::default()
            };
            txn.set_coloring(db_coloring)?;
        }

        // if the consignment contains unsafe history set status to WaitingSafeHeight and stop here
        if validation_status.validity() == Validity::Warnings {
            let unsafe_txids =
                Self::collect_unsafe_history_txids(&validation_status.warnings, &txid);
            if !unsafe_txids.is_empty() {
                warn!(
                    self.logger(),
                    "Unsafe history detected in consignment: {unsafe_txids:?}"
                );
                updated_batch_transfer.status = ActiveValue::Set(TransferStatus::WaitingSafeHeight);
                return Ok(Some(
                    txn.update_batch_transfer(&mut updated_batch_transfer)?,
                ));
            }
        }

        self.ack_consignment(
            txn,
            batch_transfer,
            recipient_id,
            &mut updated_batch_transfer,
            proxy_url,
        )
    }

    fn wait_safe_height(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger(), "Waiting safe height...");

        let (_, transfer) =
            batch_transfer.get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;
        let recipient_id = transfer
            .recipient_id
            .clone()
            .expect("transfer should have a recipient ID");
        let consignment_path = self.get_receive_consignment_path(&recipient_id);
        let valid_consignment_path = self.get_receive_valid_consignment_path(&consignment_path);
        let valid_consignment =
            ValidTransfer::load_file(&valid_consignment_path).map_err(InternalError::from)?;
        let validation_status = valid_consignment.validation_status();
        let txid = batch_transfer
            .txid
            .as_deref()
            .expect("batch_transfer in WaitingSafeHeight must have a txid");
        let unsafe_txids = Self::collect_unsafe_history_txids(&validation_status.warnings, txid);

        if !unsafe_txids.is_empty() {
            let safe_height = self.safe_height(batch_transfer.min_confirmations)?;
            for txid in unsafe_txids {
                let Some(tx_height) = self.tx_height(txid)? else {
                    return Ok(None);
                };
                if tx_height > safe_height {
                    return Ok(None);
                }
            }
        }

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        let tte_data = txn.get_transfer_transport_endpoints_data(transfer.idx)?;
        let (_, transport_endpoint) = tte_data
            .into_iter()
            .find(|(tte, _)| tte.used)
            .expect("there should be 1 used TTE");
        self.ack_consignment(
            txn,
            batch_transfer,
            recipient_id,
            &mut updated_batch_transfer,
            transport_endpoint.endpoint,
        )
    }

    fn wait_ack(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger(), "Waiting ACK...");

        let mut batch_transfer_data =
            batch_transfer.get_transfers(&db_data.asset_transfers, &db_data.transfers)?;
        for asset_transfer_data in batch_transfer_data.asset_transfers_data.iter_mut() {
            for transfer in asset_transfer_data.transfers.iter_mut() {
                if transfer.ack.is_some() {
                    continue;
                }
                let tte_data = txn.get_transfer_transport_endpoints_data(transfer.idx)?;
                if let Some(updated_transfer) =
                    self.fail_batch_transfer_if_no_endpoints(txn, batch_transfer, &tte_data)?
                {
                    return Ok(Some(updated_transfer));
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
                debug!(self.logger(), "Recipient ID: {recipient_id}");
                let proxy_client = ProxyClient::new(&proxy_url)?;
                let ack_res = proxy_client.get_ack(&recipient_id)?;
                debug!(
                    self.logger(),
                    "Consignment ACK/NACK response: {:?}", ack_res
                );

                if ack_res.result.is_some() {
                    let mut updated_transfer: DbTransferActMod = transfer.clone().into();
                    updated_transfer.ack = ActiveValue::Set(ack_res.result);
                    txn.update_transfer(&mut updated_transfer)?;
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
            return Ok(Some(
                self.fail_batch_transfer(txn, batch_transfer)
                    .map_err(|e| match e {
                        Error::MultisigTransferStatusMismatch => Error::MultisigUnexpectedData {
                            details: s!("hub reports accepted=true but recipient sent a NACK"),
                        },
                        other => other,
                    })?,
            ));
        } else if batch_transfer_transfers.iter().all(|t| t.ack == Some(true)) {
            match self.set_hub_accept_status(batch_transfer.idx)? {
                Some(true) => {}
                Some(false) => return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?)),
                None => return Ok(None),
            }
            let txid = batch_transfer
                .txid
                .as_ref()
                .expect("batch transfer should have a TXID");
            let transfer_dir = self.get_transfers_dir().join(txid);
            let signed_psbt = self.get_signed_psbt(&transfer_dir)?;
            let mut runtime = self.rgb_runtime()?;
            let fascia_path = transfer_dir.join(FASCIA_FILE);
            let fascia_str = fs::read_to_string(fascia_path)?;
            let fascia: Fascia = serde_json::from_str(&fascia_str).map_err(InternalError::from)?;
            self.broadcast_and_update_rgb(txn, &mut runtime, &signed_psbt, fascia)?;
            updated_batch_transfer.status = ActiveValue::Set(TransferStatus::WaitingConfirmations);
        } else {
            return Ok(None);
        }

        Ok(Some(
            txn.update_batch_transfer(&mut updated_batch_transfer)?,
        ))
    }

    fn tx_height(&self, txid: String) -> Result<Option<u32>, Error> {
        let txid = RgbTxid::from_str(&txid).map_err(|_| Error::InvalidTxid)?;
        Ok(
            match self
                .blockchain_resolver()
                .resolve_witness(txid)
                .map_err(|e| Error::Network {
                    details: e.to_string(),
                })? {
                WitnessStatus::Resolved(_, WitnessOrd::Mined(witness_pos)) => {
                    Some(witness_pos.height().get())
                }
                _ => None,
            },
        )
    }

    fn wait_confirmations(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
        incoming: bool,
        skip_sync: bool,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger(), "Waiting confirmations...");
        let txid = batch_transfer
            .txid
            .clone()
            .expect("batch transfer should have a TXID");
        debug!(
            self.logger(),
            "Getting details of transaction with ID '{}'...", txid
        );
        let confirmations = self.indexer().get_tx_confirmations(&txid)?;
        debug!(self.logger(), "Confirmations: {:?}", confirmations);

        if let Some(confirmations) = confirmations {
            if confirmations < batch_transfer.min_confirmations as u64 {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }

        if incoming {
            let (asset_transfer, transfer) = batch_transfer
                .get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;
            let recipient_id = transfer
                .clone()
                .recipient_id
                .expect("transfer should have a recipient ID");
            debug!(self.logger(), "Recipient ID: {recipient_id}");

            if let Some(RecipientTypeFull::Witness { vout }) = transfer.recipient_type {
                if !skip_sync {
                    self.sync_wallet(
                        txn,
                        SyncOptions {
                            keychain: SyncKeychain::Colored,
                            strategy: SyncStrategy::FastSync,
                        },
                        false,
                    )?;
                }
                let outpoint = Outpoint {
                    txid: txid.clone(),
                    vout: vout.unwrap(),
                };
                let txo = txn.get_txo(&outpoint)?.expect("txo must exist");
                let mut txo: DbTxoActMod = txo.into();
                txo.pending_witness = ActiveValue::Set(false);
                txn.update_txo(txo)?;
            }

            // accept consignment
            let consignment_path = self.get_receive_consignment_path(&recipient_id);
            let valid_consignment_path = self.get_receive_valid_consignment_path(&consignment_path);
            let valid_consignment =
                ValidTransfer::load_file(&valid_consignment_path).map_err(InternalError::from)?;
            let mut runtime = self.rgb_runtime()?;
            runtime.accept_transfer(valid_consignment.clone(), self.blockchain_resolver())?;
            let asset_schema: AssetSchema = valid_consignment.schema_id().try_into()?;
            if asset_schema == AssetSchema::Ifa {
                let contract_id = valid_consignment.contract_id();
                let contract_wrapper =
                    runtime.contract_wrapper::<InflatableFungibleAsset>(contract_id)?;
                let known_circulating_supply = contract_wrapper.total_issued_supply().into();
                let asset_id = asset_transfer.asset_id.unwrap();
                let db_asset = txn.get_asset(asset_id).unwrap().unwrap();
                let db_known_circulating_supply = db_asset
                    .known_circulating_supply
                    .as_ref()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap();
                if db_known_circulating_supply < known_circulating_supply {
                    let mut updated_asset: DbAssetActMod = db_asset.into();
                    updated_asset.known_circulating_supply =
                        ActiveValue::Set(Some(known_circulating_supply.to_string()));
                    txn.update_asset(&mut updated_asset)?;
                }
            }
        }

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Settled);
        Ok(Some(
            txn.update_batch_transfer(&mut updated_batch_transfer)?,
        ))
    }

    fn wait_counterparty(
        &mut self,
        txn: &DbTxn,
        transfer: &DbBatchTransfer,
        db_data: &DbData,
        incoming: bool,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        if incoming {
            self.wait_consignment(txn, transfer, db_data)
        } else {
            self.wait_ack(txn, transfer, db_data)
        }
    }

    fn refresh_transfer(
        &mut self,
        txn: &DbTxn,
        transfer: &DbBatchTransfer,
        db_data: &DbData,
        filter: &[RefreshFilter],
        skip_sync: bool,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger(), "Refreshing transfer: {:?}", transfer);
        let incoming = transfer.incoming(&db_data.asset_transfers, &db_data.transfers);
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
                if self.get_hub_fail_status(transfer.idx)? {
                    return Ok(Some(self.fail_batch_transfer(txn, transfer)?));
                }
                self.wait_counterparty(txn, transfer, db_data, incoming)
            }
            TransferStatus::WaitingSafeHeight => self.wait_safe_height(txn, transfer, db_data),
            TransferStatus::WaitingConfirmations => {
                self.wait_confirmations(txn, transfer, db_data, incoming, skip_sync)
            }
            _ => Ok(None),
        }
    }

    fn refresh_impl(
        &mut self,
        txn: &DbTxn,
        asset_id: Option<String>,
        filter: Vec<RefreshFilter>,
        skip_sync: bool,
    ) -> Result<RefreshResult, Error> {
        let mut db_data = txn.get_db_data(false)?;

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
        db_data.batch_transfers.retain(|t| t.waiting());

        let mut refresh_result = HashMap::new();
        for transfer in &db_data.batch_transfers {
            let mut failure = None;
            let mut updated_status = None;
            match self.refresh_transfer(txn, transfer, &db_data, &filter, skip_sync) {
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

        Ok(refresh_result)
    }

    fn select_rgb_inputs(
        &self,
        asset_id: String,
        assignments_needed: &AssignmentsCollection,
        unspents: Vec<LocalUnspent>,
    ) -> Result<AssetSpend, Error> {
        // sort unspents by the sum of main amounts
        fn cmp_localunspent_allocation_sum(a: &LocalUnspent, b: &LocalUnspent) -> Ordering {
            let a_sum: u64 = a
                .rgb_allocations
                .iter()
                .map(|a| a.assignment.main_amount())
                .sum();
            let b_sum: u64 = b
                .rgb_allocations
                .iter()
                .map(|a| a.assignment.main_amount())
                .sum();
            a_sum.cmp(&b_sum)
        }
        // sort unspents by the sum of inflation right amounts
        fn cmp_localunspent_inflation_sum(a: &LocalUnspent, b: &LocalUnspent) -> Ordering {
            let a_sum: u64 = a
                .rgb_allocations
                .iter()
                .map(|a| a.assignment.inflation_amount())
                .sum();
            let b_sum: u64 = b
                .rgb_allocations
                .iter()
                .map(|a| a.assignment.inflation_amount())
                .sum();
            a_sum.cmp(&b_sum)
        }

        debug!(
            self.logger(),
            "Selecting inputs for asset '{}'...", asset_id
        );
        let mut input_outpoints: Vec<Outpoint> = Vec::new();

        let mut mut_unspents = unspents;

        // sort unspents first by inflation rights amount, then main amount
        if assignments_needed.inflation > 0 {
            mut_unspents.sort_by(cmp_localunspent_inflation_sum);
        }
        if assignments_needed.fungible > 0 {
            mut_unspents.sort_by(cmp_localunspent_allocation_sum);
        }

        let mut assignments_collected = AssignmentsCollection::default();
        let mut input_btc_amt = 0;
        for unspent in mut_unspents {
            // get spendable allocations for the required asset
            let asset_allocations: Vec<LocalRgbAllocation> = unspent
                .rgb_allocations
                .into_iter()
                .filter(|a| a.asset_id == Some(asset_id.clone()) && a.status.settled())
                .collect();

            // skip UTXOs with no allocations
            if asset_allocations.is_empty() {
                continue;
            }

            // check if the unspent hosts any needed allocations
            let mut needed = false;
            if assignments_collected.fungible < assignments_needed.fungible
                && asset_allocations
                    .iter()
                    .any(|a| matches!(a.assignment, Assignment::Fungible(_)))
            {
                needed = true;
            }
            if !assignments_collected.non_fungible & assignments_needed.non_fungible
                && asset_allocations
                    .iter()
                    .any(|a| matches!(a.assignment, Assignment::NonFungible))
            {
                needed = true;
            }
            if assignments_collected.inflation < assignments_needed.inflation
                && asset_allocations
                    .iter()
                    .any(|a| matches!(a.assignment, Assignment::InflationRight(_)))
            {
                needed = true;
            }
            // skip UTXOs with no needed allocations
            if !needed {
                continue;
            }

            // add selected allocations to collected assignments
            asset_allocations
                .iter()
                .for_each(|a| a.assignment.add_to_assignments(&mut assignments_collected));
            input_outpoints.push(unspent.utxo.outpoint());

            input_btc_amt += unspent.utxo.btc_amount.parse::<u64>().unwrap();

            // stop as soon as we have the needed assignments
            if assignments_collected.enough(assignments_needed) {
                break;
            }
        }
        if !assignments_collected.enough(assignments_needed) {
            return Err(Error::InsufficientAssignments {
                asset_id,
                available: assignments_collected,
            });
        }

        debug!(
            self.logger(),
            "Asset input assignments {:?}", assignments_collected
        );
        Ok(AssetSpend {
            input_outpoints,
            assignments_collected,
            input_btc_amt,
        })
    }

    fn prepare_psbt(
        &mut self,
        input_outpoints: HashSet<BdkOutPoint>,
        witness_recipients: &Vec<(ScriptBuf, u64)>,
        fee_rate: FeeRate,
    ) -> Result<(Psbt, Option<BtcChange>), Error> {
        let change_addr = self.get_new_address()?.script_pubkey();
        let mut builder = self.bdk_wallet_mut().build_tx();
        builder
            .add_data(&[0; 32])
            .add_utxos(&input_outpoints.into_iter().collect::<Vec<_>>())
            .map_err(InternalError::from)?
            .manually_selected_only()
            .fee_rate(fee_rate)
            .ordering(bdk_wallet::tx_builder::TxOrdering::Untouched);
        for (script_buf, amount_sat) in witness_recipients {
            builder.add_recipient(script_buf.clone(), BdkAmount::from_sat(*amount_sat));
        }
        builder.drain_to(change_addr.clone());

        let psbt = builder.finish().map_err(|e| match e {
            bdk_wallet::error::CreateTxError::CoinSelection(InsufficientFunds {
                needed,
                available,
            }) => Error::InsufficientBitcoins {
                needed: needed.to_sat(),
                available: available.to_sat(),
            },
            bdk_wallet::error::CreateTxError::OutputBelowDustLimit(_) => {
                Error::OutputBelowDustLimit
            }
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
                amount: o.value.to_sat(),
            });

        Ok((psbt, btc_change))
    }

    fn try_prepare_psbt(
        &mut self,
        input_unspents: &[LocalUnspent],
        all_inputs: &mut HashSet<BdkOutPoint>,
        witness_recipients: &Vec<(ScriptBuf, u64)>,
        fee_rate: FeeRate,
    ) -> Result<(Psbt, Option<BtcChange>), Error> {
        Ok(loop {
            break match self.prepare_psbt(all_inputs.clone(), witness_recipients, fee_rate) {
                Ok(res) => res,
                Err(Error::InsufficientBitcoins { .. }) => {
                    let used_txos: Vec<Outpoint> =
                        all_inputs.clone().into_iter().map(|o| o.into()).collect();
                    let mut free_utxos = self.get_available_allocations(
                        input_unspents,
                        used_txos.as_slice(),
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
                        all_inputs.insert(a.utxo.into());
                        continue;
                    }
                    return Err(Error::InsufficientAllocationSlots);
                }
                Err(e) => return Err(e),
            };
        })
    }

    fn get_beneficiary_seal(
        &self,
        local_recipient_data: &LocalRecipientData,
    ) -> BuilderSeal<GraphSeal> {
        match local_recipient_data {
            LocalRecipientData::Blind(secret_seal) => BuilderSeal::Concealed(*secret_seal),
            LocalRecipientData::Witness(witness_data) => {
                let graph_seal = if let Some(blinding) = witness_data.blinding {
                    GraphSeal::with_blinded_vout(witness_data.vout, blinding)
                } else {
                    GraphSeal::new_random_vout(witness_data.vout)
                };
                BuilderSeal::Revealed(graph_seal)
            }
        }
    }

    fn get_change_seal(
        &self,
        txn: &DbTxn,
        btc_change: &Option<BtcChange>,
        change_utxo_option: &mut Option<DbTxo>,
        input_outpoints: &[Outpoint],
        unspents: &[LocalUnspent],
    ) -> Result<BlindSeal<TxPtr>, Error> {
        Ok(if let Some(btc_change) = btc_change {
            GraphSeal::new_random_vout(btc_change.vout)
        } else {
            if change_utxo_option.is_none() {
                let change_utxo =
                    self.get_utxo(txn, input_outpoints, Some(unspents), true, None)?;
                debug!(
                    self.logger(),
                    "Change outpoint '{}'",
                    change_utxo.outpoint().to_string()
                );
                *change_utxo_option = Some(change_utxo);
            }
            let change_utxo = change_utxo_option.clone().unwrap();
            let blind_seal = self.get_blind_seal(change_utxo).transmutate();
            GraphSeal::from(blind_seal)
        })
    }

    fn check_dag(
        &self,
        dag_data: &OpoutsDagData,
        reject_opouts: &HashSet<Opout>,
        allow_opouts: &HashSet<Opout>,
        check_opouts: &HashSet<Opout>,
    ) -> Result<HashSet<Opout>, Error> {
        let (dag, index) = dag_data;
        let mut to_reject = HashSet::new();

        // for each opout we are checking, traverse its ancestor chain
        for check_opout in check_opouts {
            let &opout_node = index.get(check_opout).ok_or(Error::Internal {
                details: s!("opout not found in DAG"),
            })?;

            // traverse from this node to its ancestors, depth first
            let mut stack = vec![opout_node];
            let mut visited = HashSet::new();
            while let Some(node) = stack.pop() {
                if !visited.insert(node) {
                    continue;
                }
                let node_opout = &dag[node];
                // allow shields this path upward: do not traverse this branch further
                if allow_opouts.contains(node_opout) {
                    continue;
                }
                // encountering a reject node on an unshielded path: reject
                if reject_opouts.contains(node_opout) {
                    to_reject.insert(*check_opout);
                    break;
                }
                for (_edge, parent) in dag.parents(node).iter(dag) {
                    stack.push(parent);
                }
            }
        }
        Ok(to_reject)
    }

    fn prepare_rgb_psbt(
        &self,
        txn: &DbTxn,
        psbt: &mut Psbt,
        transfer_info_map: &mut BTreeMap<String, InfoAssetTransfer>,
        transfer_dir: PathBuf,
        donation: bool,
        unspents: Vec<LocalUnspent>,
        runtime: &mut RgbRuntime,
        min_confirmations: u8,
        expiration_timestamp: Option<i64>,
        btc_change: Option<BtcChange>,
        rejected: &mut HashSet<Opout>,
    ) -> Result<PrepareRgbPsbtResult, Error> {
        let mut change_utxo_option = None;

        let prev_outputs = psbt
            .unsigned_tx
            .input
            .iter()
            .map(|txin| txin.previous_output)
            .collect::<HashSet<OutPoint>>();

        let input_outpoints: Vec<Outpoint> =
            prev_outputs.iter().map(|o| Outpoint::from(*o)).collect();

        let mut all_transitions: HashMap<ContractId, Vec<Transition>> = HashMap::new();
        let mut asset_beneficiaries = bmap![];
        let mut extra_state = HashMap::<ContractId, Vec<(OutPoint, Opout, AllocatedState)>>::new();
        let mut input_opouts: HashMap<ContractId, HashMap<Opout, AllocatedState>> = HashMap::new();
        for (asset_id, transfer_info) in transfer_info_map.iter_mut() {
            let asset_utxos = transfer_info.asset_spend.input_outpoints.iter().cloned();
            let mut all_opout_state_vec = Vec::new();
            for (explicit_seal, opout_state_map) in runtime.contract_assignments_for(
                transfer_info.asset_info.contract_id,
                asset_utxos.clone(),
            )? {
                all_opout_state_vec.extend(
                    opout_state_map
                        .into_iter()
                        .map(|(o, s)| (explicit_seal.to_outpoint(), o, s)),
                );
            }

            // sort by state globally (smaller to bigger)
            all_opout_state_vec.sort_by_key(|(_, _, state)| match state {
                AllocatedState::Amount(amt) => amt.as_u64(),
                _ => 0, // non-amount states sorted first
            });

            let mut inputs_added = AssignmentsCollection::default();
            let mut uda_state = None;
            let mut asset_transition_builder = runtime.transition_builder(
                transfer_info.asset_info.contract_id,
                transfer_info.main_transition.clone().type_name(),
            )?;
            for (outpoint, opout, state) in all_opout_state_vec {
                let mut should_add_as_input = !rejected.contains(&opout);
                if should_add_as_input {
                    should_add_as_input = inputs_added.opout_contributes(
                        &opout,
                        &state,
                        &transfer_info.assignments_needed,
                    );
                }
                if !should_add_as_input {
                    extra_state
                        .entry(transfer_info.asset_info.contract_id)
                        .or_default()
                        .push((outpoint, opout, state.clone()));
                    continue;
                }

                inputs_added.add_opout_state(&opout, &state);
                transfer_info
                    .assignments_spent
                    .entry(outpoint)
                    .or_default()
                    .push(Assignment::from_opout_and_state(opout, &state));
                // there can be only a single state when contract is UDA
                uda_state = Some(state.clone());
                asset_transition_builder =
                    asset_transition_builder.add_input(opout, state.clone())?;
                input_opouts
                    .entry(transfer_info.asset_info.contract_id)
                    .or_default()
                    .insert(opout, state);
            }

            let mut beneficiaries = vec![];
            for recipient in &transfer_info.recipients {
                let seal;
                match &recipient.assignment {
                    Assignment::Fungible(amt) => {
                        if *amt == 0 {
                            continue;
                        }
                        seal = self.get_beneficiary_seal(&recipient.local_recipient_data);
                        asset_transition_builder = asset_transition_builder.add_fungible_state(
                            RGB_STATE_ASSET_OWNER,
                            seal,
                            *amt,
                        )?;
                    }
                    Assignment::NonFungible => {
                        if let AllocatedState::Data(state) = uda_state.clone().unwrap() {
                            seal = self.get_beneficiary_seal(&recipient.local_recipient_data);
                            asset_transition_builder = asset_transition_builder
                                .add_data(RGB_STATE_ASSET_OWNER, seal, Allocation::from(state))
                                .map_err(Error::from)?;
                        } else {
                            continue;
                        }
                    }
                    Assignment::InflationRight(amt) => {
                        if *amt == 0 {
                            continue;
                        }
                        seal = self.get_beneficiary_seal(&recipient.local_recipient_data);
                        asset_transition_builder = asset_transition_builder.add_fungible_state(
                            RGB_STATE_INFLATION_ALLOWANCE,
                            seal,
                            *amt,
                        )?;
                    }
                    _ => unreachable!(),
                };

                beneficiaries.push((seal, recipient.recipient_id.clone()));
            }

            let change = inputs_added.change(&transfer_info.original_assignments_needed);

            if change != AssignmentsCollection::default() {
                transfer_info.change = change.clone();
                let seal = self.get_change_seal(
                    txn,
                    &btc_change,
                    &mut change_utxo_option,
                    &input_outpoints,
                    unspents.as_slice(),
                )?;
                if change.fungible > 0 {
                    asset_transition_builder = asset_transition_builder.add_fungible_state(
                        RGB_STATE_ASSET_OWNER,
                        seal,
                        change.fungible,
                    )?;
                }
                if change.inflation > 0 {
                    asset_transition_builder = asset_transition_builder.add_fungible_state(
                        RGB_STATE_INFLATION_ALLOWANCE,
                        seal,
                        change.inflation,
                    )?;
                }
            };

            // add necessary globals/metadata to transition
            match transfer_info.main_transition {
                TypeOfTransition::Inflate => {
                    let inflation = transfer_info.original_assignments_needed.inflation;
                    asset_transition_builder = asset_transition_builder
                        .add_global_state(RGB_GLOBAL_ISSUED_SUPPLY, Amount::from(inflation))
                        .unwrap()
                        .add_metadata(
                            RGB_METADATA_ALLOWED_INFLATION,
                            Amount::from(change.inflation),
                        )
                        .unwrap();
                }
                TypeOfTransition::Burn => {
                    let burn = transfer_info.original_assignments_needed.fungible;
                    asset_transition_builder = asset_transition_builder
                        .add_metadata(RGB_METADATA_BURNED_ASSET, Amount::from(burn))
                        .unwrap()
                        .add_metadata(RGB_METADATA_BURNED_INFLATION, Amount::from(0u64))
                        .unwrap();
                }
                _ => {}
            }

            let transition = asset_transition_builder.complete_transition()?;
            all_transitions
                .entry(transfer_info.asset_info.contract_id)
                .or_default()
                .push(transition.clone());
            psbt.push_rgb_transition(transition)
                .map_err(InternalError::from)?;
            asset_beneficiaries.insert(asset_id.clone(), beneficiaries);
        }

        for id in runtime.contracts_assigning(prev_outputs.clone())? {
            if transfer_info_map.contains_key(&id.to_string()) {
                continue;
            }
            let state = runtime.contract_assignments_for(id, prev_outputs.clone())?;
            let entry = extra_state.entry(id).or_default();
            for (explicit_seal, opout_state_map) in state {
                entry.extend(
                    opout_state_map
                        .into_iter()
                        .map(|(o, s)| (explicit_seal.to_outpoint(), o, s)),
                );
            }
        }

        let mut extra_allocations: HashMap<String, HashMap<OutPoint, Vec<Assignment>>> =
            HashMap::new();
        for (cid, opout_state_map) in extra_state {
            let schema = runtime.contract_schema(cid)?;
            for (outpoint, opout, state) in opout_state_map {
                let transition_type = schema.default_transition_for_assignment(&opout.ty);
                let mut extra_builder = runtime.transition_builder_raw(cid, transition_type)?;
                let assignment = Assignment::from_opout_and_state(opout, &state);
                let seal = self.get_change_seal(
                    txn,
                    &btc_change,
                    &mut change_utxo_option,
                    &input_outpoints,
                    unspents.as_slice(),
                )?;
                extra_builder = extra_builder
                    .add_input(opout, state.clone())?
                    .add_owned_state_raw(opout.ty, seal, state)?;
                let extra_transition = extra_builder.complete_transition()?;
                all_transitions
                    .entry(cid)
                    .or_default()
                    .push(extra_transition.clone());
                extra_allocations
                    .entry(cid.to_string())
                    .or_default()
                    .entry(outpoint)
                    .or_default()
                    .push(assignment);
                psbt.push_rgb_transition(extra_transition)
                    .map_err(InternalError::from)?;
            }
        }

        let opreturn_index = psbt
            .unsigned_tx
            .output
            .iter()
            .enumerate()
            .find(|(_, o)| o.script_pubkey.is_op_return())
            .expect("psbt should have an op_return output")
            .0;
        let opreturn_output = psbt.outputs.get_mut(opreturn_index).unwrap();
        opreturn_output.set_opret_host();
        let entropy = rand::rng().random_range(0..u64::MAX);

        opreturn_output
            .set_mpc_entropy(entropy)
            .map_err(InternalError::from)?;

        for (cid, transitions) in &all_transitions {
            for transition in transitions {
                for opout in transition.inputs() {
                    psbt.set_rgb_contract_consumer(*cid, opout, transition.id())
                        .map_err(InternalError::from)?;
                }
            }
        }

        psbt.set_rgb_close_method(CloseMethod::OpretFirst);
        let fascia = psbt.rgb_commit().map_err(InternalError::from)?;
        fs::create_dir_all(&transfer_dir)?;
        let fascia_path = transfer_dir.join(FASCIA_FILE);
        let serialized_fascia = serde_json::to_string(&fascia).map_err(InternalError::from)?;
        fs::write(fascia_path, serialized_fascia)?;

        let witness_txid = psbt.get_txid();
        for (asset_id, transfer_info) in transfer_info_map.iter_mut() {
            let beneficiaries = asset_beneficiaries[asset_id].clone();
            let (beneficiaries_witness, beneficiaries_blinded) = beneficiaries.iter().fold(
                (Vec::new(), Vec::new()),
                |(mut witness, mut blinded), (builder_seal, _)| {
                    match builder_seal {
                        BuilderSeal::Revealed(seal) => {
                            let explicit_seal = ExplicitSeal::with(witness_txid, seal.vout);
                            witness.push(explicit_seal);
                        }
                        BuilderSeal::Concealed(secret_seal) => {
                            blinded.push(*secret_seal);
                        }
                    }
                    (witness, blinded)
                },
            );
            transfer_info.beneficiaries_blinded = beneficiaries_blinded;
            transfer_info.beneficiaries_witness = beneficiaries_witness;

            let should_build_dag = transfer_info.main_transition == TypeOfTransition::Transfer
                && transfer_info.asset_info.reject_list_url.is_some();

            #[cfg(test)]
            let should_build_dag = if skip_build_dag() {
                false
            } else {
                should_build_dag
            };

            if should_build_dag {
                let (_, dag_data) = runtime.transfer_from_fascia_with_dag(
                    transfer_info.asset_info.contract_id,
                    transfer_info.beneficiaries_witness.clone(),
                    transfer_info.beneficiaries_blinded.clone(),
                    &fascia,
                )?;

                let (reject_opouts, allow_opouts) = self
                    .get_reject_list(transfer_info.asset_info.reject_list_url.as_ref().unwrap())?;
                let asset_opouts = input_opouts
                    .get(&transfer_info.asset_info.contract_id)
                    .unwrap();
                let asset_input_opouts = asset_opouts.keys().cloned().collect();
                let to_reject = self.check_dag(
                    &dag_data,
                    &reject_opouts,
                    &allow_opouts,
                    &asset_input_opouts,
                )?;
                if !to_reject.is_empty() {
                    warn!(
                        self.logger(),
                        "Found {} rejected input opout(s), retrying transfer",
                        to_reject.len()
                    );
                    // update assignments_needed to account for rejected amounts
                    for rejected_opout in &to_reject {
                        if let Some(state) = asset_opouts.get(rejected_opout) {
                            transfer_info
                                .assignments_needed
                                .add_opout_state(rejected_opout, state);
                        }
                        rejected.insert(*rejected_opout);
                    }
                    return Ok(PrepareRgbPsbtResult::Retry);
                }
            }
        }

        let created_at = now().unix_timestamp();

        // save batch transfer data to file (for operation finalization)
        let info_batch_transfer = InfoBatchTransfer {
            btc_change,
            change_utxo_outpoint: change_utxo_option.as_ref().map(|utxo| utxo.outpoint()),
            extra_allocations,
            donation,
            min_confirmations,
            expiration_timestamp,
            created_at,
            entropy,
            transfers: transfer_info_map.clone(),
        };
        let serialized_info =
            serde_json::to_string(&info_batch_transfer).map_err(InternalError::from)?;
        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        fs::write(info_file, serialized_info)?;

        Ok(PrepareRgbPsbtResult::Success(Box::new(
            BeginOperationData {
                psbt: psbt.clone(),
                transfer_dir: transfer_dir.clone(),
                info_batch_transfer,
                batch_transfer_idx: None,
            },
        )))
    }

    fn post_consignment_to_proxy<P: AsRef<Path>>(
        &self,
        proxy_client: &ProxyClient,
        recipient_id: String,
        consignment_path: P,
        txid: String,
        vout: Option<u32>,
    ) -> Result<(), Error> {
        let consignment_res =
            proxy_client.post_consignment(&recipient_id, consignment_path, &txid, vout)?;
        debug!(
            self.logger(),
            "Consignment POST response: {:?}", consignment_res
        );

        if let Some(err) = consignment_res.error {
            if err.code == -101 {
                return Err(Error::RecipientIDAlreadyUsed)?;
            }
            return Err(Error::InvalidTransportEndpoint {
                details: format!("proxy error: {}", err.message),
            });
        }
        if consignment_res.result.is_none() {
            return Err(Error::InvalidTransportEndpoint {
                details: s!("invalid result"),
            });
        }

        Ok(())
    }

    fn post_transfer_data(
        &self,
        recipients: &mut Vec<LocalRecipient>,
        asset_transfer_dir: PathBuf,
        txid: String,
        medias: Vec<Media>,
    ) -> Result<(), Error> {
        let consignment_path = self.get_send_consignment_path_impl(&asset_transfer_dir);
        for recipient in recipients {
            let recipient_id = &recipient.recipient_id;
            let mut found_valid = false;
            for transport_endpoint in recipient.transport_endpoints.iter_mut() {
                if transport_endpoint.transport_type != TransportType::JsonRpc
                    || !transport_endpoint.usable
                {
                    debug!(
                        self.logger(),
                        "Skipping transport endpoint {:?}", transport_endpoint
                    );
                    continue;
                }
                let proxy_url = transport_endpoint.endpoint.clone();
                debug!(
                    self.logger(),
                    "Posting consignment for recipient ID: {recipient_id}"
                );
                #[cfg(test)]
                let vout = mock_vout(recipient.local_recipient_data.vout());
                #[cfg(not(test))]
                let vout = recipient.local_recipient_data.vout();
                let proxy_client = ProxyClient::new(&proxy_url)?;
                match self.post_consignment_to_proxy(
                    &proxy_client,
                    recipient_id.clone(),
                    &consignment_path,
                    txid.clone(),
                    vout,
                ) {
                    Err(Error::RecipientIDAlreadyUsed) => {
                        return Err(Error::RecipientIDAlreadyUsed);
                    }
                    Err(_) => continue,
                    Ok(()) => {}
                }

                for media in &medias {
                    let digest = media.get_digest();
                    let media_res = proxy_client.post_media(&digest, &media.file_path)?;
                    debug!(self.logger(), "Attachment POST response: {:?}", media_res);
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

    fn get_change_utxo_idx(
        &self,
        txn: &DbTxn,
        txid: &str,
        info_contents: &InfoBatchTransfer,
        change_utxo_idx: &mut Option<i32>,
    ) -> Result<i32, Error> {
        if let Some(idx) = *change_utxo_idx {
            return Ok(idx);
        }
        let idx = if let Some(btc_change) = &info_contents.btc_change {
            match txn.get_txo(&Outpoint {
                txid: txid.to_string(),
                vout: btc_change.vout,
            })? {
                Some(txo) => txo.idx,
                None => {
                    let db_utxo = DbTxoActMod {
                        txid: ActiveValue::Set(txid.to_string()),
                        vout: ActiveValue::Set(btc_change.vout),
                        btc_amount: ActiveValue::Set(btc_change.amount.to_string()),
                        spent: ActiveValue::Set(false),
                        exists: ActiveValue::Set(false),
                        pending_witness: ActiveValue::Set(false),
                        ..Default::default()
                    };
                    txn.set_txo(db_utxo)?
                }
            }
        } else {
            let outpoint = info_contents
                .change_utxo_outpoint
                .as_ref()
                .expect("change utxo source");
            txn.get_txo(outpoint)?.expect("should exist").idx
        };
        *change_utxo_idx = Some(idx);
        Ok(idx)
    }

    fn save_transfers(
        &mut self,
        txn: &DbTxn,
        txid: String,
        info_contents: &InfoBatchTransfer,
        status: TransferStatus,
    ) -> Result<i32, Error> {
        let batch_transfer = DbBatchTransferActMod {
            txid: ActiveValue::Set(Some(txid.clone())),
            status: ActiveValue::Set(status),
            expiration: ActiveValue::Set(info_contents.expiration_timestamp),
            created_at: ActiveValue::Set(info_contents.created_at),
            min_confirmations: ActiveValue::Set(info_contents.min_confirmations),
            ..Default::default()
        };
        let batch_transfer_idx = txn.set_batch_transfer(batch_transfer)?;

        let mut change_utxo_idx: Option<i32> = None;

        for (asset_id, transfer_info) in info_contents.transfers.iter() {
            let asset_transfer = DbAssetTransferActMod {
                user_driven: ActiveValue::Set(true),
                batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
                asset_id: ActiveValue::Set(Some(asset_id.clone())),
                ..Default::default()
            };
            let asset_transfer_idx = txn.set_asset_transfer(asset_transfer)?;

            for (outpoint, assignments) in &transfer_info.assignments_spent {
                let outpoint: Outpoint = (*outpoint).into();
                let txo_idx = match txn.get_txo(&outpoint)? {
                    Some(txo) => txo.idx,
                    None => {
                        self.sync_wallet(
                            txn,
                            SyncOptions {
                                keychain: SyncKeychain::Colored,
                                strategy: SyncStrategy::FastSync,
                            },
                            true,
                        )?;
                        let bdk_utxo = txn.get_txo(&outpoint)?.expect("should exist");
                        let new_db_utxo: DbTxoActMod = bdk_utxo.clone().into();
                        txn.set_txo(new_db_utxo)?
                    }
                };

                for assignment in assignments {
                    let db_coloring = DbColoringActMod {
                        txo_idx: ActiveValue::Set(txo_idx),
                        asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                        r#type: ActiveValue::Set(ColoringType::Input),
                        assignment: ActiveValue::Set(assignment.clone()),
                        ..Default::default()
                    };
                    txn.set_coloring(db_coloring)?;
                }
            }
            if transfer_info.change.fungible > 0 {
                let db_coloring = DbColoringActMod {
                    txo_idx: ActiveValue::Set(self.get_change_utxo_idx(
                        txn,
                        &txid,
                        info_contents,
                        &mut change_utxo_idx,
                    )?),
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    r#type: ActiveValue::Set(ColoringType::Change),
                    assignment: ActiveValue::Set(Assignment::Fungible(
                        transfer_info.change.fungible,
                    )),
                    ..Default::default()
                };
                txn.set_coloring(db_coloring)?;
            }
            if transfer_info.change.inflation > 0 {
                let db_coloring = DbColoringActMod {
                    txo_idx: ActiveValue::Set(self.get_change_utxo_idx(
                        txn,
                        &txid,
                        info_contents,
                        &mut change_utxo_idx,
                    )?),
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    r#type: ActiveValue::Set(ColoringType::Change),
                    assignment: ActiveValue::Set(Assignment::InflationRight(
                        transfer_info.change.inflation,
                    )),
                    ..Default::default()
                };
                txn.set_coloring(db_coloring)?;
            }

            for recipient in transfer_info.recipients.clone() {
                let (rcpt_id, rcpt_type, req_ass) = match transfer_info.main_transition {
                    TypeOfTransition::Inflate => {
                        let local_witness_data = if let LocalRecipientData::Witness(lwd) =
                            recipient.local_recipient_data
                        {
                            lwd
                        } else {
                            unreachable!("inflation uses witness recipients")
                        };
                        let vout = local_witness_data.vout;
                        let txo_idx = match txn.get_txo(&Outpoint {
                            txid: txid.clone(),
                            vout,
                        })? {
                            Some(txo) => txo.idx,
                            None => {
                                let db_utxo = DbTxoActMod {
                                    txid: ActiveValue::Set(txid.clone()),
                                    vout: ActiveValue::Set(vout),
                                    btc_amount: ActiveValue::Set(
                                        local_witness_data.amount_sat.to_string(),
                                    ),
                                    spent: ActiveValue::Set(false),
                                    exists: ActiveValue::Set(false),
                                    pending_witness: ActiveValue::Set(false),
                                    ..Default::default()
                                };
                                txn.set_txo(db_utxo)?
                            }
                        };
                        let db_coloring = DbColoringActMod {
                            txo_idx: ActiveValue::Set(txo_idx),
                            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                            r#type: ActiveValue::Set(ColoringType::Issue),
                            assignment: ActiveValue::Set(recipient.assignment.clone()),
                            ..Default::default()
                        };
                        txn.set_coloring(db_coloring)?;
                        (
                            Some(recipient.recipient_id.clone()),
                            Some(RecipientTypeFull::Witness { vout: Some(vout) }),
                            recipient.assignment,
                        )
                    }
                    TypeOfTransition::Burn => (
                        None,
                        None,
                        Assignment::Fungible(transfer_info.original_assignments_needed.fungible),
                    ),
                    TypeOfTransition::Transfer => (
                        Some(recipient.recipient_id.clone()),
                        None,
                        recipient.assignment,
                    ),
                };

                let transfer = DbTransferActMod {
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    requested_assignment: ActiveValue::Set(Some(req_ass)),
                    incoming: ActiveValue::Set(false),
                    recipient_id: ActiveValue::Set(rcpt_id),
                    recipient_type: ActiveValue::Set(rcpt_type),
                    ..Default::default()
                };
                let transfer_idx = txn.set_transfer(transfer)?;
                for transport_endpoint in recipient.transport_endpoints {
                    self.save_transfer_transport_endpoint(txn, transfer_idx, &transport_endpoint)?;
                }
            }
        }

        for (asset_id, txo_assignments) in &info_contents.extra_allocations {
            let asset_transfer = DbAssetTransferActMod {
                user_driven: ActiveValue::Set(false),
                batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
                asset_id: ActiveValue::Set(Some(asset_id.clone())),
                ..Default::default()
            };
            let asset_transfer_idx = txn.set_asset_transfer(asset_transfer)?;
            for (outpoint, assignments) in txo_assignments {
                let outpoint: Outpoint = (*outpoint).into();
                let input_idx = match txn.get_txo(&outpoint)? {
                    Some(txo) => txo.idx,
                    None => {
                        self.sync_wallet(
                            txn,
                            SyncOptions {
                                keychain: SyncKeychain::Colored,
                                strategy: SyncStrategy::FastSync,
                            },
                            true,
                        )?;
                        let bdk_utxo = txn.get_txo(&outpoint)?.expect("should exist");
                        let new_db_utxo: DbTxoActMod = bdk_utxo.clone().into();
                        txn.set_txo(new_db_utxo)?
                    }
                };
                for assignment in assignments {
                    let db_coloring = DbColoringActMod {
                        txo_idx: ActiveValue::Set(input_idx),
                        asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                        r#type: ActiveValue::Set(ColoringType::Input),
                        assignment: ActiveValue::Set(assignment.clone()),
                        ..Default::default()
                    };
                    txn.set_coloring(db_coloring)?;
                    let db_coloring = DbColoringActMod {
                        txo_idx: ActiveValue::Set(self.get_change_utxo_idx(
                            txn,
                            &txid,
                            info_contents,
                            &mut change_utxo_idx,
                        )?),
                        asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                        r#type: ActiveValue::Set(ColoringType::Change),
                        assignment: ActiveValue::Set(assignment.clone()),
                        ..Default::default()
                    };
                    txn.set_coloring(db_coloring)?;
                }
            }
        }

        Ok(batch_transfer_idx)
    }

    fn update_or_save_transfers(
        &mut self,
        txn: &DbTxn,
        txid: String,
        info_contents: &InfoBatchTransfer,
        status: TransferStatus,
        sync_tte_used: bool,
    ) -> Result<i32, Error> {
        if let Some(existing) = txn.get_batch_transfer_by_txid(&txid)? {
            let mut updated: DbBatchTransferActMod = existing.clone().into();
            updated.status = ActiveValue::Set(status);
            txn.update_batch_transfer(&mut updated)?;
            if sync_tte_used {
                let asset_transfers = txn.iter_asset_transfers()?;
                let transfers = txn.iter_transfers()?;
                let batch_data = existing.get_transfers(&asset_transfers, &transfers)?;
                for asset_transfer_data in &batch_data.asset_transfers_data {
                    let asset_id = asset_transfer_data
                        .asset_transfer
                        .asset_id
                        .as_ref()
                        .expect("exists at this point");
                    let info_asset = info_contents
                        .transfers
                        .get(asset_id)
                        .expect("exists at this point");
                    for db_transfer in &asset_transfer_data.transfers {
                        let recipient = info_asset
                            .recipients
                            .iter()
                            .find(|r| {
                                db_transfer.recipient_id.as_deref() == Some(r.recipient_id.as_str())
                            })
                            .expect("recipient should be set");
                        let tte_data =
                            txn.get_transfer_transport_endpoints_data(db_transfer.idx)?;
                        for (tte, te) in tte_data {
                            let local_used = recipient
                                .transport_endpoints
                                .iter()
                                .find(|lte| lte.endpoint == te.endpoint)
                                .is_some_and(|lte| lte.used);
                            if tte.used != local_used {
                                let mut updated_tte: DbTransferTransportEndpointActMod = tte.into();
                                updated_tte.used = ActiveValue::Set(local_used);
                                txn.update_transfer_transport_endpoint(&mut updated_tte)?;
                            }
                        }
                    }
                }
            }
            Ok(existing.idx)
        } else {
            self.save_transfers(txn, txid, info_contents, status)
        }
    }

    fn get_input_unspents(&self, unspents: &[LocalUnspent]) -> Result<Vec<LocalUnspent>, Error> {
        let mut input_unspents = unspents.to_vec();
        // consider the following UTXOs unspendable:
        // - incoming and pending
        // - outgoing and in initiated or waiting counterparty status
        // - pending incoming witness
        // - pending incoming blinded
        // - inexistent
        input_unspents.retain(|u| {
            !(u.rgb_allocations
                .iter()
                .any(|a| a.incoming && a.status.pending()))
                && !(u.rgb_allocations.iter().any(|a| {
                    !a.incoming && (a.status.initiated() || a.status.waiting_counterparty())
                }))
                && !u.utxo.pending_witness
                && u.pending_blinded == 0
                && u.utxo.exists
        });
        Ok(input_unspents)
    }

    fn get_transfer_begin_data(
        &mut self,
        txn: &DbTxn,
        fee_rate: u64,
    ) -> Result<(FeeRate, Vec<LocalUnspent>, Vec<LocalUnspent>, RgbRuntime), Error> {
        let fee_rate_checked = self.check_fee_rate(fee_rate)?;

        let db_data = txn.get_db_data(false)?;

        let utxos = txn.get_unspent_txos(db_data.txos)?;

        let unspents = txn.get_rgb_allocations(
            utxos,
            Some(db_data.colorings),
            Some(db_data.batch_transfers),
            Some(db_data.asset_transfers),
            Some(db_data.transfers),
        )?;

        #[cfg(test)]
        let input_unspents = mock_input_unspents(self, &unspents);
        #[cfg(not(test))]
        let input_unspents = self.get_input_unspents(&unspents)?;

        let runtime = self.rgb_runtime()?;

        Ok((fee_rate_checked, unspents, input_unspents, runtime))
    }

    fn setup_transfer_directory(&self, receive_ids: Vec<String>) -> Result<PathBuf, Error> {
        let mut receive_ids_dedup = receive_ids.clone();
        receive_ids_dedup.sort();
        receive_ids_dedup.dedup();
        if receive_ids.len() != receive_ids_dedup.len() {
            return Err(Error::RecipientIDDuplicated);
        }
        let mut hasher = DefaultHasher::new();
        receive_ids.hash(&mut hasher);
        let transfer_dir = self.get_transfer_dir(&hasher.finish().to_string());
        if transfer_dir.exists() {
            fs::remove_dir_all(&transfer_dir)?;
        }
        Ok(transfer_dir)
    }

    fn prepare_transfer_psbt(
        &mut self,
        txn: &DbTxn,
        transfer_info_map: &mut BTreeMap<String, InfoAssetTransfer>,
        transfer_dir: PathBuf,
        donation: bool,
        unspents: Vec<LocalUnspent>,
        input_unspents: &[LocalUnspent],
        witness_recipients: &Vec<(ScriptBuf, u64)>,
        fee_rate_checked: FeeRate,
        min_confirmations: u8,
        expiration_timestamp: Option<i64>,
        runtime: &mut RgbRuntime,
        rejected: &mut HashSet<Opout>,
        dry_run: bool,
    ) -> Result<PrepareTransferPsbtResult, Error> {
        // prepare BDK PSBT
        let mut all_inputs: HashSet<BdkOutPoint> = transfer_info_map
            .values()
            .flat_map(|ti| {
                ti.asset_spend
                    .input_outpoints
                    .iter()
                    .map(|o| o.clone().into())
            })
            .collect();
        let (mut psbt, btc_change) = self.try_prepare_psbt(
            input_unspents,
            &mut all_inputs,
            witness_recipients,
            fee_rate_checked,
        )?;
        psbt.unsigned_tx.output[0].script_pubkey = ScriptBuf::new_op_return([]);

        // prepare RGB PSBT
        let begin_operation_data = match self.prepare_rgb_psbt(
            txn,
            &mut psbt,
            transfer_info_map,
            transfer_dir.clone(),
            donation,
            unspents,
            runtime,
            min_confirmations,
            expiration_timestamp,
            btc_change,
            rejected,
        )? {
            PrepareRgbPsbtResult::Retry => return Ok(PrepareTransferPsbtResult::Retry),
            PrepareRgbPsbtResult::Success(begin_operation_data) => begin_operation_data,
        };

        // rename transfer directory
        let txid = psbt
            .clone()
            .extract_tx()
            .map_err(InternalError::from)?
            .compute_txid()
            .to_string();
        let new_transfer_dir = self.get_transfer_dir(&txid);
        fs::rename(transfer_dir, &new_transfer_dir)?;

        // persist the unsigned PSBT
        fs::write(
            new_transfer_dir.join(UNSIGNED_PSBT_FILE),
            begin_operation_data.psbt.to_string(),
        )?;

        // update transfer_dir to the new (renamed) directory
        let mut begin_operation_data = begin_operation_data;
        begin_operation_data.transfer_dir = new_transfer_dir;

        if !dry_run {
            // save transfer to DB with Initiated status to reserve the UTXOs
            let batch_transfer_idx = self.save_transfers(
                txn,
                txid,
                &begin_operation_data.info_batch_transfer,
                TransferStatus::Initiated,
            )?;
            begin_operation_data.batch_transfer_idx = Some(batch_transfer_idx);
        }

        Ok(PrepareTransferPsbtResult::Success(begin_operation_data))
    }

    fn finalize_transfer_end(
        &mut self,
        txn: &DbTxn,
        txid: String,
        psbt: &Psbt,
        info_contents: &InfoBatchTransfer,
        status: TransferStatus,
        fascia: Fascia,
        sync_tte_used: bool,
    ) -> Result<i32, Error> {
        let mut runtime = self.rgb_runtime()?;
        self.broadcast_and_update_rgb(txn, &mut runtime, psbt, fascia)?;
        self.update_or_save_transfers(txn, txid, info_contents, status, sync_tte_used)
    }

    fn send_begin_impl(
        &mut self,
        txn: &DbTxn,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        expiration_timestamp: Option<i64>,
        dry_run: bool,
    ) -> Result<BeginOperationData, Error> {
        if recipient_map.is_empty() || recipient_map.values().any(|v| v.is_empty()) {
            return Err(Error::InvalidRecipientMap);
        }

        let (fee_rate_checked, unspents, input_unspents, mut runtime) =
            self.get_transfer_begin_data(txn, fee_rate)?;

        let chainnet: ChainNet = self.bitcoin_network().into();
        let mut witness_recipients: Vec<(ScriptBuf, u64)> = vec![];
        let mut recipient_vout = 1;
        let main_transition = TypeOfTransition::Transfer;
        let mut assets_data: BTreeMap<String, (AssetInfo, AssignmentsCollection)> = BTreeMap::new();
        let mut local_recipients: BTreeMap<String, Vec<LocalRecipient>> = BTreeMap::new();
        for (asset_id, recipients) in &recipient_map {
            let asset = txn.check_asset_exists(asset_id.clone())?;
            let schema = asset.schema;
            self.check_schema_support(&schema)?;

            let mut original_assignments_needed = AssignmentsCollection::default();
            for recipient in recipients.clone() {
                self.check_transport_endpoints(&recipient.transport_endpoints)?;
                match (&recipient.assignment, schema) {
                    (
                        Assignment::Fungible(amt),
                        AssetSchema::Nia | AssetSchema::Cfa | AssetSchema::Ifa,
                    ) => {
                        if *amt == 0 {
                            return Err(Error::InvalidAmountZero);
                        }
                    }
                    (Assignment::NonFungible, AssetSchema::Uda) => {}
                    (Assignment::InflationRight(amt), AssetSchema::Ifa) => {
                        if *amt == 0 {
                            return Err(Error::InvalidAmountZero);
                        }
                    }
                    _ => {
                        return Err(Error::InvalidAssignment);
                    }
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
                    if check_proxy(&transport_endpoint.endpoint).is_ok() {
                        local_transport_endpoint.usable = true;
                        found_valid = true;
                    }
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
                    Beneficiary::WitnessVout(pay_2_vout, _) => {
                        if let Some(ref witness_data) = recipient.witness_data {
                            let script_buf = pay_2_vout.to_script();
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

                local_recipients
                    .entry(asset_id.clone())
                    .or_default()
                    .push(LocalRecipient {
                        recipient_id: recipient.recipient_id,
                        local_recipient_data,
                        assignment: recipient.assignment.clone(),
                        transport_endpoints,
                    });

                recipient
                    .assignment
                    .add_to_assignments(&mut original_assignments_needed);
            }
            let contract_id = ContractId::from_str(asset_id).expect("invalid contract ID");
            assets_data.insert(
                asset_id.clone(),
                (
                    AssetInfo {
                        contract_id,
                        reject_list_url: asset.reject_list_url,
                    },
                    original_assignments_needed,
                ),
            );
        }

        let receive_ids: Vec<String> = recipient_map
            .values()
            .flatten()
            .map(|r| r.recipient_id.clone())
            .collect();
        let transfer_dir = self.setup_transfer_directory(receive_ids)?;

        let mut rejected = HashSet::new();
        let mut transfer_info_map: BTreeMap<String, InfoAssetTransfer> = BTreeMap::new();

        Ok(loop {
            for asset_id in recipient_map.keys() {
                let (asset_info, original_assignments_needed) =
                    assets_data.get(asset_id).unwrap().clone();
                let assignments_needed =
                    if let Some(existing_info) = transfer_info_map.get(asset_id) {
                        existing_info.assignments_needed.clone()
                    } else {
                        original_assignments_needed.clone()
                    };

                // if no more assignments this returns an error that makes the loop stop
                let asset_spend = self.select_rgb_inputs(
                    asset_id.clone(),
                    &assignments_needed,
                    input_unspents.clone(),
                )?;

                let transfer_info = InfoAssetTransfer {
                    asset_info,
                    recipients: local_recipients[asset_id].clone(),
                    asset_spend,
                    change: AssignmentsCollection::default(),
                    original_assignments_needed,
                    assignments_needed,
                    assignments_spent: HashMap::new(),
                    main_transition,
                    beneficiaries_blinded: vec![],
                    beneficiaries_witness: vec![],
                };
                transfer_info_map.insert(asset_id.clone(), transfer_info);
            }

            match self.prepare_transfer_psbt(
                txn,
                &mut transfer_info_map,
                transfer_dir.clone(),
                donation,
                unspents.clone(),
                &input_unspents,
                &witness_recipients,
                fee_rate_checked,
                min_confirmations,
                expiration_timestamp,
                &mut runtime,
                &mut rejected,
                dry_run,
            )? {
                PrepareTransferPsbtResult::Retry => continue,
                PrepareTransferPsbtResult::Success(begin_operation_data) => {
                    break *begin_operation_data;
                }
            }
        })
    }

    fn send_end_impl(&mut self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<OperationResult, Error> {
        let (txid, transfer_dir, mut info_contents, fascia) =
            self.get_transfer_end_data(signed_psbt)?;

        self.gen_consignments(&fascia, &info_contents.transfers, &transfer_dir)?;

        let psbt_out = transfer_dir.join(SIGNED_PSBT_FILE);
        fs::write(psbt_out, signed_psbt.to_string())?;

        let mut medias = None;
        let mut tokens = None;
        let mut token_medias = None;
        for (asset_id, info_contents_asset) in info_contents.transfers.iter_mut() {
            let asset = txn.get_asset(asset_id.clone())?.unwrap();
            let token = match asset.schema {
                AssetSchema::Uda => {
                    if medias.clone().is_none() {
                        medias = Some(txn.iter_media()?);
                        tokens = Some(txn.iter_tokens()?);
                        token_medias = Some(txn.iter_token_medias()?);
                    }
                    self.get_asset_token(
                        asset.idx,
                        medias.as_ref().unwrap(),
                        tokens.as_ref().unwrap(),
                        token_medias.as_ref().unwrap(),
                    )
                }
                AssetSchema::Nia | AssetSchema::Cfa | AssetSchema::Ifa => None,
            };

            // post consignment(s) and optional media(s)
            let asset_transfer_dir = self.get_asset_transfer_dir(&transfer_dir, asset_id);
            self.post_transfer_data(
                &mut info_contents_asset.recipients,
                asset_transfer_dir,
                txid.clone(),
                self.get_asset_medias(txn, asset.media_idx, token)?,
            )?;
        }

        let sync_tte_used = true;
        let batch_transfer_idx = if info_contents.donation {
            self.finalize_transfer_end(
                txn,
                txid.clone(),
                signed_psbt,
                &info_contents,
                TransferStatus::WaitingConfirmations,
                fascia,
                sync_tte_used,
            )?
        } else {
            self.update_or_save_transfers(
                txn,
                txid.clone(),
                &info_contents,
                TransferStatus::WaitingCounterparty,
                sync_tte_used,
            )?
        };

        Ok(OperationResult {
            txid,
            batch_transfer_idx,
            entropy: info_contents.entropy,
        })
    }

    fn send_btc_begin_impl(
        &mut self,
        txn: &DbTxn,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
        dry_run: bool,
    ) -> Result<Psbt, Error> {
        let fee_rate_checked = self.check_fee_rate(fee_rate)?;

        if !skip_sync {
            self.sync_wallet(
                txn,
                SyncOptions {
                    keychain: SyncKeychain::Vanilla {
                        lookback: self.vanilla_sync_lookback(),
                    },
                    strategy: SyncStrategy::FastSync,
                },
                false,
            )?;
        }

        let script_pubkey = self.get_script_pubkey(&address)?;

        let unspendable = self.get_unspendable_bdk_outpoints(txn)?;

        let mut tx_builder = self.bdk_wallet_mut().build_tx();
        tx_builder
            .unspendable(unspendable)
            .add_recipient(script_pubkey, BdkAmount::from_sat(amount))
            .fee_rate(fee_rate_checked);
        let psbt = tx_builder.finish().map_err(|e| match e {
            bdk_wallet::error::CreateTxError::CoinSelection(InsufficientFunds {
                needed,
                available,
            }) => Error::InsufficientBitcoins {
                needed: needed.to_sat(),
                available: available.to_sat(),
            },
            bdk_wallet::error::CreateTxError::OutputBelowDustLimit(_) => {
                Error::OutputBelowDustLimit
            }
            _ => Error::Internal {
                details: e.to_string(),
            },
        })?;

        if !dry_run {
            self.reserve_vanilla_txos(txn, &psbt, WalletTransactionType::SendBtc)?;
        }

        Ok(psbt)
    }

    fn send_btc_end_impl(&mut self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<String, Error> {
        let tx = self.broadcast_psbt(txn, signed_psbt)?;
        self.finalize_vanilla_wallet_transaction(txn, signed_psbt, WalletTransactionType::SendBtc)?;
        Ok(tx.compute_txid().to_string())
    }

    fn inflate_begin_impl(
        &mut self,
        txn: &DbTxn,
        asset_id: String,
        inflation_amounts: Vec<u64>,
        fee_rate: u64,
        min_confirmations: u8,
        dry_run: bool,
    ) -> Result<BeginOperationData, Error> {
        let asset = txn.check_asset_exists(asset_id.clone())?;
        let schema = asset.schema;
        self.check_schema_support(&schema)?;
        if !SCHEMAS_SUPPORTING_INFLATION.contains(&schema) {
            return Err(Error::UnsupportedInflation {
                asset_schema: schema,
            });
        }

        let known_circulating_supply = asset
            .known_circulating_supply
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let inflation =
            self.get_total_inflation_amount(&inflation_amounts, known_circulating_supply)?;
        if inflation == 0 {
            return Err(Error::NoInflationAmounts);
        }

        let (fee_rate_checked, unspents, input_unspents, mut runtime) =
            self.get_transfer_begin_data(txn, fee_rate)?;

        let assignments_needed = AssignmentsCollection {
            inflation,
            ..Default::default()
        };
        let asset_spend = self.select_rgb_inputs(
            asset_id.clone(),
            &assignments_needed,
            input_unspents.clone(),
        )?;

        let chainnet: ChainNet = self.bitcoin_network().into();
        let amount_sat = asset_spend.input_btc_amt / inflation_amounts.len() as u64;
        let dust = self
            .bdk_wallet()
            .public_descriptor(KeychainKind::External)
            .dust_value()
            .to_sat();
        let amount_sat = max(amount_sat, dust);
        let mut local_recipients = vec![];
        let mut witness_recipients: Vec<(ScriptBuf, u64)> = vec![];
        for (idx, amt) in inflation_amounts.iter().enumerate() {
            let script_pubkey = self
                .get_new_addresses(KeychainKind::External, 1)?
                .script_pubkey();
            let beneficiary = beneficiary_from_script_buf(script_pubkey.clone());
            let beneficiary = XChainNet::with(chainnet, beneficiary);
            let recipient_id = beneficiary.to_string();
            witness_recipients.push((script_pubkey, amount_sat));
            let vout = idx as u32 + 1; // start from 1 because of OP_RETURN
            local_recipients.push(LocalRecipient {
                recipient_id,
                local_recipient_data: LocalRecipientData::Witness(LocalWitnessData {
                    amount_sat,
                    blinding: None,
                    vout,
                }),
                assignment: Assignment::Fungible(*amt),
                transport_endpoints: vec![],
            })
        }

        let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
        let asset_info = AssetInfo {
            contract_id,
            reject_list_url: asset.reject_list_url,
        };
        let transfer_info = InfoAssetTransfer {
            asset_info,
            recipients: local_recipients.clone(),
            asset_spend: asset_spend.clone(),
            change: AssignmentsCollection::default(),
            original_assignments_needed: assignments_needed.clone(),
            assignments_needed,
            assignments_spent: HashMap::new(),
            main_transition: TypeOfTransition::Inflate,
            beneficiaries_blinded: vec![],
            beneficiaries_witness: vec![],
        };
        let mut transfer_info_map: BTreeMap<String, InfoAssetTransfer> =
            BTreeMap::from([(asset_id.clone(), transfer_info)]);

        let receive_ids: Vec<String> = local_recipients
            .iter()
            .map(|lr| lr.recipient_id.clone())
            .collect();
        let transfer_dir = self.setup_transfer_directory(receive_ids)?;

        let mut rejected = HashSet::new();
        Ok(
            match self.prepare_transfer_psbt(
                txn,
                &mut transfer_info_map,
                transfer_dir.clone(),
                false,
                unspents,
                &input_unspents,
                &witness_recipients,
                fee_rate_checked,
                min_confirmations,
                None,
                &mut runtime,
                &mut rejected,
                dry_run,
            )? {
                PrepareTransferPsbtResult::Retry => {
                    unreachable!("inflate transition has no retry logic")
                }
                PrepareTransferPsbtResult::Success(begin_operation_data) => *begin_operation_data,
            },
        )
    }

    fn inflate_end_impl(
        &mut self,
        txn: &DbTxn,
        signed_psbt: &Psbt,
    ) -> Result<OperationResult, Error> {
        let (txid, _transfer_dir, info_contents, fascia) =
            self.get_transfer_end_data(signed_psbt)?;

        let batch_transfer_idx = self.finalize_transfer_end(
            txn,
            txid.clone(),
            signed_psbt,
            &info_contents,
            TransferStatus::WaitingConfirmations,
            fascia,
            false,
        )?;

        let (asset_id, transfer_info) = info_contents.transfers.into_iter().next().unwrap();
        let inflation = transfer_info.original_assignments_needed.inflation;
        let db_asset = txn.get_asset(asset_id).unwrap().unwrap();
        let updated_known_circulating_supply = db_asset
            .known_circulating_supply
            .as_ref()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            + inflation;
        let mut updated_asset: DbAssetActMod = db_asset.into();
        updated_asset.known_circulating_supply =
            ActiveValue::Set(Some(updated_known_circulating_supply.to_string()));
        txn.update_asset(&mut updated_asset)?;

        Ok(OperationResult {
            txid,
            batch_transfer_idx,
            entropy: info_contents.entropy,
        })
    }

    fn burn_begin_impl(
        &mut self,
        txn: &DbTxn,
        asset_id: String,
        amount: u64,
        fee_rate: u64,
        min_confirmations: u8,
        dry_run: bool,
    ) -> Result<BeginOperationData, Error> {
        let asset = txn.check_asset_exists(asset_id.clone())?;
        let schema = asset.schema;
        self.check_schema_support(&schema)?;
        if !SCHEMAS_SUPPORTING_BURN.contains(&schema) {
            return Err(Error::UnsupportedBurn {
                asset_schema: schema,
            });
        }

        if amount == 0 {
            return Err(Error::NoBurnAmount);
        }

        let (fee_rate_checked, unspents, input_unspents, mut runtime) =
            self.get_transfer_begin_data(txn, fee_rate)?;

        let assignments_needed = AssignmentsCollection {
            fungible: amount,
            ..Default::default()
        };
        let asset_spend = self.select_rgb_inputs(
            asset_id.clone(),
            &assignments_needed,
            input_unspents.clone(),
        )?;

        let chainnet: ChainNet = self.bitcoin_network().into();
        let script_pubkey = self
            .get_new_addresses(KeychainKind::External, 1)?
            .script_pubkey();
        let dust = self
            .bdk_wallet()
            .public_descriptor(KeychainKind::External)
            .dust_value()
            .to_sat();
        let witness_recipients: Vec<(ScriptBuf, u64)> = vec![(script_pubkey.clone(), dust)];
        let beneficiary = beneficiary_from_script_buf(script_pubkey.clone());
        let beneficiary = XChainNet::with(chainnet, beneficiary);
        let recipient_id = beneficiary.to_string();
        let local_recipients = vec![LocalRecipient {
            recipient_id,
            local_recipient_data: LocalRecipientData::Witness(LocalWitnessData {
                amount_sat: dust,
                blinding: None,
                vout: 1,
            }),
            assignment: Assignment::Fungible(0),
            transport_endpoints: vec![],
        }];

        let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
        let asset_info = AssetInfo {
            contract_id,
            reject_list_url: asset.reject_list_url,
        };
        let transfer_info = InfoAssetTransfer {
            asset_info,
            recipients: local_recipients.clone(),
            asset_spend: asset_spend.clone(),
            change: AssignmentsCollection::default(),
            original_assignments_needed: assignments_needed.clone(),
            assignments_needed,
            assignments_spent: HashMap::new(),
            main_transition: TypeOfTransition::Burn,
            beneficiaries_blinded: vec![],
            beneficiaries_witness: vec![],
        };
        let mut transfer_info_map: BTreeMap<String, InfoAssetTransfer> =
            BTreeMap::from([(asset_id.clone(), transfer_info)]);

        let receive_ids: Vec<String> = local_recipients
            .iter()
            .map(|lr| lr.recipient_id.clone())
            .collect();
        let transfer_dir = self.setup_transfer_directory(receive_ids)?;

        let mut rejected = HashSet::new();
        Ok(
            match self.prepare_transfer_psbt(
                txn,
                &mut transfer_info_map,
                transfer_dir.clone(),
                false,
                unspents,
                &input_unspents,
                &witness_recipients,
                fee_rate_checked,
                min_confirmations,
                None,
                &mut runtime,
                &mut rejected,
                dry_run,
            )? {
                PrepareTransferPsbtResult::Retry => {
                    unreachable!("burn transition has no retry logic")
                }
                PrepareTransferPsbtResult::Success(begin_operation_data) => *begin_operation_data,
            },
        )
    }

    fn burn_end_impl(&mut self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<OperationResult, Error> {
        let (txid, _transfer_dir, info_contents, fascia) =
            self.get_transfer_end_data(signed_psbt)?;

        let batch_transfer_idx = self.finalize_transfer_end(
            txn,
            txid.clone(),
            signed_psbt,
            &info_contents,
            TransferStatus::WaitingConfirmations,
            fascia,
            false,
        )?;

        Ok(OperationResult {
            txid,
            batch_transfer_idx,
            entropy: info_contents.entropy,
        })
    }
}

/// Online operations for a wallet.
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub trait RgbWalletOpsOnline: RgbWalletOpsOffline + WalletOnline {
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
    fn fail_transfers(
        &mut self,
        online: Online,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
        skip_sync: bool,
    ) -> Result<bool, Error> {
        info!(
            self.logger(),
            "Failing batch transfer with idx {:?}...", batch_transfer_idx
        );
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let outcome =
            self.fail_transfers_impl(&txn, batch_transfer_idx, no_asset_only, skip_sync)?;
        if outcome.transfers_changed {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Fail transfers completed");
        if outcome.cannot_fail {
            return Err(Error::CannotFailBatchTransfer);
        }
        Ok(outcome.transfers_changed)
    }

    /// Sync the wallet and save new colored UTXOs to the DB.
    ///
    /// Gets [`SyncOptions`] to configure the sync strategy and keychain.
    ///
    /// Callers that want both keychains synced must invoke this method once per keychain.
    fn sync(&mut self, online: Online, options: SyncOptions) -> Result<(), Error> {
        info!(self.logger(), "Syncing...");
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        self.sync_impl(&txn, options)?;
        txn.commit()?;
        info!(self.logger(), "Sync completed");
        Ok(())
    }

    /// Return the fee estimation in sat/vB for the requested number of `blocks`.
    ///
    /// The `blocks` parameter must be between 1 and 1008.
    fn get_fee_estimation(&self, online: Online, blocks: u16) -> Result<f64, Error> {
        info!(self.logger(), "Getting fee estimation...");
        self.check_online(online)?;
        let estimation = self.get_fee_estimation_impl(blocks)?;
        info!(self.logger(), "Get fee estimation completed");
        Ok(estimation)
    }

    /// Update pending RGB transfers, based on their current status, and return a
    /// [`RefreshResult`].
    ///
    /// An optional `asset_id` can be provided to refresh transfers related to a specific asset.
    ///
    /// Each item in the [`RefreshFilter`] vector defines transfers to be refreshed. Transfers not
    /// matching any provided filter are skipped. If the vector is empty, all transfers are
    /// refreshed.
    fn refresh(
        &mut self,
        online: Online,
        asset_id: Option<String>,
        filter: Vec<RefreshFilter>,
        skip_sync: bool,
    ) -> Result<RefreshResult, Error> {
        info!(self.logger(), "Refreshing asset {:?}...", asset_id);
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        if let Some(aid) = &asset_id {
            txn.check_asset_exists(aid.clone())?;
        }
        let res = self.refresh_impl(&txn, asset_id, filter, skip_sync)?;
        if res.transfers_changed() {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Refresh completed");
        Ok(res)
    }
}
