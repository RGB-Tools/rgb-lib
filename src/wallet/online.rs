//! Online functionality.
//!
//! This module defines the online wallet methods.

use super::*;

const SIGNED_PSBT_FILE: &str = "signed.psbt";
/// Signed TX released to a recipient, saved to broadcast it again until the broadcast deadline
const RECEIVED_SIGNED_TX_FILE: &str = "rcv_signed.tx";
/// Last external chain blocks scanned for the mint events of a received consignment, saved to
/// resume the scans at the next refresh
const MINT_SCAN_FILE: &str = "rcv_mint_scan.json";

const MIN_FEE_RATE: u64 = 1;

pub(crate) const UTXO_SIZE: u32 = 1000;
pub(crate) const UTXO_NUM: u8 = 5;

pub(crate) const MIN_BLOCK_ESTIMATION: u16 = 1;
pub(crate) const MAX_BLOCK_ESTIMATION: u16 = 1008;

/// Return the indexer protocol for the provided URL.
/// An error is raised if the provided indexer URL is invalid or if the service is for the wrong
/// network or doesn't have the required functionality.
///
/// A Bitcoin Core node is only detected when `bitcoind_auth` is provided, otherwise the URL is
/// probed for an electrum server first and assumed to be an esplora one if that fails. See
/// [`IndexerConfig`] for the meaning of the returned protocol.
pub fn check_indexer_url(
    indexer_url: &str,
    bitcoind_auth: Option<&BitcoindAuth>,
    bitcoin_network: BitcoinNetwork,
) -> Result<IndexerProtocol, Error> {
    let indexer_config =
        detect_indexer(indexer_url, bitcoind_auth).ok_or_else(|| Error::InvalidIndexer {
            details: if bitcoind_auth.is_some() {
                s!("bitcoind support is not enabled (requires the `bitcoind` feature)")
            } else {
                s!("not a valid electrum nor esplora server")
            },
        })?;
    get_indexer_and_resolver(&indexer_config, bitcoin_network).map_err(|e| {
        // the esplora config is a blind fallback guess (see detect_indexer), so an unreachable
        // esplora server means neither probed protocol answered
        match (&indexer_config, &e) {
            (IndexerConfig::Esplora { .. }, Error::InvalidIndexer { details })
                if details == "not a valid esplora server" =>
            {
                Error::InvalidIndexer {
                    details: s!("not a valid electrum nor esplora server"),
                }
            }
            _ => e,
        }
    })?;

    Ok(indexer_config.protocol())
}

/// Parse and validate the [`OnlineOptions::proxy_auth_keys`], keying them by proxy URL.
fn parse_proxy_auth_keys(
    proxy_auth_keys: &HashMap<String, String>,
) -> Result<HashMap<String, ed25519_dalek::SigningKey>, Error> {
    let mut keys = HashMap::new();
    for (endpoint, seed) in proxy_auth_keys {
        let proxy_url = TransportEndpoint::new(endpoint.clone())?.endpoint;
        let seed: [u8; 32] = hex::decode(seed)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| Error::InvalidProxyAuthKey {
                details: format!(
                    "expected a hex-encoded 32-byte Ed25519 seed for endpoint {endpoint}"
                ),
            })?;
        let signing_key = ed25519_from_seed(&seed);
        if keys.insert(proxy_url.clone(), signing_key).is_some() {
            return Err(Error::DuplicateProxyAuthKey {
                details: format!(
                    "endpoint {endpoint} resolves to a proxy URL ({proxy_url}) already covered by another entry"
                ),
            });
        }
    }
    Ok(keys)
}

/// Request the ACKs of the operation of `transfer_ids` from the sender proxy (`ack.get`), signing
/// the request with the operation (sender) key.
fn get_ack_signed(
    proxy_client: &ProxyClient,
    transfer_ids: &[String],
    sender_sk: &ed25519_dalek::SigningKey,
) -> Result<JsonRpcResponse<GetAckResponse>, Error> {
    let timestamp = now().unix_timestamp().to_string();
    let digest = sha256_digest(get_ack_message(transfer_ids, &timestamp).as_bytes());
    let signature = hex::encode(sign_ed25519(sender_sk, &digest));
    proxy_client.get_ack(transfer_ids, &signature, &timestamp)
}

/// List the transfers to an invoice on a recipient proxy (`transfer.list`), signing the request
/// with the recipient key.
fn list_invoice_transfers(
    proxy_url: &str,
    invoice_id: &str,
    recipient_seed: &[u8],
) -> Result<JsonRpcResponse<HashMap<String, ProxyTransferState>>, Error> {
    let seed: [u8; 32] = recipient_seed.try_into().map_err(|_| Error::Internal {
        details: s!("invalid recipient secret key"),
    })?;
    let sk = ed25519_from_seed(&seed);
    let timestamp = now().unix_timestamp().to_string();
    let digest = sha256_digest(list_transfers_message(invoice_id, &timestamp).as_bytes());
    let signature = hex::encode(sign_ed25519(&sk, &digest));
    ProxyClient::new(proxy_url)?.list_transfers(invoice_id, &signature, &timestamp)
}

/// Whether the sender proxy knows the operation of `transfer_ids`, registered with the operation
/// (sender) key, i.e. whether an earlier `ack.prepare` for it succeeded.
fn sender_proxy_knows_operation(
    proxy_client: &ProxyClient,
    transfer_ids: &[String],
    sender_sk: &ed25519_dalek::SigningKey,
) -> Result<bool, Error> {
    match get_ack_signed(proxy_client, transfer_ids, sender_sk)?.error {
        None => Ok(true),
        Some(err) => match map_proxy_error(err) {
            Error::OperationNotFound => Ok(false),
            e => Err(e),
        },
    }
}

/// Interpret the sender proxy's response to `ack.clean`, requested before deliberately failing
/// an outgoing operation.
///
/// Any proxy response other than the ones mapped to a [`CleanAckOutcome`] cannot be unambiguously
/// interpreted safely and is surfaced as an error.
fn clean_ack_outcome(error: Option<JsonRpcError>) -> Result<CleanAckOutcome, Error> {
    match error {
        None => Ok(CleanAckOutcome::Revoked),
        Some(err) => match map_proxy_error(err) {
            Error::ProxyTxAlreadyReleased => Ok(CleanAckOutcome::Released),
            Error::OperationNotFound => Ok(CleanAckOutcome::Unknown),
            e => Err(e),
        },
    }
}

fn clock_skew_margin() -> u32 {
    #[cfg(test)]
    if let Some(margin) = mock_clock_skew_margin() {
        return margin;
    }
    PROXY_CLOCK_SKEW_MARGIN_SECS
}

/// Whether the expiration of an outgoing operation has passed on the local clock by more than the
/// clock skew margin, so it has passed on the sender proxy's clock as well and the proxy can no
/// longer release the signed TX, whatever it knows about the operation.
fn expired_beyond_skew_margin(batch_transfer: &DbBatchTransfer) -> bool {
    batch_transfer.is_expired_with_margin(now().unix_timestamp(), clock_skew_margin())
}

/// Whether the broadcast deadline of an outgoing operation (its expiration plus the sender proxy
/// retention grace, or just its expiration if unknown) has passed on the local clock by more than
/// the clock skew margin, so the sender proxy can no longer hand out the signed TX, not even to
/// recipients that already obtained it, and recipients stopped broadcasting it
/// (`v1.0-changes-spec.md` 6.2).
fn broadcast_deadline_passed_beyond_skew_margin(batch_transfer: &DbBatchTransfer) -> bool {
    batch_transfer
        .broadcast_deadline
        .or(batch_transfer.expiration)
        .is_some_and(|deadline| deadline + i64::from(clock_skew_margin()) < now().unix_timestamp())
}

/// Whether the broadcast deadline of an incoming transfer plus the confirmation margin has passed,
/// after which a transfer whose TX is not confirmed is failed (`v1.0-changes-spec.md` 6.4). A
/// transfer with no known broadcast deadline is never considered past it.
fn confirmation_deadline_passed(batch_transfer: &DbBatchTransfer) -> bool {
    #[cfg(test)]
    let margin = mock_confirmation_margin().unwrap_or(CONFIRMATION_MARGIN_SECS);
    #[cfg(not(test))]
    let margin = CONFIRMATION_MARGIN_SECS;
    batch_transfer
        .broadcast_deadline
        .is_some_and(|deadline| deadline + i64::from(margin) < now().unix_timestamp())
}

/// Whether an incoming transfer is expired, so it can be failed: a transfer waiting for the
/// broadcast of a TX the sender proxy may still release to it (it has a broadcast deadline) only
/// once past the broadcast deadline plus the confirmation margin, a blind receive reserved on
/// proxies (it has a retention deadline) still waiting for the counterparty only once the invoice
/// expiration has passed on the proxies' clocks too, as they accept transfers until then, any
/// other once past the invoice expiration.
fn incoming_expired(batch_transfer: &DbBatchTransfer, now: i64) -> bool {
    if batch_transfer.status == TransferStatus::WaitingBroadcast
        && batch_transfer.broadcast_deadline.is_some()
    {
        return confirmation_deadline_passed(batch_transfer);
    }
    if batch_transfer.status == TransferStatus::WaitingCounterparty
        && batch_transfer.retention_deadline.is_some()
    {
        return batch_transfer.is_expired_with_margin(now, clock_skew_margin());
    }
    batch_transfer.is_expired(now)
}

/// The retention grace advertised by a sender proxy (`retention_grace_secs`), i.e. how long
/// after an operation expiration it keeps a released signed TX available to the recipients. A
/// proxy not advertising it is considered to grant none.
fn sender_proxy_retention_grace(proxy_url: &str) -> Result<u32, Error> {
    #[cfg(test)]
    if let Some(grace) = mock_retention_grace() {
        return Ok(grace);
    }
    let res = ProxyClient::new(proxy_url)?.get_info()?;
    if let Some(err) = res.error {
        return Err(map_proxy_error(err));
    }
    let info = res.result.ok_or_else(|| Error::Proxy {
        details: s!("missing server.info result"),
    })?;
    Ok(proxy_retention_grace(&info))
}

/// The minimum sender proxy retention grace a recipient requires to ACK a transfer.
fn min_sender_retention_grace() -> u32 {
    #[cfg(test)]
    if let Some(grace) = mock_min_retention_grace() {
        return grace;
    }
    MIN_SENDER_RETENTION_GRACE_SECS
}

/// The broadcast deadline of an operation from the `expires_at` returned by `ack.post` and the
/// sender proxy retention grace, `None` if the proxy didn't return a valid expiration.
fn broadcast_deadline(expires_at: Option<&str>, retention_grace: u32) -> Option<i64> {
    expires_at
        .and_then(|exp| exp.parse::<i64>().ok())
        .map(|exp| exp + i64::from(retention_grace))
}

/// Minimum time between the expiration of an outgoing operation and the earliest invoice
/// expiration among its recipients (see [`INVOICE_EXPIRATION_MARGIN_SECS`]), following the clock
/// skew margin it accounts for.
fn invoice_expiration_margin() -> u32 {
    #[cfg(test)]
    if let Some(skew_margin) = mock_clock_skew_margin() {
        return 2 * skew_margin;
    }
    INVOICE_EXPIRATION_MARGIN_SECS
}

/// A received consignment that passed validation and the checks on what it pays the recipient:
/// the validated consignment, its validation status and the assignments received.
type CheckedConsignment = (ValidTransfer, ValidationStatus, HashMap<Opout, Assignment>);

/// Minimum time, in seconds, between the creation of an outgoing operation and its expiration: an
/// operation expiring sooner than the clock skew margin could already be expired on the sender
/// proxy's clock.
fn min_operation_lifetime() -> u32 {
    #[cfg(test)]
    if let Some(skew_margin) = mock_clock_skew_margin() {
        return skew_margin;
    }
    PROXY_CLOCK_SKEW_MARGIN_SECS
}

/// Minimum time the recipient's invoice must have left when a mint begins (see
/// [`MIN_MINT_LOCK_WINDOW_SECS`]).
fn mint_after_block_grace() -> i64 {
    #[cfg(test)]
    if let Some(grace) = mock_mint_after_block_grace() {
        return grace;
    }
    MINT_AFTER_BLOCK_GRACE_SECS
}

fn min_mint_lock_window() -> u32 {
    #[cfg(test)]
    if let Some(window) = mock_mint_lock_window() {
        return window;
    }
    MIN_MINT_LOCK_WINDOW_SECS
}

/// Check the expiration of an outgoing operation leaves it enough time (see
/// [`min_operation_lifetime`]).
fn check_operation_expiration(expiration_timestamp: i64) -> Result<(), Error> {
    if expiration_timestamp < now().unix_timestamp() + min_operation_lifetime() as i64 {
        return Err(Error::InvalidExpiration);
    }
    Ok(())
}

/// Interpret an error returned by the sender proxy to `ack.get`, requested while waiting for the
/// recipients' ACKs.
///
/// The only error with a safe interpretation is the proxy no longer knowing the operation (e.g.
/// it dropped its data): the ACKs cannot be retrieved anymore, so the caller falls back to
/// checking whether the TX was broadcast. That case returns `Ok(())`. Any other proxy response
/// cannot be unambiguously interpreted safely and is surfaced as an error.
fn check_get_ack_error(err: JsonRpcError) -> Result<(), Error> {
    match map_proxy_error(err) {
        Error::OperationNotFound => Ok(()),
        e => Err(e),
    }
}

pub trait WalletOnline: WalletOffline {
    fn blockchain_resolver(&self) -> ChainResolver<'_> {
        ChainResolver {
            inner: &self.online_data().as_ref().unwrap().resolver,
        }
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
                    #[cfg(feature = "bitcoind")]
                    IndexerError::Bitcoind(ref e) => {
                        let err_str = e.to_string();
                        if err_str.contains("min relay fee not met")
                            || err_str.contains("mempool min fee not met")
                        {
                            return Err(Error::MinFeeNotMet { txid: txid.clone() });
                        } else if err_str.contains("Fee exceeds maximum configured")
                            || err_str.contains("max-fee-exceeded")
                        {
                            return Err(Error::MaxFeeExceeded { txid: txid.clone() });
                        }
                    }
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
        self.bdk_wallet_mut()
            .apply_unconfirmed_txs([(tx.clone(), seen_at)]);
        // the TX is on the network now and the enclosing transaction may still fail or take a
        // while to commit: keep a copy on disk so a crash cannot lose it
        self.flush_bdk_pending()?;

        // promote any newly-known colored UTXOs (e.g. the change output) from
        // exists=false to exists=true in the rgb_lib DB
        let pending_witness_scripts = self.reconcile_pending_witness(txn)?;
        self.update_db_colored_txos_from_bdk(txn, false, &pending_witness_scripts)?;

        let input_outpoints: Vec<Outpoint> = tx
            .input
            .iter()
            .map(|i| Outpoint::from(i.previous_output))
            .collect();
        for outpoint in &input_outpoints {
            if let Some(db_txo) = txn.get_txo(outpoint)? {
                let mut db_txo: DbTxoActMod = db_txo.into();
                db_txo.spent = ActiveValue::Set(true);
                txn.update_txo(db_txo)?;
            }
        }
        // the inputs are spent now, release the reservations of the pending transfer
        txn.del_colored_reserved_txos_by_outpoints(&input_outpoints)?;

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
                reserved_for_vanilla: ActiveValue::Set(Some(wt_idx)),
                ..Default::default()
            })
            .collect();
        txn.set_reserved_txos(reservations)?;
        Ok(())
    }

    fn reserve_colored_txos(
        &self,
        txn: &DbTxn,
        psbt: &Psbt,
        batch_transfer_idx: i32,
    ) -> Result<(), Error> {
        let reservations: Vec<DbReservedTxoActMod> = psbt
            .unsigned_tx
            .input
            .iter()
            .map(|i| DbReservedTxoActMod {
                txid: ActiveValue::Set(i.previous_output.txid.to_string()),
                vout: ActiveValue::Set(i.previous_output.vout),
                reserved_for_colored: ActiveValue::Set(Some(batch_transfer_idx)),
                ..Default::default()
            })
            .collect();
        txn.set_reserved_txos(reservations)?;
        Ok(())
    }

    /// Check that the PSBT being ended is of the given vanilla operation: an RGB operation (which
    /// has a transfer directory) or another vanilla one (as recorded by a non-dry-run begin) would
    /// be completed skipping its checks and steps.
    fn check_vanilla_end(
        &self,
        txn: &DbTxn,
        psbt: &Psbt,
        r#type: WalletTransactionType,
    ) -> Result<(), Error> {
        let txid = psbt.unsigned_tx.compute_txid().to_string();
        if self.get_transfer_dir(&txid).exists() {
            return Err(Error::WrongOperation { txid });
        }
        if let Some((wallet_transaction, _)) =
            txn.get_wallet_transaction_with_reserved_txos_by_txid(&txid)?
            && wallet_transaction.r#type != r#type
        {
            return Err(Error::WrongOperation { txid });
        }
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
        stock: &mut RgbStock<'_>,
        signed_psbt: &Psbt,
        fascia: Fascia,
    ) -> Result<BdkTransaction, Error> {
        let tx = self.broadcast_psbt(txn, signed_psbt)?;
        stock.consume_fascia(fascia, None)?;
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
        let mut psbt = tx_builder.finish()?;
        self.add_non_witness_utxos(&mut psbt);
        Ok(psbt)
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
            let reserved = self.get_reserved_colored_outpoints(txn)?;
            let allocatable = self
                .get_available_allocations(unspents, &reserved, None)?
                .len();
            // compare in usize since the count of allocatable UTXOs can exceed u8::MAX
            if allocatable >= utxos_to_create as usize {
                return Err(Error::AllocationsAlreadyAvailable);
            }
            // allocatable < utxos_to_create <= u8::MAX, so the conversion cannot fail
            utxos_to_create -=
                u8::try_from(allocatable).expect("allocatable count cannot exceed u8::MAX");
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
                    let usable_btc_amount = usable_btc_amount
                        .checked_add(value)
                        .expect("total UTXO value cannot exceed u64::MAX");
                    (inputs, usable_btc_amount)
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
        self.check_vanilla_end(txn, signed_psbt, WalletTransactionType::CreateUtxos)?;
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

        let mut psbt = tx_builder.finish().map_err(|e| match e {
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
        self.add_non_witness_utxos(&mut psbt);

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
        self.check_vanilla_end(txn, signed_psbt, WalletTransactionType::Drain)?;
        self.finalize_vanilla_wallet_transaction(txn, signed_psbt, WalletTransactionType::Drain)?;
        let tx = self.broadcast_psbt(txn, signed_psbt)?;
        Ok(tx)
    }

    fn supports_out_of_band_exchange(&self) -> bool {
        true
    }

    /// Whether the data of a handled incoming transfer can be deleted from the recipient proxy
    /// (`transfer.retrieved`), which is not the case when other wallets sharing the invoice (e.g.
    /// multisig cosigners) still need to download it.
    fn releases_received_transfer_data(&self) -> bool {
        true
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

    /// Clear the pending witness flag of the TXOs flagged for witness receives that are not pending
    /// anymore, as they would otherwise never become spendable. The TXOs are identified by their
    /// outpoint (the one registered when accepting the consignment) or by the script they pay (one
    /// detected by a sync, e.g. when the counterparty broadcasts before its consignment gets
    /// refused). Pending witness scripts are left to [`WalletCore::reconcile_pending_witness`],
    /// which keeps watching them for a grace time in case the counterparty broadcasts anyway.
    fn unflag_pending_witness_txos(
        &self,
        txn: &DbTxn,
        outpoints: &HashSet<Outpoint>,
        scripts: &HashSet<ScriptBuf>,
    ) -> Result<(), Error> {
        for txo in txn.iter_pending_witness_txos()? {
            let outpoint = txo.outpoint();
            let pays_script = || {
                self.bdk_wallet()
                    .tx_graph()
                    .get_txout(outpoint.clone().into())
                    .is_some_and(|txout| scripts.contains(&txout.script_pubkey))
            };
            if !outpoints.contains(&outpoint) && !pays_script() {
                continue;
            }
            let mut txo: DbTxoActMod = txo.into();
            txo.pending_witness = ActiveValue::Set(false);
            txn.update_txo(txo)?;
        }
        Ok(())
    }

    /// Clear the pending witness flag of the TXOs flagged for the witness receives of a batch
    /// transfer being failed.
    fn clean_failed_witness_receives(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<(), Error> {
        if !batch_transfer.incoming {
            return Ok(());
        }
        let mut outpoints = HashSet::new();
        let mut scripts = HashSet::new();
        for transfer in txn.get_batch_transfer_transfers(batch_transfer.idx)? {
            let vout = match transfer.recipient_type {
                Some(RecipientTypeFull::Witness { vout }) => vout,
                _ => continue,
            };
            if let (Some(txid), Some(vout)) = (&batch_transfer.txid, vout) {
                outpoints.insert(Outpoint {
                    txid: txid.clone(),
                    vout,
                });
            }
            if let Some(recipient_id) = transfer.recipient_id
                && let Some(script) = script_buf_from_recipient_id(recipient_id)?
            {
                scripts.insert(script);
            }
        }
        if outpoints.is_empty() && scripts.is_empty() {
            return Ok(());
        }
        self.unflag_pending_witness_txos(txn, &outpoints, &scripts)
    }

    fn fail_batch_transfer(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<DbBatchTransfer, Error> {
        self.set_hub_fail_status(batch_transfer.idx)?;
        self.clean_failed_witness_receives(txn, batch_transfer)?;
        txn.del_reserved_txos_for_batch_transfer(batch_transfer.idx)?;
        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        txn.update_batch_transfer(&mut updated_batch_transfer)
    }

    /// Revoke the stored signed TXs for an outgoing operation on the sender proxy (`ack.clean`).
    /// An operation with nothing registered on a sender proxy (legacy, or whose recipients are
    /// all out-of-band) has nothing to revoke and is reported as [`CleanAckOutcome::Revoked`].
    fn clean_sender_ack(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<CleanAckOutcome, Error> {
        let Some(seed) = &batch_transfer.operation_secret_key else {
            // legacy: nothing to clean
            return Ok(CleanAckOutcome::Revoked);
        };
        let bt_data = batch_transfer.get_transfers(&db_data.asset_transfers, &db_data.transfers)?;
        // only the transfers registered on the sender proxy (out-of-band ones are not)
        let mut transfer_ids: Vec<String> = vec![];
        let mut sender_proxy = None;
        for atd in &bt_data.asset_transfers_data {
            for t in &atd.transfers {
                let transfer_sender_proxy = txn
                    .get_transfer_transport_endpoints_data(t.idx)?
                    .into_iter()
                    .find(|(tte, _)| tte.kind == TransportEndpointKind::SenderProxy)
                    .map(|(_, ce)| ce.endpoint);
                let Some(transfer_sender_proxy) = transfer_sender_proxy else {
                    continue;
                };
                if let Some(tid) = &t.proxy_transfer_id {
                    transfer_ids.push(tid.clone());
                }
                if sender_proxy.is_none() {
                    sender_proxy = Some(transfer_sender_proxy);
                }
            }
        }
        let Some(sender_proxy) = sender_proxy else {
            // no sender proxy (all recipients out-of-band): nothing to clean
            return Ok(CleanAckOutcome::Revoked);
        };
        transfer_ids.sort();
        let seed: [u8; 32] = seed.clone().try_into().map_err(|_| Error::Internal {
            details: s!("invalid operation secret key"),
        })?;
        let sk = ed25519_from_seed(&seed);
        let digest = sha256_digest(transfer_ids.concat().as_bytes());
        let sig = hex::encode(sign_ed25519(&sk, &digest));
        let proxy_client = ProxyClient::new(&sender_proxy)?;
        let res = proxy_client.clean_ack(&transfer_ids, &sig)?;
        clean_ack_outcome(res.error)
    }

    fn try_fail_batch_transfer(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
        revoke_released: bool,
    ) -> Result<TryFailBatchTransferOutcome, Error> {
        // an outgoing transfer is expired once its expiration has passed on the sender proxy's
        // clock too, as the proxy may release its TX until then
        let expired = if batch_transfer.incoming {
            incoming_expired(batch_transfer, now().unix_timestamp())
        } else {
            expired_beyond_skew_margin(batch_transfer)
        };
        let updated_batch_transfer = match self.refresh_transfer(
            txn,
            batch_transfer,
            db_data,
            &[],
            true,
        ) {
            Err(Error::MinFeeNotMet { txid: _ }) | Err(Error::MaxFeeExceeded { txid: _ }) => {
                Ok(None)
            }
            // the sender proxy lost the operation or serves an invalid ACK signature: the
            // refresh fails the transfer itself once its broadcast deadline has passed, until
            // then the signed TX may still be broadcast by a recipient, so the fail is denied
            Err(Error::OperationNotFound) | Err(Error::InvalidContentSignature)
                if !batch_transfer.incoming =>
            {
                info!(
                    self.logger(),
                    "Sender proxy cannot be relied upon, fail denied until the broadcast deadline"
                );
                return Ok(TryFailBatchTransferOutcome::Denied);
            }
            // a transfer that keeps failing to refresh (e.g. its ACK can't be posted or
            // retrieved) would otherwise never be failed; once expired its TX should no longer
            // be broadcast by the sender, so it's safe to fail it, while before that the
            // error could be transient and the transfer could still complete
            Err(e) if expired => {
                warn!(
                    self.logger(),
                    "Failing expired transfer that failed to refresh: {e}"
                );
                Ok(None)
            }
            Err(e) => Err(e),
            Ok(v) => Ok(v),
        }?;
        // if the status changed after the refresh, the transfer progressed
        if let Some(updated) = updated_batch_transfer {
            // the refresh may have failed the batch itself (e.g. it discovered a recipient NACK);
            // that is exactly the outcome a deliberate fail wanted, so report it as failed
            if updated.status == TransferStatus::Failed {
                return Ok(TryFailBatchTransferOutcome::Failed);
            }
            return Ok(TryFailBatchTransferOutcome::Refreshed);
        }
        // the status didn't change: before failing an outgoing transfer make sure its TX is not
        // on-chain already (a recipient holding the TX may have broadcast it), in which case the
        // transfer progresses instead
        if !batch_transfer.incoming
            && batch_transfer.status == TransferStatus::WaitingCounterparty
            && self
                .complete_batch_if_tx_on_chain(txn, batch_transfer)?
                .is_some()
        {
            return Ok(TryFailBatchTransferOutcome::Refreshed);
        }
        // an outgoing transfer every recipient ACKed has had its TX released: if it's still waiting
        // for the counterparty (e.g. its TX is not broadcast as an invoice has expired) it can only
        // be failed after spending one of the TX inputs (v1.0-changes-spec.md 6.1), which is not
        // supported, or once one of them has been spent by another confirmed transaction, which the
        // refresh above detects
        if !batch_transfer.incoming && batch_transfer.status == TransferStatus::WaitingCounterparty
        {
            // read from the DB, as the refresh above may have just stored the ACKs
            let transfers = txn.get_batch_transfer_transfers(batch_transfer.idx)?;
            if !transfers.is_empty() && transfers.iter().all(|t| t.ack == Some(true)) {
                info!(
                    self.logger(),
                    "Every recipient ACKed the transfer, fail denied"
                );
                return Ok(TryFailBatchTransferOutcome::Denied);
            }
        }
        // for a deliberate fail of an outgoing non-donation operation, revoke the released signed
        // TXs on the sender proxy first
        if revoke_released && !batch_transfer.incoming {
            match self.clean_sender_ack(txn, batch_transfer, db_data)? {
                CleanAckOutcome::Revoked => {}
                // the TX was already released to a recipient, who may still broadcast it: deny
                CleanAckOutcome::Released => return Ok(TryFailBatchTransferOutcome::Denied),
                // the proxy no longer knows the operation (e.g. it dropped its data), so whether
                // the TX was released cannot be known. Only an operation that completed send_end
                // had a signed TX registered: as recommended by v1.0-changes-spec.md 6.2, it is
                // failed only once its broadcast deadline (expiration plus the proxy retention
                // grace) has passed on the proxy's clock too, since until then the proxy may
                // recover the operation (e.g. from a backup) and hand out the TX, while recipients
                // that obtained the TX have time to broadcast it, which the chain check above
                // detects. The residual risk of a TX broadcast after that is accepted
                CleanAckOutcome::Unknown
                    if batch_transfer.status == TransferStatus::WaitingCounterparty =>
                {
                    if !broadcast_deadline_passed_beyond_skew_margin(batch_transfer) {
                        info!(
                            self.logger(),
                            "Operation unknown to the sender proxy, fail denied until its broadcast deadline"
                        );
                        return Ok(TryFailBatchTransferOutcome::Denied);
                    }
                    warn!(
                        self.logger(),
                        "Failing an operation unknown to the sender proxy past its broadcast deadline: its TX can still be broadcast if it was released before the proxy lost the operation"
                    );
                }
                // an operation that didn't complete send_end (still Initiated) never had a signed
                // TX registered, so the proxy is expected not to know it and nothing can have
                // been released
                CleanAckOutcome::Unknown => {}
            }
        }
        // a blind receive keeps its blinded UTXO reserved while a transfer to the invoice can
        // still be downloaded
        if batch_transfer.incoming
            && batch_transfer.status == TransferStatus::WaitingCounterparty
            && !self.blind_receive_may_be_failed(txn, batch_transfer, db_data)?
        {
            info!(
                self.logger(),
                "A transfer to the blind invoice may still be downloaded, fail denied"
            );
            return Ok(TryFailBatchTransferOutcome::Denied);
        }
        self.fail_batch_transfer(txn, batch_transfer)?;
        Ok(TryFailBatchTransferOutcome::Failed)
    }

    /// Whether the given batch transfer is a mint that has not reached mint_end yet.
    ///
    /// A mint batch carries a single main transition, the mint of one asset (the allocations of
    /// other assets moved along are not among its transfers), so a mint is never mixed with other
    /// transitions. Were it ever, the whole batch would still follow the mint flow (posted before
    /// signing, never registered on the sender proxy, broadcast by mint_end), so any mint
    /// transition makes the batch a mint.
    fn is_pending_mint(batch_transfer: &DbBatchTransfer, db_data: &DbData) -> bool {
        if batch_transfer.incoming
            || batch_transfer.status != TransferStatus::Initiated
            || batch_transfer.txid.is_none()
        {
            return false;
        }
        db_data.asset_transfers.iter().any(|at| {
            at.batch_transfer_idx == batch_transfer.idx
                && at.transition_type == Some(TypeOfTransition::Mint)
        })
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

            // a transfer waiting for the counterparty's broadcast can only be failed once it has
            // expired, since the TX may still be broadcast before then (for a mint, until the
            // finality grace of a lock included in time has passed too)
            if batch_transfer.status == TransferStatus::WaitingBroadcast {
                let now = now().unix_timestamp();
                let expired =
                    self.is_expired_with_mint_grace(txn, &batch_transfer, &db_data, now)?;
                if !expired {
                    return Ok(FailTransfersOutcome {
                        transfers_changed: false,
                        cannot_fail: true,
                    });
                }
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
            // a deliberate fail (explicit batch idx) revokes any released signed TXs first
            if !matches!(
                self.try_fail_batch_transfer(txn, &batch_transfer, &db_data, true)?,
                TryFailBatchTransferOutcome::Failed
            ) {
                cannot_fail = true;
            }
        } else {
            // fail all expired transfers that are in a fallible status; an outgoing one is expired
            // once its expiration has passed on the sender proxy's clock too (ack.get reports no
            // expiration), as the proxy may release its TX until then. A mint is the exception: it
            // is never registered on the sender proxy and mint_end refuses to broadcast it past
            // its expiration on the local clock, so no margin is needed. A mint whose lock was
            // included on the external chain by the expiration is waited for past it, until the
            // lock is finalized or the finality grace has passed, on both sides
            let now = now().unix_timestamp();
            let mut expired_transfers = vec![];
            for t in db_data.batch_transfers.iter().filter(|t| t.is_fallible()) {
                let expired = if t.incoming || Self::is_pending_mint(t, &db_data) {
                    self.is_expired_with_mint_grace(txn, t, &db_data, now)?
                } else {
                    expired_beyond_skew_margin(t)
                };
                if expired {
                    expired_transfers.push(t);
                }
            }
            for batch_transfer in expired_transfers {
                if no_asset_only {
                    let connected_assets = batch_transfer
                        .get_asset_transfers(&db_data.asset_transfers)
                        .iter()
                        .any(|t| t.asset_id.is_some());
                    if connected_assets {
                        continue;
                    }
                }
                // expired transfers: the proxy operation has lapsed, no need to revoke released TXs
                if !matches!(
                    self.try_fail_batch_transfer(txn, batch_transfer, &db_data, false)?,
                    TryFailBatchTransferOutcome::Denied
                ) {
                    transfers_changed = true;
                }
            }
        }

        Ok(FailTransfersOutcome {
            transfers_changed,
            cannot_fail,
        })
    }

    fn wallet_specific_consistency_checks(&mut self, _txn: &DbTxn) -> Result<(), Error>;

    fn check_consistency(&mut self, txn: &DbTxn) -> Result<(), Error> {
        info!(self.logger(), "Doing a consistency check...");

        self.wallet_specific_consistency_checks(txn)?;

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

    /// The EVM RPC endpoints to go online with, if the wallet supports bridged assets.
    ///
    /// Each endpoint is checked to be a working EVM JSON-RPC server on the chain it is
    /// configured for: one on another chain would report meaningless block numbers and events
    /// for the bridges living on the configured one. It is also checked to serve what confirming
    /// mint events needs (see [`EvmClient::check_mint_support`]).
    fn check_evm_rpc_urls(
        &self,
        online_options: &OnlineOptions,
    ) -> Result<HashMap<u64, String>, Error> {
        if !self.supports_schema(&AssetSchema::Bfa) {
            return Ok(HashMap::new());
        }
        for (chain_id, evm_rpc_url) in &online_options.evm_rpc_urls {
            let evm_client = EvmClient::new(evm_rpc_url)?;
            let actual = evm_client.chain_id().map_err(|e| Error::InvalidEvmRpcUrl {
                details: e.to_string(),
            })?;
            if actual != *chain_id {
                return Err(Error::EvmChainIdMismatch {
                    expected: *chain_id,
                    actual,
                });
            }
            evm_client.check_mint_support()?;
            debug!(
                self.logger(),
                "EVM RPC endpoint for chain ID {chain_id}: {evm_rpc_url}"
            );
        }
        Ok(online_options.evm_rpc_urls.clone())
    }

    fn get_online_data(
        &self,
        online_options: &OnlineOptions,
    ) -> Result<(Online, OnlineData), Error> {
        let id = now().unix_timestamp_nanos() as u64;
        let online = Online { id };
        let proxy_auth_keys = parse_proxy_auth_keys(&online_options.proxy_auth_keys)?;

        let (indexer, resolver) =
            get_indexer_and_resolver(&online_options.indexer_config, self.bitcoin_network())?;
        indexer.populate_tx_cache(self.bdk_wallet());

        let evm_rpc_urls = self.check_evm_rpc_urls(online_options)?;

        let online_data = OnlineData {
            id: online.id,
            indexer_config: online_options.indexer_config.clone(),
            indexer,
            resolver,
            evm_rpc_urls,
            hub_client: None,
            user_role: None,
            vanilla_sync_lookback: online_options.vanilla_sync_lookback,
            failed_witness_receive_grace_secs: online_options.failed_witness_receive_grace_secs,
            proxy_auth_keys,
        };

        Ok((online, online_data))
    }

    fn go_online_impl(&mut self, online_options: &OnlineOptions) -> Result<Online, Error> {
        let online = if let Some(online_data) = self.online_data().as_ref() {
            let online = Online { id: online_data.id };
            if online_data.indexer_config != online_options.indexer_config {
                let (online, online_data) = self.get_online_data(online_options)?;
                *self.online_data_mut() = Some(online_data);
                info!(self.logger(), "Went online with new indexer");
                online
            } else {
                self.check_online(online)?;
                // the indexer is unchanged but the other options may have changed. No `..` on
                // purpose: a new option doesn't compile until it's handled here
                let OnlineOptions {
                    indexer_config: _,         // a new indexer goes online from scratch, see above
                    skip_consistency_check: _, // only used during this call, not stored
                    vanilla_sync_lookback,
                    failed_witness_receive_grace_secs,
                    proxy_auth_keys,
                    evm_rpc_urls: _, // checked and returned by check_evm_rpc_urls below
                } = online_options;
                let proxy_auth_keys = parse_proxy_auth_keys(proxy_auth_keys)?;
                let evm_rpc_urls = self.check_evm_rpc_urls(online_options)?;
                let online_data = self.online_data_mut().as_mut().unwrap();
                online_data.vanilla_sync_lookback = *vanilla_sync_lookback;
                online_data.failed_witness_receive_grace_secs = *failed_witness_receive_grace_secs;
                online_data.proxy_auth_keys = proxy_auth_keys;
                online_data.evm_rpc_urls = evm_rpc_urls;
                online
            }
        } else {
            let (online, online_data) = self.get_online_data(online_options)?;
            *self.online_data_mut() = Some(online_data);
            online
        };

        if !online_options.skip_consistency_check {
            let txn = self.database().begin_transaction()?;
            self.check_consistency(&txn)?;
            self.persist_and_commit(txn)?;
        }

        Ok(online)
    }

    /// Whether one of the inputs of the TX of an outgoing transfer has been spent by another
    /// confirmed transaction, so that the TX can never confirm.
    fn tx_inputs_spent_elsewhere(&self, batch_transfer: &DbBatchTransfer) -> Result<bool, Error> {
        let Some(txid) = &batch_transfer.txid else {
            return Ok(false);
        };
        let psbt = self.get_signed_psbt(&self.get_transfer_dir(txid))?;
        let inputs: HashSet<BdkOutPoint> = psbt
            .unsigned_tx
            .input
            .iter()
            .map(|i| i.previous_output)
            .collect();
        Ok(self.bdk_wallet().transactions().any(|wallet_tx| {
            wallet_tx.chain_position.is_confirmed()
                && wallet_tx.tx_node.txid.to_string() != *txid
                && wallet_tx
                    .tx_node
                    .tx
                    .input
                    .iter()
                    .any(|i| inputs.contains(&i.previous_output))
        }))
    }

    fn get_signed_psbt(&self, transfer_dir: &Path) -> Result<Psbt, Error> {
        let psbt_file = transfer_dir.join(SIGNED_PSBT_FILE);
        let psbt_str = fs::read_to_string(psbt_file)?;
        Ok(Psbt::from_str(&psbt_str)?)
    }

    /// Verify a downloaded blob against its expected plaintext hash, first decrypting it with the
    /// key shared with the counterparty as dictated by `encryption`, the mode the payload is
    /// expected in: the recipient's invoice preference for consignments and media, always
    /// `Required` for the signed TX. For `Optional` the encrypted form is tried first, falling back
    /// to cleartext; for `Unsupported` the blob is taken as cleartext. Anything that doesn't match
    /// the hash afterwards (e.g. a blob encrypted despite `Unsupported`) is rejected.
    fn decrypt_blob_with_shared_key(
        &self,
        raw: Vec<u8>,
        encryption: Encryption,
        own_seed: &[u8],
        counterparty_pubkey_hex: &str,
        hash_hex: &str,
    ) -> Result<Vec<u8>, Error> {
        let (sym_key, hash_raw) =
            Self::shared_key_and_hash(own_seed, counterparty_pubkey_hex, hash_hex)?;

        // When `raw` is returned unmodified it might still be encrypted.
        // The plaintext hash check below catches these cases as well.
        let plaintext = match encryption {
            Encryption::Unsupported => raw,
            Encryption::Required => decrypt_blob(&raw, &sym_key, &hash_raw)?,
            Encryption::Optional => match decrypt_blob(&raw, &sym_key, &hash_raw) {
                Ok(p) => p,
                Err(_) => raw,
            },
        };
        if hex::encode(sha256_digest(&plaintext)) != hash_hex {
            return Err(Error::HashMismatch);
        }
        Ok(plaintext)
    }

    /// Symmetric key shared with the counterparty (from the own operation seed and the
    /// counterparty's hex-encoded Ed25519 pubkey) and the raw bytes of the expected plaintext hash,
    /// as the encryption primitives take them.
    fn shared_key_and_hash(
        own_seed: &[u8],
        counterparty_pubkey_hex: &str,
        hash_hex: &str,
    ) -> Result<([u8; 32], [u8; 32]), Error> {
        let seed: [u8; 32] = own_seed.try_into().map_err(|_| Error::Internal {
            details: s!("invalid own secret key"),
        })?;
        let own_sk = ed25519_from_seed(&seed);
        let counterparty_vk =
            verifying_key_from_bytes(&hex::decode(counterparty_pubkey_hex).map_err(|_| {
                Error::Internal {
                    details: s!("invalid counterparty pubkey"),
                }
            })?)?;
        let derived = derive_shared(&own_sk, &counterparty_vk)?;
        let hash_raw = decode_hash_hex(hash_hex)?;
        Ok((derived.sym_key, hash_raw))
    }

    /// File counterpart of [`decrypt_blob_with_shared_key`](Self::decrypt_blob_with_shared_key)
    /// for the (potentially big) consignment and media, taking the payload as the proxy delivers
    /// it: base64-encoded in its JSON-RPC response, so the ciphertext is already in memory and
    /// will stay that way until the proxy supports streaming it. The payload is decoded and
    /// decrypted on the fly, the plaintext is streamed into `dest` one chunk at a time and written
    /// atomically, then the file is verified against the expected plaintext hash before being
    /// made visible.
    fn decrypt_blob_with_shared_key_to_file(
        &self,
        payload_b64: &str,
        encryption: Encryption,
        own_seed: &[u8],
        counterparty_pubkey_hex: &str,
        hash_hex: &str,
        dest: &Path,
    ) -> Result<(), Error> {
        let (sym_key, hash_raw) =
            Self::shared_key_and_hash(own_seed, counterparty_pubkey_hex, hash_hex)?;
        let decoded =
            || base64::read::DecoderReader::new(payload_b64.as_bytes(), &general_purpose::STANDARD);
        atomic_write_with(dest, |tmp| {
            let decrypt_to = |tmp: &Path| -> Result<(), Error> {
                let mut file = fs::File::create(tmp)?;
                decrypt_stream(decoded(), &mut file, &sym_key, &hash_raw)
            };
            let copy_to = |tmp: &Path| -> Result<(), Error> {
                let mut file = fs::File::create(tmp)?;
                io::copy(&mut decoded(), &mut file).map_err(|e| match e.kind() {
                    // malformed base64 is a malformed payload, not an I/O failure
                    io::ErrorKind::InvalidData => Error::InvalidEncryptedBlob {
                        details: e.to_string(),
                    },
                    _ => e.into(),
                })?;
                Ok(())
            };
            // When the payload is written unmodified it might still be encrypted.
            // The plaintext hash check below catches these cases as well.
            match encryption {
                Encryption::Unsupported => copy_to(tmp)?,
                Encryption::Required => decrypt_to(tmp)?,
                Encryption::Optional => {
                    if decrypt_to(tmp).is_err() {
                        copy_to(tmp)?;
                    }
                }
            }
            if hash_file(tmp)? != hash_hex {
                return Err(Error::HashMismatch);
            }
            Ok(())
        })
    }

    /// Sign an ACK/NACK with the recipient's operation key and post it to the sender's proxy.
    fn sign_and_post_ack(
        &self,
        sender_proxy: &str,
        transfer_id: &str,
        ack: bool,
        consignment_hash: &str,
        recipient_seed: &[u8],
    ) -> Result<PostAckResponse, Error> {
        let seed: [u8; 32] = recipient_seed.try_into().map_err(|_| Error::Internal {
            details: s!("invalid recipient secret key"),
        })?;
        let sk = ed25519_from_seed(&seed);
        let digest = Self::ack_digest(transfer_id, ack, consignment_hash);
        let sig = hex::encode(sign_ed25519(&sk, &digest));
        let proxy_client = ProxyClient::new(sender_proxy)?;
        let res = proxy_client.post_ack(transfer_id, ack, &sig)?;
        if let Some(err) = res.error {
            return Err(map_proxy_error(err));
        }
        res.result.ok_or_else(|| Error::Proxy {
            details: s!("missing ack.post result"),
        })
    }

    fn get_received_signed_tx_path(&self, recipient_id: &str) -> PathBuf {
        self.get_receive_consignment_path(recipient_id)
            .with_file_name(RECEIVED_SIGNED_TX_FILE)
    }

    /// Decrypt the released signed TX, verify it, save it so it can be broadcast again until the
    /// broadcast deadline (v1.0-changes-spec.md 6.4) and broadcast it.
    fn broadcast_received_tx(
        &self,
        recipient_id: &str,
        signed_tx_b64: &str,
        signed_tx_hash: &str,
        sender_pubkey_hex: &str,
        recipient_seed: &[u8],
    ) -> Result<(), Error> {
        let enc = general_purpose::STANDARD
            .decode(signed_tx_b64)
            .map_err(InternalError::from)?;
        let tx_bytes = self.decrypt_blob_with_shared_key(
            enc,
            Encryption::Required,
            recipient_seed,
            sender_pubkey_hex,
            signed_tx_hash,
        )?;
        let tx: BdkTransaction = bdk_wallet::bitcoin::consensus::encode::deserialize(&tx_bytes)
            .map_err(|e| Error::Internal {
                details: e.to_string(),
            })?;
        atomic_write(&self.get_received_signed_tx_path(recipient_id), &tx_bytes)?;
        self.broadcast_tx(tx)?;
        Ok(())
    }

    /// Broadcast again the signed TX released to a recipient, if saved, e.g. after it was evicted
    /// from the mempool. Failures are only logged, as the TX may e.g. have been replaced.
    fn rebroadcast_received_tx(&self, recipient_id: &str) {
        let path = self.get_received_signed_tx_path(recipient_id);
        let Ok(tx_bytes) = fs::read(&path) else {
            return;
        };
        let tx: BdkTransaction =
            match bdk_wallet::bitcoin::consensus::encode::deserialize(&tx_bytes) {
                Ok(tx) => tx,
                Err(e) => {
                    warn!(self.logger(), "Cannot load saved signed TX: {e}");
                    return;
                }
            };
        debug!(self.logger(), "Broadcasting the saved signed TX again");
        if let Err(e) = self.broadcast_tx(tx) {
            warn!(
                self.logger(),
                "Failed to broadcast the saved signed TX again: {e}"
            );
        }
    }

    fn refuse_consignment(
        &self,
        txn: &DbTxn,
        mode: &ReceiveMode,
        recipient_id: &str,
        recipient_seed: &[u8],
        updated_batch_transfer: &mut DbBatchTransferActMod,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(
            self.logger(),
            "Refusing invalid consignment for {recipient_id}"
        );

        // only the proxy exchange has an operation to NACK; out-of-band just fails locally
        if let ReceiveMode::Proxy(data) = mode {
            match self.sign_and_post_ack(
                &data.sender_proxy,
                &data.transfer_id,
                false,
                &data.consignment_hash,
                recipient_seed,
            ) {
                Ok(r) => debug!(self.logger(), "Consignment NACK response: {:?}", r),
                Err(Error::ProxyCannotChange { .. }) => {
                    warn!(self.logger(), "Found an ACK when trying NACK");
                }
                // the sender proxy may have no operation (e.g. donation): just fail locally
                Err(Error::OperationNotFound) => {}
                Err(e) => {
                    error!(self.logger(), "Failed to post NACK: {e}");
                    return Err(e);
                }
            };
        }

        // a refused receive has no TXO registered, but a sync may have detected one paying its
        // script if the counterparty has already broadcast
        let scripts = script_buf_from_recipient_id(recipient_id.to_string())?
            .into_iter()
            .collect();
        self.unflag_pending_witness_txos(txn, &HashSet::new(), &scripts)?;
        self.delete_valid_consignment(recipient_id)?;
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::Failed);
        Ok(Some(txn.update_batch_transfer(updated_batch_transfer)?))
    }

    fn fetch_and_save_attachments(
        &self,
        attachments: Vec<Attachment>,
        mode: &ReceiveMode,
        recipient_seed: &[u8],
    ) -> Result<bool, Error> {
        let provided_map: HashMap<String, String> = match mode {
            ReceiveMode::OutOfBand { media_file_paths } => {
                let mut map = HashMap::new();
                for path in media_file_paths {
                    map.insert(hash_file(Path::new(path))?, path.clone());
                }
                map
            }
            ReceiveMode::Proxy { .. } => HashMap::new(),
        };

        // for the proxy exchange the media is stored under an id derived from the shared secret (not
        // the plaintext digest); derive the secret once so we can locate each attachment
        let proxy_derived = if let ReceiveMode::Proxy(data) = mode {
            let seed: [u8; 32] = recipient_seed.try_into().map_err(|_| Error::Internal {
                details: s!("invalid recipient secret key"),
            })?;
            let recipient_sk = ed25519_from_seed(&seed);
            let sender_vk =
                verifying_key_from_bytes(&hex::decode(&data.sender_pubkey).map_err(|_| {
                    Error::Internal {
                        details: s!("invalid sender pubkey"),
                    }
                })?)?;
            Some(derive_shared(&recipient_sk, &sender_vk)?)
        } else {
            None
        };

        let mut saved_media_paths = vec![];
        for attachment in attachments {
            let digest = hex::encode(attachment.digest);
            let media_path = self.media_dir().join(&digest);
            // resolve media only if the file is not already present
            if media_path.exists() {
                continue;
            }
            match mode {
                ReceiveMode::Proxy(data) => {
                    let proxy_client = ProxyClient::new(&data.proxy_url)?;
                    let media_id = proxy_derived
                        .as_ref()
                        .expect("proxy mode always derives the shared secret")
                        .media_id(&digest);
                    let media_res = proxy_client.get_media(&data.transfer_id, &media_id)?;
                    #[cfg(test)]
                    debug!(self.logger(), "Media GET response: {:?}", media_res);
                    match media_res.result {
                        Some(media_res) => {
                            // also checks the plaintext against the digest in the contract
                            self.decrypt_blob_with_shared_key_to_file(
                                &media_res,
                                data.recipient_encryption,
                                recipient_seed,
                                &data.sender_pubkey,
                                &digest,
                                &media_path,
                            )?;
                        }
                        None => {
                            error!(
                                self.logger(),
                                "Cannot find the media file but the contract defines one"
                            );
                            Self::cleanup_media_files(&saved_media_paths)?;
                            return Ok(false);
                        }
                    }
                }
                ReceiveMode::OutOfBand { .. } => match provided_map.get(&digest) {
                    Some(path) => atomic_write_with(&media_path, |tmp| {
                        fs::copy(path, tmp)?;
                        Ok(())
                    })?,
                    None => {
                        error!(
                            self.logger(),
                            "Media for digest {digest} was not provided for the out-of-band receive"
                        );
                        Self::cleanup_media_files(&saved_media_paths)?;
                        return Ok(false);
                    }
                },
            }
            saved_media_paths.push(media_path);
        }

        Ok(true)
    }

    fn cleanup_media_files(paths: &[PathBuf]) -> Result<(), Error> {
        for path in paths {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn get_consignment(
        &self,
        proxy_url: &str,
        transfer_id: String,
    ) -> Result<GetConsignmentResponse, Error> {
        let proxy_client = ProxyClient::new(proxy_url)?;
        let consignment_res = proxy_client.get_consignment(&transfer_id);
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
        match consignment
            .bundles
            .iter()
            .find(|bundle| bundle.witness_id() == witness_id)
        {
            Some(bundle) => self.assignments_for_bundle(bundle, vout, known_concealed),
            None => HashMap::new(),
        }
    }

    /// Locate the consignment terminal paying this recipient.
    ///
    /// Returns the terminal witness bundle and, for witness receives, the vout being paid.
    ///
    /// The bundle and vout returned here together identify the one payment destination the transfer
    /// waits on and credits from. A consignment declaring more than one such destination for this
    /// recipient - a different witness TXID (so a different bundle), or a different vout of the
    /// same TXID - is refused instead of resolved arbitrarily: `terminals` is counterparty-supplied
    /// data, it is read before validation, and its iteration order (by bundle ID) carries no
    /// meaning, so picking one of several genuinely different destinations would credit an
    /// unpredictable one of them. Several terminal seals defined on the same vout of the same
    /// bundle are a different matter (see the dedup in `push_match` below): that is one
    /// destination described more than once, not several to choose between.
    fn find_terminal_bundle<'c>(
        &self,
        consignment: &'c RgbTransfer,
        matcher: &ReceiveMatcher,
    ) -> Option<(&'c WitnessBundle, Option<u32>)> {
        let mut matches: Vec<(&'c WitnessBundle, Option<u32>)> = vec![];
        let mut push_match = |witness_bundle: &'c WitnessBundle, vout: Option<u32>| {
            // several revealed seals of one bundle can be defined on the same witness output - one
            // per assignment, e.g. two assignment types paying this recipient, differing only by
            // blinding - so keying on bundle + vout counts them as the single payment they are,
            // instead of tripping the ambiguity check below
            //
            // a no-op for concealed seals, which are matched on presence alone and push `None` once
            // per bundle
            if !matches.iter().any(|(wb, v)| {
                wb.bundle.bundle_id() == witness_bundle.bundle.bundle_id() && *v == vout
            }) {
                matches.push((witness_bundle, vout));
            }
        };

        for (bundle_id, seals) in consignment.terminals.iter() {
            // this lookup happens before validation, so a bundle a terminal references is not
            // yet guaranteed to be present and is simply skipped here: validation later refuses
            // the consignment over it
            let Some(witness_bundle) = consignment
                .bundles
                .iter()
                .find(|wb| wb.bundle.bundle_id() == *bundle_id)
            else {
                continue;
            };
            match matcher {
                ReceiveMatcher::Blind(secret_seal) => {
                    if seals
                        .into_iter()
                        .any(|seal| seal == BuilderSeal::Concealed(*secret_seal))
                    {
                        push_match(witness_bundle, None);
                    }
                }
                ReceiveMatcher::Witness(script_pubkey, blinding) => {
                    let tx = &witness_bundle.tx;
                    for seal in seals {
                        // a witness receive is matched by the scriptPubKey at the vout the seal
                        // names, which a concealed seal does not expose - it belongs to the blind
                        // arm above
                        let BuilderSeal::Revealed(seal) = seal else {
                            continue;
                        };
                        // only a seal closed by this bundle's own witness TX is a payment made by
                        // it: one naming an explicit TXID points at another transaction, so its
                        // vout would be looked up against the wrong `tx` below
                        if seal.txid != TxPtr::WitnessTx {
                            continue;
                        }
                        // with the proxy exchange the blinding is derived from the keys of both
                        // parties, so a seal with a different one doesn't pay this transfer
                        if blinding.is_some_and(|b| b != seal.blinding) {
                            continue;
                        }
                        let vout = seal.vout.into_u32();
                        if tx
                            .output
                            .get(vout as usize)
                            .is_some_and(|out| out.script_pubkey == *script_pubkey)
                        {
                            push_match(witness_bundle, Some(vout));
                        }
                    }
                }
            }
        }

        if matches.len() > 1 {
            error!(
                self.logger(),
                "Consignment declares {} terminal payments to this recipient, refusing it as \
                 ambiguous",
                matches.len()
            );
            return None;
        }
        matches.into_iter().next()
    }

    // extract the assignments in a single bundle that pay the recipient identified by
    // `known_concealed` (blind receives) or by the witness `vout` (witness receives)
    fn assignments_for_bundle(
        &self,
        bundle: &WitnessBundle,
        vout: Option<u32>,
        known_concealed: Option<SecretSeal>,
    ) -> HashMap<Opout, Assignment> {
        let mut received = HashMap::new();
        for KnownTransition { transition, opid } in bundle.bundle.known_transitions.iter() {
            for (ass_type, typed_assigns) in transition.assignments.iter() {
                for (no, fungible_assignment) in typed_assigns.as_fungible().iter().enumerate() {
                    let opout = Opout::new(*opid, *ass_type, no as u16);
                    if let BuilderSeal::Concealed(seal) = fungible_assignment.seal
                        && Some(seal) == known_concealed
                    {
                        match *ass_type {
                            OS_ASSET => {
                                received.insert(
                                    opout,
                                    Assignment::Fungible(fungible_assignment.state.as_u64()),
                                );
                            }
                            OS_INFLATION => {
                                received.insert(
                                    opout,
                                    Assignment::InflationRight(fungible_assignment.state.as_u64()),
                                );
                            }
                            _ => {}
                        }
                    };
                    if let BuilderSeal::Revealed(seal) = fungible_assignment.seal
                        && seal.txid == TxPtr::WitnessTx
                        && Some(seal.vout.into_u32()) == vout
                    {
                        match *ass_type {
                            OS_ASSET => {
                                received.insert(
                                    opout,
                                    Assignment::Fungible(fungible_assignment.state.as_u64()),
                                );
                            }
                            OS_INFLATION => {
                                received.insert(
                                    opout,
                                    Assignment::InflationRight(fungible_assignment.state.as_u64()),
                                );
                            }
                            _ => {}
                        }
                    };
                }
                for (no, structured_assignment) in typed_assigns.as_structured().iter().enumerate()
                {
                    let opout = Opout::new(*opid, *ass_type, no as u16);
                    if let BuilderSeal::Concealed(seal) = structured_assignment.seal
                        && Some(seal) == known_concealed
                    {
                        received.insert(opout, Assignment::NonFungible);
                    }
                    if let BuilderSeal::Revealed(seal) = structured_assignment.seal
                        && seal.txid == TxPtr::WitnessTx
                        && Some(seal.vout.into_u32()) == vout
                    {
                        received.insert(opout, Assignment::NonFungible);
                    };
                }
                for (no, rights_assignment) in typed_assigns.as_declarative().iter().enumerate() {
                    if *ass_type != OS_MINT {
                        continue;
                    }
                    let opout = Opout::new(*opid, *ass_type, no as u16);
                    if let BuilderSeal::Concealed(seal) = rights_assignment.seal
                        && Some(seal) == known_concealed
                    {
                        received.insert(opout, Assignment::MintRight);
                    }
                    if let BuilderSeal::Revealed(seal) = rights_assignment.seal
                        && seal.txid == TxPtr::WitnessTx
                        && Some(seal.vout.into_u32()) == vout
                    {
                        received.insert(opout, Assignment::MintRight);
                    };
                }
            }
        }

        received
    }

    fn get_reject_list(&self, source: &RejectListSource) -> Result<RejectList, Error> {
        match source {
            RejectListSource::Url(url) => self.get_url_reject_list(url),
            RejectListSource::Evm(contract) => self.get_evm_reject_list(contract),
        }
    }

    fn get_evm_reject_list(&self, contract: &EvmContract) -> Result<RejectList, Error> {
        let evm_client = self.checked_evm_client_for_chain(contract.chain_id)?;
        RejectList::from_evm(&evm_client, &contract.address)
    }

    fn get_url_reject_list(&self, reject_list_url: &str) -> Result<RejectList, Error> {
        let reject_list_client = RejectListClient::new(reject_list_url)?;
        // invalid lines are counted and reported once, as a list can be made of them
        let (reject_list, invalid) = RejectList::parse(&reject_list_client.get_reject_list()?);
        if invalid > 0 {
            warn!(
                self.logger(),
                "Ignoring {invalid} invalid opout(s) in reject list"
            );
        }
        Ok(reject_list)
    }

    fn extract_attachments(
        &self,
        stock: &RgbStock<'_>,
        valid_contract: &ValidContract,
        asset_schema: AssetSchema,
    ) -> Result<Vec<Attachment>, Error> {
        let mut attachments = vec![];
        let contract_data = stock.consignment_data(valid_contract)?;
        match asset_schema {
            AssetSchema::Nia => {
                let contract = NiaWrapper::with(contract_data);
                if let Some(attachment) = contract.contract_terms().media {
                    attachments.push(attachment)
                }
            }
            AssetSchema::Uda => {
                let contract = UdaWrapper::with(contract_data);
                if let Some(attachment) = contract.contract_terms().media {
                    attachments.push(attachment)
                }
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
                let contract = CfaWrapper::with(contract_data);
                if let Some(attachment) = contract.contract_terms().media {
                    attachments.push(attachment)
                }
            }
            AssetSchema::Ifa => {
                let contract = IfaWrapper::with(contract_data);
                if let Some(attachment) = contract.contract_terms().media {
                    attachments.push(attachment)
                }
            }
            AssetSchema::Bfa => {
                let contract = BfaWrapper::with(contract_data);
                if let Some(attachment) = contract.contract_terms().media {
                    attachments.push(attachment)
                }
            }
        };
        Ok(attachments)
    }

    fn safe_height(&self, min_confirmations: u8) -> Result<u32, Error> {
        Ok(self
            .indexer()
            .get_latest_block_height()?
            .saturating_sub(min_confirmations as u32)
            + 1)
    }

    /// Load the validated consignment of a received transfer.
    ///
    /// The validated copy is kept while the transfer is pending, to avoid validating again on
    /// every refresh. When missing (e.g. a transfer migrated from the legacy format, whose
    /// validated copy cannot be carried over), the consignment is validated again and the copy
    /// saved.
    fn load_received_valid_transfer(
        &self,
        batch_transfer: &DbBatchTransfer,
        recipient_id: &str,
    ) -> Result<ValidTransfer, Error> {
        let consignment_path = self.get_receive_consignment_path(recipient_id);
        let valid_consignment_path = self.get_receive_valid_consignment_path(&consignment_path);
        if valid_consignment_path.exists() {
            return ValidTransfer::load_file(&valid_consignment_path)
                .map_err(|e| InternalError::from(e).into());
        }

        debug!(
            self.logger(),
            "No validated consignment for {recipient_id}, validating consignment"
        );
        let consignment = RgbTransfer::load_file(&consignment_path).map_err(InternalError::from)?;
        let txid = batch_transfer
            .txid
            .as_deref()
            .expect("received batch_transfer with a consignment must have a txid");
        let witness_id = RgbTxid::from_str(txid).map_err(InternalError::from)?;
        let safe_height = NonZeroU32::new(self.safe_height(batch_transfer.min_confirmations)?);
        let validation_config = ValidationConfig {
            chain_net: self.chain_net(),
            // the reject list has already been checked when the consignment was received, so no
            // caller of this method needs the opouts DAG, which would only bloat the saved file
            build_opouts_dag: false,
            safe_height,
        };
        let valid_consignment = validate_consignment_internal(
            &consignment,
            Some(witness_id),
            &self.blockchain_resolver(),
            self.evm_rpc_urls(),
            &validation_config,
        )
        .map_err(|e| match e {
            Error::ConsignmentValidationFailed { details } => Error::Internal {
                details: format!("received consignment failed re-validation: {details}"),
            },
            e => e,
        })?;
        atomic_write_with(&valid_consignment_path, |tmp| {
            Ok(valid_consignment.save_file(tmp)?)
        })?;
        Ok(valid_consignment)
    }

    /// Delete the validated copy of a received consignment
    fn delete_valid_consignment(&self, recipient_id: &str) -> Result<(), Error> {
        let consignment_path = self.get_receive_consignment_path(recipient_id);
        match fs::remove_file(self.get_receive_valid_consignment_path(&consignment_path)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
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

    /// Whether the terminal bundle of a received consignment carries a mint transition,
    /// i.e. the consignment brings assets in from the external chain.
    ///
    /// A mint consignment is posted by the sender before its TX is signed (so the recipient
    /// can verify the mint before locking funds on the external chain), hence no operation can be
    /// registered on the sender proxy and no ACK is exchanged for it.
    fn is_mint_consignment(
        stock: &RgbStock<'_>,
        asset_schema: AssetSchema,
        contract_id: ContractId,
        terminal_bundle: &WitnessBundle,
    ) -> Result<bool, Error> {
        if !asset_schema.supports_mint() {
            return Ok(false);
        }
        let mint_type = stock
            .contract_schema(contract_id)?
            .transition_type(TypeOfTransition::Mint.type_name());
        Ok(terminal_bundle
            .bundle
            .known_transitions
            .iter()
            .any(|KnownTransition { transition, .. }| transition.transition_type == mint_type))
    }

    /// Return the signed witness TX embedded in the consignment, if present.
    fn donation_signed_tx(tx: &BdkTransaction) -> Option<BdkTransaction> {
        Some(tx)
            .filter(|tx| {
                !tx.input.is_empty()
                    && tx
                        .input
                        .iter()
                        .all(|i| !i.witness.is_empty() || !i.script_sig.is_empty())
            })
            .cloned()
    }

    // the `RecipientProxy` transport endpoint marked used, if any
    fn used_recipient_proxy_endpoint(
        tte_data: &[(DbTransferTransportEndpoint, DbTransportEndpoint)],
    ) -> Option<String> {
        tte_data
            .iter()
            .find(|(tte, _)| tte.kind == TransportEndpointKind::RecipientProxy && tte.used)
            .map(|(_, te)| te.endpoint.clone())
    }

    fn ack_consignment(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        transfer: &DbTransfer,
        updated_batch_transfer: &mut DbBatchTransferActMod,
        mode: &ReceiveMode,
        recipient_seed: &[u8],
        // signed TX embedded in the consignment (donation flow), broadcast directly if present
        donation_tx: Option<BdkTransaction>,
        // whether the consignment mints bridged assets (see `is_mint_consignment`)
        mint_consignment: bool,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger(), "ACKing consignment...");

        let recipient_id = transfer
            .recipient_id
            .clone()
            .expect("transfer should have a recipient ID");

        // the sender proxy retention grace is how long after the operation expiration a released
        // signed TX can still be retrieved: a non-donation, non-mint transfer whose sender proxy
        // grants less than the acceptable minimum is NACKed (v1.0-changes-spec.md 6.4)
        let mut retention_grace = None;
        if let ReceiveMode::Proxy(data) = mode
            && donation_tx.is_none()
            && !mint_consignment
        {
            let grace = sender_proxy_retention_grace(&data.sender_proxy)?;
            if grace < min_sender_retention_grace() {
                warn!(
                    self.logger(),
                    "Sender proxy retention grace ({grace}s) is below the acceptable minimum"
                );
                return self.refuse_consignment(
                    txn,
                    mode,
                    &recipient_id,
                    recipient_seed,
                    updated_batch_transfer,
                );
            }
            retention_grace = Some(grace);
        }

        match self.set_hub_accept_status(batch_transfer.idx)? {
            Some(true) => {}
            Some(false) => return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?)),
            None => return Ok(None),
        }

        // record our own ACK on the transfer
        let record_own_ack = |txn: &DbTxn| -> Result<(), Error> {
            let mut updated_transfer: DbTransferActMod = transfer.clone().into();
            updated_transfer.ack = ActiveValue::Set(Some(true));
            txn.update_transfer(&mut updated_transfer)?;
            Ok(())
        };

        match mode {
            ReceiveMode::Proxy(_) if mint_consignment => {
                // a mint consignment is posted before its TX is signed, so the sender cannot
                // have registered the operation on its proxy (ack.prepare needs the signed TX):
                // there is nothing to ACK, the sender broadcasts the TX on its own once signed
                debug!(
                    self.logger(),
                    "Mint consignment, no ACK expected: waiting for the broadcast"
                );
                record_own_ack(txn)?;
                updated_batch_transfer.status = ActiveValue::Set(TransferStatus::WaitingBroadcast);
            }
            ReceiveMode::Proxy(data) => {
                let res = match self.sign_and_post_ack(
                    &data.sender_proxy,
                    &data.transfer_id,
                    true,
                    &data.consignment_hash,
                    recipient_seed,
                ) {
                    Ok(r) => r,
                    // the sender proxy may have no operation (e.g. donation): broadcast locally
                    Err(Error::OperationNotFound) if donation_tx.is_some() => {
                        record_own_ack(txn)?;
                        self.broadcast_tx(donation_tx.expect("checked to be some"))?;
                        updated_batch_transfer.status =
                            ActiveValue::Set(TransferStatus::WaitingConfirmations);
                        return Ok(Some(txn.update_batch_transfer(updated_batch_transfer)?));
                    }
                    // a NACK is already recorded for this recipient (only this wallet could have
                    // posted it, as ACKs are signed with its operation key): the sender fails the
                    // operation on seeing it, so there is nothing left to wait for
                    Err(Error::ProxyCannotChange { .. }) => {
                        warn!(
                            self.logger(),
                            "A NACK was already posted for this recipient, failing the transfer"
                        );
                        return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?));
                    }
                    Err(e) => return Err(e),
                };
                debug!(self.logger(), "Consignment ACK response: {:?}", res);

                record_own_ack(txn)?;

                // the shared deadline for broadcasting the TX, after which (plus a confirmation
                // margin) the transfer is failed if the TX is not confirmed
                if let Some(grace) = retention_grace {
                    updated_batch_transfer.broadcast_deadline =
                        ActiveValue::Set(broadcast_deadline(res.expires_at.as_deref(), grace));
                }

                match (res.signed_tx, res.signed_tx_hash, res.sender_pk) {
                    (Some(signed_tx), Some(signed_tx_hash), Some(sender_pubkey)) => {
                        // the TX has been released: decrypt and broadcast it
                        self.broadcast_received_tx(
                            &recipient_id,
                            &signed_tx,
                            &signed_tx_hash,
                            &sender_pubkey,
                            recipient_seed,
                        )?;
                        updated_batch_transfer.status =
                            ActiveValue::Set(TransferStatus::WaitingConfirmations);
                    }
                    _ if donation_tx.is_some() => {
                        // donation: the signed TX travels inside the consignment, broadcast it
                        self.broadcast_tx(donation_tx.expect("checked to be some"))?;
                        updated_batch_transfer.status =
                            ActiveValue::Set(TransferStatus::WaitingConfirmations);
                    }
                    _ => {
                        // the TX has not been released (yet): for a batch operation not all
                        // recipients have ACKed yet, for a non-batch one the proxy is not
                        // compliant. Either way wait for the broadcast, posting the ACK again until
                        // the TX is returned or the operation expires (v1.0-changes-spec.md 6.4)
                        if !data.batch {
                            warn!(
                                self.logger(),
                                "No signed TX released for a non-batch operation"
                            );
                        }
                        updated_batch_transfer.status =
                            ActiveValue::Set(TransferStatus::WaitingBroadcast);
                    }
                }
            }
            ReceiveMode::OutOfBand { .. } => {
                // out-of-band: no proxy operation to ACK; the counterparty ACK is exchanged out of
                // band. Broadcast the donation TX if present, otherwise wait for the broadcast.
                record_own_ack(txn)?;
                updated_batch_transfer.status = if let Some(tx) = donation_tx {
                    debug!(
                        self.logger(),
                        "Consignment contains the signed TX, broadcasting it"
                    );
                    self.broadcast_tx(tx)?;
                    ActiveValue::Set(TransferStatus::WaitingConfirmations)
                } else {
                    debug!(
                        self.logger(),
                        "Consignment doesn't contain the signed TX, waiting for broadcast"
                    );
                    ActiveValue::Set(TransferStatus::WaitingBroadcast)
                };
            }
        }

        Ok(Some(txn.update_batch_transfer(updated_batch_transfer)?))
    }

    /// the canonical ACK/NACK content message digest used both when signing (recipient) and
    /// verifying (sender) an ACK (SHA-256 of `recipient_id || true|false || consignment_hash`)
    fn ack_digest(transfer_id: &str, ack: bool, consignment_hash: &str) -> [u8; 32] {
        let msg = format!(
            "{transfer_id}{}{consignment_hash}",
            if ack { "true" } else { "false" }
        );
        sha256_digest(msg.as_bytes())
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

        if transfer.uses_out_of_band_exchange() {
            debug!(self.logger(), "Skipping consignment exchange out-of-band");
            return Ok(None);
        }

        let tte_data = txn.get_transfer_transport_endpoints_data(transfer.idx)?;

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();

        // the recipient's per-operation Ed25519 secret seed (used to derive the shared secret)
        let recipient_seed = batch_transfer
            .operation_secret_key
            .clone()
            .unwrap_or_default();

        // if we already downloaded the consignment and its metadata in a previous attempt that
        // failed for a transient reason (e.g. network error), reuse them instead of hitting the
        // proxy again; the endpoint we used is recorded in the metadata, since a failed refresh
        // rolls back the `used` flag on the transfer transport endpoint
        let consignment_path = self.get_receive_consignment_path(&recipient_id);
        let consignment_meta_path = self.get_receive_consignment_meta_path(&recipient_id);
        let data = if consignment_path.exists()
            && consignment_meta_path.exists()
            && let Ok(meta_str) = fs::read_to_string(&consignment_meta_path)
            && let Ok(meta) = serde_json::from_str::<ReceivedConsignmentMeta>(&meta_str)
            && let Some(cached_transfer_id) = meta.transfer_id.clone()
            && let Some((cached_tte, cached_te)) = tte_data.iter().find(|(tte, te)| {
                tte.kind == TransportEndpointKind::RecipientProxy
                    && meta
                        .proxy_url
                        .as_ref()
                        .map_or(tte.used, |url| url == &te.endpoint)
            }) {
            debug!(
                self.logger(),
                "Reusing previously-downloaded consignment for {recipient_id}"
            );
            let data = ProxyReceiveData {
                proxy_url: cached_te.endpoint.clone(),
                transfer_id: cached_transfer_id,
                sender_proxy: meta.sender_transport_endpoint,
                sender_pubkey: meta.sender_pubkey,
                consignment_hash: meta.consignment_hash,
                recipient_encryption: transfer.recipient_encryption,
                batch: meta.batch,
            };
            // a failed refresh also rolls back what was saved along with the download
            if !cached_tte.used {
                let mut updated_transfer_transport_endpoint: DbTransferTransportEndpointActMod =
                    cached_tte.clone().into();
                updated_transfer_transport_endpoint.used = ActiveValue::Set(true);
                txn.update_transfer_transport_endpoint(&mut updated_transfer_transport_endpoint)?;
            }
            if transfer.consignment_hash.is_none() || transfer.proxy_transfer_id.is_none() {
                let mut ut: DbTransferActMod = transfer.clone().into();
                ut.consignment_hash = ActiveValue::Set(Some(data.consignment_hash.clone()));
                ut.proxy_transfer_id = ActiveValue::Set(Some(data.transfer_id.clone()));
                txn.update_transfer(&mut ut)?;
            }
            if !tte_data
                .iter()
                .any(|(tte, _)| tte.kind == TransportEndpointKind::SenderProxy)
            {
                self.save_sender_transport_endpoint(txn, transfer.idx, &data.sender_proxy)?;
            }
            data
        } else {
            // discover the transfer to the invoice and download its consignment
            let recipient_pk = hex::encode(transfer.recipient_pubkey.as_ref().ok_or_else(
                || Error::Internal {
                    details: s!("missing recipient pubkey"),
                },
            )?);
            let invoice_id = invoice_id(&recipient_id, &recipient_pk);
            let mut proxy_res = None;
            for (transfer_transport_endpoint, transport_endpoint) in tte_data {
                if transfer_transport_endpoint.kind != TransportEndpointKind::RecipientProxy {
                    continue;
                }
                let Some(found) = self.find_invoice_transfer(
                    &transport_endpoint.endpoint,
                    &invoice_id,
                    &recipient_seed,
                )?
                else {
                    info!(
                        self.logger(),
                        "Skipping transport endpoint: {:?}", &transport_endpoint
                    );
                    continue;
                };

                proxy_res = Some((transport_endpoint.endpoint, found));
                let mut updated_transfer_transport_endpoint: DbTransferTransportEndpointActMod =
                    transfer_transport_endpoint.into();
                updated_transfer_transport_endpoint.used = ActiveValue::Set(true);
                txn.update_transfer_transport_endpoint(&mut updated_transfer_transport_endpoint)?;
                break;
            }
            let Some((proxy_url, (transfer_id, result))) = proxy_res else {
                return Ok(None);
            };
            // the sender endpoint arrives in RgbTransport (rpc://) format; convert to an HTTP URL
            let sender_proxy =
                self.convert_transport_endpoint_url(&result.sender_transport_endpoint)?;
            let data = ProxyReceiveData {
                proxy_url,
                transfer_id,
                sender_proxy,
                sender_pubkey: result.sender_pk.clone(),
                consignment_hash: result.consignment_hash.clone(),
                recipient_encryption: transfer.recipient_encryption,
                batch: result.batch,
            };

            // decode and (if needed) decrypt the consignment; a malformed payload is refused like
            // one that fails decryption or verification
            match self.decrypt_blob_with_shared_key_to_file(
                &result.consignment,
                data.recipient_encryption,
                &recipient_seed,
                &data.sender_pubkey,
                &data.consignment_hash,
                &consignment_path,
            ) {
                Ok(()) => {}
                Err(e @ Error::IO { .. }) => return Err(e),
                Err(_) => {
                    error!(self.logger(), "Failed to decrypt/verify consignment");
                    return self.refuse_consignment(
                        txn,
                        &ReceiveMode::Proxy(data),
                        &recipient_id,
                        &recipient_seed,
                        &mut updated_batch_transfer,
                    );
                }
            }
            let meta = ReceivedConsignmentMeta {
                proxy_url: Some(data.proxy_url.clone()),
                transfer_id: Some(data.transfer_id.clone()),
                sender_transport_endpoint: data.sender_proxy.clone(),
                sender_pubkey: data.sender_pubkey.clone(),
                consignment_hash: data.consignment_hash.clone(),
                batch: data.batch,
            };
            let meta_str = serde_json::to_string(&meta).map_err(InternalError::from)?;
            atomic_write(&consignment_meta_path, meta_str.as_bytes())?;

            // persist the transfer ID, the consignment hash and the sender proxy endpoint (for the
            // ACK)
            let mut ut: DbTransferActMod = transfer.clone().into();
            ut.consignment_hash = ActiveValue::Set(Some(data.consignment_hash.clone()));
            ut.proxy_transfer_id = ActiveValue::Set(Some(data.transfer_id.clone()));
            txn.update_transfer(&mut ut)?;
            self.save_sender_transport_endpoint(txn, transfer.idx, &data.sender_proxy)?;

            data
        };

        let proxy_url = data.proxy_url.clone();
        let transfer_id = data.transfer_id.clone();
        let mode = ReceiveMode::Proxy(data);
        let res = self.process_received_consignment(
            txn,
            batch_transfer,
            &asset_transfer,
            &transfer,
            recipient_id,
            &consignment_path,
            mode,
            &mut updated_batch_transfer,
        )?;
        // once the consignment has been handled (ACKed or refused), its data is saved locally and
        // the proxy can delete it, while the transfer keeps counting towards the invoice quota
        if res.is_some() && self.releases_received_transfer_data() {
            self.mark_transfer_retrieved(&proxy_url, &transfer_id, &recipient_seed);
        }
        Ok(res)
    }

    /// Find the transfer to an invoice on a recipient proxy and download its consignment.
    ///
    /// The proxy is asked for the pending transfers of the invoice, the first one whose transfer
    /// ID matches the one derived from the invoice ID and the sender pubkey is returned. Invoices
    /// are single-use, so at most one transfer is expected. Proxy errors (e.g. an unreachable
    /// proxy or an invoice it doesn't know) make it skip the proxy.
    fn find_invoice_transfer(
        &self,
        proxy_url: &str,
        invoice_id: &str,
        recipient_seed: &[u8],
    ) -> Result<Option<(String, GetConsignmentResponse)>, Error> {
        let transfers = match list_invoice_transfers(proxy_url, invoice_id, recipient_seed) {
            Ok(res) => match (res.result, res.error) {
                (Some(transfers), None) => transfers,
                (_, err) => {
                    debug!(self.logger(), "Transfer list error: {:?}", err);
                    return Ok(None);
                }
            },
            Err(e) => {
                debug!(self.logger(), "Transfer list error: {e}");
                return Ok(None);
            }
        };
        let mut pending: Vec<String> = transfers
            .into_iter()
            .filter(|(_, state)| *state == ProxyTransferState::Pending)
            .map(|(tid, _)| tid)
            .collect();
        pending.sort();
        for transfer_id in pending {
            let result = match self.get_consignment(proxy_url, transfer_id.clone()) {
                Err(Error::NoConsignment) => continue,
                Err(e) => return Err(e),
                Ok(r) => r,
            };
            // a transfer whose ID doesn't match the sender pubkey was not posted by its sender
            if self::transfer_id(invoice_id, &result.sender_pk) != transfer_id {
                warn!(
                    self.logger(),
                    "Ignoring transfer {transfer_id}, which doesn't match its sender pubkey"
                );
                continue;
            }
            return Ok(Some((transfer_id, result)));
        }
        Ok(None)
    }

    /// Whether a blind receive still waiting for the counterparty may be failed, which releases its
    /// blinded UTXO (`v1.0-changes-spec.md` 6.1). Before the invoice expiration the receive can be
    /// cancelled. After it, the blinded UTXO stays reserved until the invoice expiration has passed
    /// on the proxies' clocks too and then, on each recipient proxy, either a `transfer.list` shows
    /// no pending transfer or the proxy retention grace has passed, since a transfer created before
    /// the expiration (e.g. a donation, whose TX is broadcast already) can be downloaded until then
    /// and would otherwise assign its assets to a spent UTXO. An "invoice not reserved" answer is
    /// taken as no pending transfer, as the proxy can delete the invoice data once nothing is
    /// pending, accepting that it can't be told apart from a data wipe.
    fn blind_receive_may_be_failed(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<bool, Error> {
        let Some(retention_deadline) = batch_transfer.retention_deadline else {
            return Ok(true);
        };
        let now = now().unix_timestamp();
        if !batch_transfer.is_expired(now) {
            return Ok(true);
        }
        let skew_margin = i64::from(clock_skew_margin());
        if !batch_transfer.is_expired_with_margin(now, clock_skew_margin()) {
            return Ok(false);
        }
        if retention_deadline + skew_margin < now {
            return Ok(true);
        }
        let (_, transfer) =
            batch_transfer.get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;
        let (Some(recipient_id), Some(recipient_pubkey), Some(recipient_seed)) = (
            &transfer.recipient_id,
            &transfer.recipient_pubkey,
            &batch_transfer.operation_secret_key,
        ) else {
            return Ok(true);
        };
        let invoice_id = invoice_id(recipient_id, &hex::encode(recipient_pubkey));
        for (tte, te) in txn.get_transfer_transport_endpoints_data(transfer.idx)? {
            if tte.kind != TransportEndpointKind::RecipientProxy {
                continue;
            }
            let res = list_invoice_transfers(&te.endpoint, &invoice_id, recipient_seed);
            let nothing_pending = match res {
                Ok(res) => match (res.result, res.error) {
                    (Some(transfers), None) => transfers
                        .values()
                        .all(|state| *state != ProxyTransferState::Pending),
                    (_, Some(err)) => matches!(map_proxy_error(err), Error::InvoiceNotReserved),
                    (None, None) => false,
                },
                Err(_) => false,
            };
            if !nothing_pending {
                debug!(
                    self.logger(),
                    "Recipient proxy {} may still hold a transfer to the invoice", te.endpoint
                );
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Let the recipient proxy delete the data of a transfer that has been handled. Failures are
    /// only logged, as the proxy deletes the data anyway once the invoice expires.
    fn mark_transfer_retrieved(&self, proxy_url: &str, transfer_id: &str, recipient_seed: &[u8]) {
        let Ok(seed) = <[u8; 32]>::try_from(recipient_seed) else {
            return;
        };
        let sk = ed25519_from_seed(&seed);
        let digest = sha256_digest(transfer_retrieved_message(transfer_id).as_bytes());
        let signature = hex::encode(sign_ed25519(&sk, &digest));
        let res = ProxyClient::new(proxy_url)
            .and_then(|client| client.transfer_retrieved(transfer_id, &signature));
        match res {
            Ok(r) if r.error.is_none() => {}
            Ok(r) => warn!(
                self.logger(),
                "Failed to mark transfer as retrieved: {:?}", r.error
            ),
            Err(e) => warn!(self.logger(), "Failed to mark transfer as retrieved: {e}"),
        }
    }

    /// The reject list a contract declares, read off it the way its `schema` requires.
    ///
    /// Only for the schemas [`AssetSchema::supports_reject_list`] accepts.
    fn reject_list_for_schema(
        schema: AssetSchema,
        contract_data: ContractData<FilteredContractState>,
    ) -> Option<RejectListSource> {
        match schema {
            AssetSchema::Bfa => BfaWrapper::with(contract_data)
                .reject_list()
                .map(RejectListSource::from),
            AssetSchema::Ifa => IfaWrapper::with(contract_data)
                .reject_list_url()
                .map(|u| RejectListSource::Url(u.to_string())),
            AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa => {
                unreachable!("{schema} does not support a reject list")
            }
        }
    }

    // validate a received consignment, if valid import any unknown asset (and its media), persist
    // the receive colorings, then update the transfer status (ACKing the consignment when possible)
    /// Phase 2 of the validation of a received consignment, followed by the checks on what it pays
    /// this recipient (see `assignments_for_bundle`) and on the reject list of its asset. Returns
    /// the validated consignment, its validation status and the assignments received, or `None`
    /// if the consignment must be refused.
    #[allow(clippy::too_many_arguments)]
    fn check_received_consignment<R: ResolveWitness>(
        &self,
        txn: &DbTxn,
        stock: &RgbStock<'_>,
        mut pending: PendingConsignment<true>,
        resolver: &R,
        asset_schema: AssetSchema,
        asset_id: &str,
        terminal_bundle: &WitnessBundle,
        vout: Option<u32>,
        known_concealed: Option<SecretSeal>,
    ) -> Result<Option<CheckedConsignment>, Error> {
        // phase 2: resolve the witnesses and finalize, which requires all the witnesses and
        // external anchors to be resolved by now
        let valid_consignment = match pending.pending().resolve_all(resolver) {
            Ok(()) => pending.finalize(),
            Err(ValidationError::InvalidConsignment(e)) => {
                error!(self.logger(), "Consignment is invalid: {}", e);
                return Ok(None);
            }
            Err(ValidationError::ResolverError(e)) => {
                warn!(self.logger(), "Network error during consignment validation");
                return Err(Error::Network {
                    details: e.to_string(),
                });
            }
            Err(e) => {
                return Err(Error::Internal {
                    details: format!("unexpected consignment validation error: {e}"),
                });
            }
        };
        let validation_status = valid_consignment.validation_status().clone();

        let validity = validation_status.validity();
        debug!(self.logger(), "Consignment validity: {:?}", validity);

        let receiving = self.assignments_for_bundle(terminal_bundle, vout, known_concealed);
        if receiving.is_empty() {
            error!(self.logger(), "Cannot find any receiving assignment");
            return Ok(None);
        };

        if asset_schema.supports_reject_list() {
            // the DB only records reject list URLs, while a BFA one can be an EVM contract
            let known_asset = txn.check_asset_exists(asset_id.to_string());
            let source = match (asset_schema, known_asset) {
                (AssetSchema::Ifa, Ok(ass)) => ass.reject_list_url.map(RejectListSource::Url),
                (AssetSchema::Ifa, Err(_))
                | (AssetSchema::Bfa | AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa, _) => {
                    Self::reject_list_for_schema(
                        asset_schema,
                        stock.consignment_data(&valid_consignment)?,
                    )
                }
            };
            if let Some(source) = &source {
                let reject_list = self.get_reject_list(source)?;

                let to_reject = rejected_opouts(
                    validation_status
                        .dag_data_opt
                        .as_ref()
                        .expect("build_opouts_dag is set for schemas supporting a reject list"),
                    &reject_list,
                    &receiving.clone().into_keys().collect(),
                )?;

                if !to_reject.is_empty() {
                    error!(
                        self.logger(),
                        "Found {} opout(s) that must be rejected",
                        to_reject.len()
                    );
                    return Ok(None);
                } else {
                    info!(
                        self.logger(),
                        "Didn't find any opout(s) that should be rejected"
                    );
                }
            }
        }

        Ok(Some((valid_consignment, validation_status, receiving)))
    }

    /// Whether a received mint pays the amount requested by the invoice, if it requests one: the
    /// lock on the external chain has to match the amount minted, so a mint of another amount
    /// than the agreed one is refused before the lock is made.
    fn mint_pays_requested(transfer: &DbTransfer, receiving: &HashMap<Opout, Assignment>) -> bool {
        match transfer.requested_assignment {
            Some(Assignment::Fungible(requested)) => {
                receiving.values().map(|a| a.main_amount()).sum::<u64>() == requested
            }
            _ => true,
        }
    }

    fn save_received_mint(
        &self,
        recipient_id: &str,
        received_mint: &ReceivedMint,
    ) -> Result<(), Error> {
        let serialized = serde_json::to_string(received_mint).map_err(InternalError::from)?;
        atomic_write(
            &self.get_received_mint_path(recipient_id),
            serialized.as_bytes(),
        )
    }

    fn get_mint_scanned_blocks_path(&self, recipient_id: &str) -> PathBuf {
        self.get_transfers_dir()
            .join(self.normalize_recipient_id(recipient_id))
            .join(MINT_SCAN_FILE)
    }

    /// The last finalized external chain block scanned without finding each mint event of the
    /// consignment received on `recipient_id`, by mint operation ID.
    fn read_mint_scanned_blocks(&self, recipient_id: &str) -> Result<HashMap<OpId, u64>, Error> {
        let path = self.get_mint_scanned_blocks_path(recipient_id);
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let scanned_blocks: HashMap<String, u64> =
            serde_json::from_str(&fs::read_to_string(path)?).map_err(InternalError::from)?;
        scanned_blocks
            .into_iter()
            .map(|(opid, block)| {
                let opid = OpId::from_str(&opid).map_err(|e| Error::Internal {
                    details: format!("invalid mint operation ID: {e}"),
                })?;
                Ok((opid, block))
            })
            .collect()
    }

    fn save_mint_scanned_blocks(
        &self,
        recipient_id: &str,
        scanned_blocks: HashMap<OpId, u64>,
    ) -> Result<(), Error> {
        let scanned_blocks: HashMap<String, u64> = scanned_blocks
            .into_iter()
            .map(|(opid, block)| (opid.to_string(), block))
            .collect();
        let serialized = serde_json::to_string(&scanned_blocks).map_err(InternalError::from)?;
        atomic_write(
            &self.get_mint_scanned_blocks_path(recipient_id),
            serialized.as_bytes(),
        )
    }

    fn process_received_consignment(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        asset_transfer: &DbAssetTransfer,
        transfer: &DbTransfer,
        recipient_id: String,
        consignment_path: &Path,
        mode: ReceiveMode,
        updated_batch_transfer: &mut DbBatchTransferActMod,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        let mut stock = self.rgb_stock(txn)?;
        // the recipient's per-operation Ed25519 secret seed (for ACK signing / decryption)
        let recipient_seed = batch_transfer
            .operation_secret_key
            .clone()
            .unwrap_or_default();
        let consignment = match RgbTransfer::load_file(consignment_path) {
            Ok(c) => c,
            Err(e) => {
                error!(self.logger(), "Failed to load consignment file: {e}");
                return self.refuse_consignment(
                    txn,
                    &mode,
                    &recipient_id,
                    &recipient_seed,
                    updated_batch_transfer,
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
                &mode,
                &recipient_id,
                &recipient_seed,
                updated_batch_transfer,
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
                    &mode,
                    &recipient_id,
                    &recipient_seed,
                    updated_batch_transfer,
                );
            }
        }

        // a transfer consignment must declare the terminals it pays
        if consignment.terminals.is_empty() {
            error!(self.logger(), "Consignment declares no terminals");
            return self.refuse_consignment(
                txn,
                &mode,
                &recipient_id,
                &recipient_seed,
                updated_batch_transfer,
            );
        }

        // locate the terminal paying us
        let blinding = match &mode {
            ReceiveMode::Proxy(data) => {
                let seed: [u8; 32] =
                    recipient_seed
                        .clone()
                        .try_into()
                        .map_err(|_| Error::Internal {
                            details: s!("invalid recipient secret key"),
                        })?;
                let sender_vk = hex::decode(&data.sender_pubkey)
                    .ok()
                    .and_then(|pk| verifying_key_from_bytes(&pk).ok());
                let Some(sender_vk) = sender_vk else {
                    error!(self.logger(), "Invalid sender pubkey");
                    return self.refuse_consignment(
                        txn,
                        &mode,
                        &recipient_id,
                        &recipient_seed,
                        updated_batch_transfer,
                    );
                };
                Some(derive_shared(&ed25519_from_seed(&seed), &sender_vk)?.seal_blinding)
            }
            ReceiveMode::OutOfBand { .. } => None,
        };
        let matcher = transfer.receive_matcher(blinding)?;
        let Some((terminal_bundle, vout)) = self.find_terminal_bundle(&consignment, &matcher)
        else {
            error!(
                self.logger(),
                "Cannot find a consignment terminal paying this recipient"
            );
            return self.refuse_consignment(
                txn,
                &mode,
                &recipient_id,
                &recipient_seed,
                updated_batch_transfer,
            );
        };
        let witness_id = terminal_bundle.witness_id();
        let txid = witness_id.to_string();
        let known_concealed = match matcher {
            ReceiveMatcher::Blind(secret_seal) => Some(secret_seal),
            ReceiveMatcher::Witness(..) => None,
        };

        // validate consignment
        debug!(self.logger(), "Validating consignment...");
        let safe_height = NonZeroU32::new(self.safe_height(batch_transfer.min_confirmations)?);
        let validation_config = ValidationConfig {
            chain_net: self.chain_net(),
            build_opouts_dag: asset_schema.supports_reject_list(),
            safe_height,
        };
        let resolver = OffchainResolver {
            witness_id: Some(witness_id),
            consignment: &consignment,
            fallback: &self.blockchain_resolver(),
        };
        let rules = asset_schema.schema_rules();
        // phase 1: everything that only depends on the consignment itself
        let mut pending = match consignment
            .clone()
            .validate_deterministic(&rules, &validation_config)
        {
            Ok(pending) => pending,
            Err(ValidationError::InvalidConsignment(e)) => {
                error!(self.logger(), "Consignment is invalid: {}", e);
                return self.refuse_consignment(
                    txn,
                    &mode,
                    &recipient_id,
                    &recipient_seed,
                    updated_batch_transfer,
                );
            }
            Err(e) => {
                return Err(Error::Internal {
                    details: format!("unexpected consignment validation error: {e}"),
                });
            }
        };

        // external anchors (mint events) live on the chain the asset's bridge contract is
        // deployed on and cannot be resolved by the witness resolver: confirm them against that
        // chain before resolving the witnesses
        let anchors: Vec<ExternalAnchor> =
            pending.pending().unresolved_anchors().cloned().collect();
        let mint_consignment = !anchors.is_empty();
        let mut received_mint = None;
        if mint_consignment {
            // only bridged (BFA) assets are expected to carry external anchors
            if !asset_schema.supports_mint() {
                error!(
                    self.logger(),
                    "Consignment has pending external anchors but its schema does not support minting"
                );
                return self.refuse_consignment(
                    txn,
                    &mode,
                    &recipient_id,
                    &recipient_seed,
                    updated_batch_transfer,
                );
            }

            // the bridge location is an immutable genesis global naming the chain the mint
            // events live on: the RPC endpoint configured for that chain is checked to actually
            // be on it before being asked about the anchors. A missing or misconfigured endpoint
            // is a local issue, not one with the consignment: keep it and retry at the next
            // refresh, until the transfer expires, without blocking the other transfers
            let bridge_location = BridgedFungibleAsset::bridge_location(
                pending.consignment().genesis(),
            )
            .map_err(|e| Error::Internal {
                details: format!("unexpected bridge location error: {e}"),
            })?;
            let BridgeLocation::Evm(EvmContract { chain_id, address }) = &bridge_location;
            let chain_id = *chain_id;
            // the mint paying this recipient, as opposed to those in its history
            let terminal_opids: HashSet<OpId> = terminal_bundle
                .bundle
                .known_transitions
                .iter()
                .map(|kt| kt.opid)
                .collect();
            received_mint = anchors.iter().find_map(|anchor| {
                let ExternalAnchor::MintEvent {
                    opid,
                    amount,
                    after_block,
                } = anchor;
                terminal_opids.contains(opid).then(|| ReceivedMint {
                    opid: opid.to_string(),
                    amount: *amount,
                    after_block: *after_block,
                    evm_chain_id: chain_id,
                    bridge_contract: address.to_string(),
                })
            });
            let Some(evm_rpc_url) = self.evm_rpc_url(chain_id) else {
                warn!(
                    self.logger(),
                    "Cannot resolve mint events: no EVM RPC URL configured for chain ID \
                     {chain_id}, will retry"
                );
                return Ok(None);
            };
            let evm_client = EvmClient::new(evm_rpc_url)?;
            // the mints are only scanned for within the same number of blocks for everyone, as
            // their after_block is chosen by whoever created them; the ones in the history of the
            // terminal one must also already be confirmed
            let mint_resolver = MintAnchorResolver::new(
                &evm_client,
                &terminal_opids,
                self.read_mint_scanned_blocks(&recipient_id)?,
                max_mint_scan_blocks(),
            );
            let anchor_resolver = match CheckedAnchorResolver::with(&mint_resolver, bridge_location)
            {
                Ok(anchor_resolver) => anchor_resolver,
                Err(AnchorResolverError::WrongChainId { expected, actual }) => {
                    warn!(
                        self.logger(),
                        "Cannot resolve mint events: the EVM RPC endpoint configured for chain \
                         ID {expected} is on chain ID {actual}, will retry"
                    );
                    return Ok(None);
                }
                Err(e) => return Err(e.into()),
            };
            // past its deadline (the invoice expiration), a mint is only waited for, and accepted,
            // if its lock was included by then (see mint_in_finality_grace)
            if let (Some(received_mint), Some(deadline)) =
                (&received_mint, batch_transfer.expiration)
            {
                let now = now().unix_timestamp();
                if now > deadline && !self.mint_in_finality_grace(received_mint, deadline, now) {
                    info!(
                        self.logger(),
                        "Mint {} expired with no lock included by its deadline, failing it",
                        received_mint.opid
                    );
                    return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?));
                }
            }
            // the mint follows the invoice, so the block it commits to (the latest finalized one
            // when it began) cannot be much older: refuse an older one (checked once, before the
            // mint is saved), which would only make the scan for the lock cover more blocks
            if let Some(received_mint) = &received_mint
                && self.read_received_mint(&recipient_id)?.is_none()
            {
                let after_block_timestamp =
                    evm_client.block_timestamp(received_mint.after_block)? as i64;
                if after_block_timestamp < batch_transfer.created_at - mint_after_block_grace() {
                    error!(
                        self.logger(),
                        "Mint {} commits to block {} which precedes the invoice",
                        received_mint.opid,
                        received_mint.after_block
                    );
                    return self.refuse_consignment(
                        txn,
                        &mode,
                        &recipient_id,
                        &recipient_seed,
                        updated_batch_transfer,
                    );
                }
            }
            debug!(
                self.logger(),
                "Resolving external anchors {:?} at bridge location {:?}",
                anchors,
                anchor_resolver.location()
            );

            // confirm each pending anchor that has a matching mint event in a finalized block of
            // the external chain
            let mut unconfirmed = 0;
            for anchor in &anchors {
                match pending.resolve_anchor(&anchor_resolver, anchor) {
                    Ok(()) => {}
                    Err(AnchorResolverError::Unconfirmed(_)) => {
                        let ExternalAnchor::MintEvent { opid, .. } = anchor;
                        if mint_resolver.is_exhausted(opid) {
                            error!(
                                self.logger(),
                                "Mint {opid} has no event within {} blocks from the one it \
                                 commits to",
                                max_mint_scan_blocks()
                            );
                            return self.refuse_consignment(
                                txn,
                                &mode,
                                &recipient_id,
                                &recipient_seed,
                                updated_batch_transfer,
                            );
                        }
                        unconfirmed += 1
                    }
                    Err(e) => return Err(e.into()),
                }
            }

            // a missing mint event doesn't make the consignment invalid, the event may simply
            // not have happened yet or not be finalized yet: keep the (cached)
            // consignment and try again at the next refresh, until the transfer expires
            if unconfirmed > 0 {
                self.save_mint_scanned_blocks(&recipient_id, mint_resolver.scanned_blocks())?;
                // a mint is posted before its lock is made, so that its recipient can check it
                // first: check it now (once), assuming the lock is there, so the user knows whether
                // to make it, and what to lock (see get_received_mint)
                if let Some(received_mint) = &received_mint
                    && self.read_received_mint(&recipient_id)?.is_none()
                {
                    let mut assumed = pending.clone();
                    let unresolved: Vec<ExternalAnchor> =
                        assumed.pending().unresolved_anchors().cloned().collect();
                    for anchor in &unresolved {
                        assumed
                            .pending()
                            .resolve_anchor(anchor)
                            .map_err(|e| Error::Internal {
                                details: format!("unexpected anchor resolution error: {e}"),
                            })?;
                    }
                    let checked = self.check_received_consignment(
                        txn,
                        &stock,
                        assumed,
                        &resolver,
                        asset_schema,
                        &asset_id,
                        terminal_bundle,
                        vout,
                        known_concealed,
                    )?;
                    let Some((_, _, receiving)) = checked else {
                        return self.refuse_consignment(
                            txn,
                            &mode,
                            &recipient_id,
                            &recipient_seed,
                            updated_batch_transfer,
                        );
                    };
                    if !Self::mint_pays_requested(transfer, &receiving) {
                        return self.refuse_consignment(
                            txn,
                            &mode,
                            &recipient_id,
                            &recipient_seed,
                            updated_batch_transfer,
                        );
                    }
                    self.save_received_mint(&recipient_id, received_mint)?;
                    info!(
                        self.logger(),
                        "Mint checked, waiting for its lock: {received_mint:?}"
                    );
                }
                info!(
                    self.logger(),
                    "{unconfirmed} mint event(s) not found or not confirmed on the external \
                     chain yet, will retry"
                );
                return Ok(None);
            }
        }

        // phase 2 and the checks on what the consignment pays
        let checked = self.check_received_consignment(
            txn,
            &stock,
            pending,
            &resolver,
            asset_schema,
            &asset_id,
            terminal_bundle,
            vout,
            known_concealed,
        )?;
        let Some((valid_consignment, validation_status, receiving)) = checked else {
            return self.refuse_consignment(
                txn,
                &mode,
                &recipient_id,
                &recipient_seed,
                updated_batch_transfer,
            );
        };
        if let Some(received_mint) = &received_mint {
            if !Self::mint_pays_requested(transfer, &receiving) {
                return self.refuse_consignment(
                    txn,
                    &mode,
                    &recipient_id,
                    &recipient_seed,
                    updated_batch_transfer,
                );
            }
            self.save_received_mint(&recipient_id, received_mint)?;
        }

        if asset_transfer.asset_id.is_none() {
            if txn.check_asset_exists(asset_id.clone()).is_err() {
                // unknown asset
                debug!(self.logger(), "Receiving unknown contract...");
                let valid_contract = valid_consignment.clone().into_valid_contract();

                let attachments =
                    self.extract_attachments(&stock, &valid_contract, asset_schema)?;
                if !self.fetch_and_save_attachments(attachments, &mode, &recipient_seed)? {
                    return self.refuse_consignment(
                        txn,
                        &mode,
                        &recipient_id,
                        &recipient_seed,
                        updated_batch_transfer,
                    );
                }

                stock
                    .import_contract(valid_contract.clone(), &self.blockchain_resolver())
                    .expect("failure importing received contract");
                debug!(self.logger(), "Contract registered");
                self.save_new_asset_internal(
                    txn,
                    &stock,
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

        // keep the validated consignment while the transfer is pending, to avoid validating again
        let valid_consignment_path = self.get_receive_valid_consignment_path(consignment_path);
        atomic_write_with(&valid_consignment_path, |tmp| {
            Ok(valid_consignment.save_file(tmp)?)
        })?;

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
                return Ok(Some(txn.update_batch_transfer(updated_batch_transfer)?));
            }
        }

        let donation_tx = Self::donation_signed_tx(&terminal_bundle.tx);

        self.ack_consignment(
            txn,
            batch_transfer,
            transfer,
            updated_batch_transfer,
            &mode,
            &recipient_seed,
            donation_tx,
            mint_consignment,
        )
    }

    fn provide_out_of_band_consignment_impl(
        &self,
        txn: &DbTxn,
        consignment_path_in: &str,
        media_file_paths: Vec<String>,
    ) -> Result<RefreshResult, Error> {
        let consignment =
            RgbTransfer::load_file(consignment_path_in).map_err(|_| Error::InvalidFilePath {
                file_path: consignment_path_in.to_string(),
            })?;

        let db_data = txn.get_db_data(false)?;

        let asset_id = consignment.contract_id().to_string();

        if consignment.terminals.is_empty() {
            return Err(Error::CannotProvideOutOfBandConsignment {
                details: s!("the provided consignment declares no terminals"),
            });
        }

        let mut matches = vec![];
        for batch_transfer in &db_data.batch_transfers {
            // skip non-incoming transfers and transfers that are not waiting for a counterparty
            if !batch_transfer.incoming
                || batch_transfer.status != TransferStatus::WaitingCounterparty
            {
                continue;
            }
            let (asset_transfer, transfer) = batch_transfer
                .get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;
            // skip transfers bound to a different asset
            if asset_transfer
                .asset_id
                .as_ref()
                .is_some_and(|aid| aid != &asset_id)
            {
                continue;
            }
            // skip transfers that are not set up for out-of-band exchange
            if !transfer.uses_out_of_band_exchange() {
                continue;
            }

            // check if a consignment terminal pays this transfer
            let matcher = transfer.receive_matcher(None)?;
            if self.find_terminal_bundle(&consignment, &matcher).is_some() {
                matches.push((
                    batch_transfer.clone(),
                    asset_transfer.clone(),
                    transfer.clone(),
                ));
            }
        }
        if matches.is_empty() {
            return Err(Error::CannotProvideOutOfBandConsignment {
                details: s!("no pending receive transfer matches the provided consignment"),
            });
        }

        // a single consignment (for one asset) can pay more than one of this wallet's pending
        // invoices (e.g. a sender batched a send to two of them), so process every matched receive
        let mut results: RefreshResult = HashMap::new();
        for (batch_transfer, asset_transfer, transfer) in matches {
            let recipient_id = transfer
                .recipient_id
                .clone()
                .expect("matched transfer should have a recipient ID");
            debug!(
                self.logger(),
                "Providing out-of-band consignment for recipient ID: {recipient_id}"
            );

            // copy the provided consignment to the canonical receive path, so later refresh stages
            // (safe height, confirmations) find it where they expect it
            let consignment_path = self.get_receive_consignment_path(&recipient_id);
            atomic_write_with(&consignment_path, |tmp| {
                fs::copy(consignment_path_in, tmp)?;
                Ok(())
            })?;

            let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
            let mode = ReceiveMode::OutOfBand {
                media_file_paths: media_file_paths.clone(),
            };
            let updated = self.process_received_consignment(
                txn,
                &batch_transfer,
                &asset_transfer,
                &transfer,
                recipient_id,
                &consignment_path,
                mode,
                &mut updated_batch_transfer,
            )?;

            results.insert(
                batch_transfer.idx,
                RefreshedTransfer {
                    updated_status: updated.map(|t| t.status),
                    failure: None,
                },
            );
        }

        Ok(results)
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
        let txid = batch_transfer
            .txid
            .as_deref()
            .expect("batch_transfer in WaitingSafeHeight must have a txid");

        // a transfer from the previous proxy protocol cannot be ACKed anymore, so waiting for a
        // safe height is pointless. Failing it right away, without waiting for the expiration, is
        // safe: in that protocol the recipient only ACKed after this wait, so no ACK was ever
        // posted and a non-donation sender never broadcasts without it; a donation was broadcast
        // at send time, which the chain check below (mempool included) catches
        if batch_transfer.is_legacy() {
            let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
            if self.indexer().get_tx_confirmations(txid)?.is_some() {
                info!(
                    self.logger(),
                    "TX of a transfer from the previous proxy protocol found on-chain"
                );
                updated_batch_transfer.status =
                    ActiveValue::Set(TransferStatus::WaitingConfirmations);
                return Ok(Some(
                    txn.update_batch_transfer(&mut updated_batch_transfer)?,
                ));
            }
            info!(
                self.logger(),
                "Transfer from the previous proxy protocol cannot be completed, failing it"
            );
            return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?));
        }

        let valid_consignment = self.load_received_valid_transfer(batch_transfer, &recipient_id)?;
        let validation_status = valid_consignment.validation_status();
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

        let matcher = transfer.receive_matcher(None)?;
        let terminal_bundle = self
            .find_terminal_bundle(&valid_consignment, &matcher)
            .map(|(terminal_bundle, _)| terminal_bundle);
        let signed_tx = terminal_bundle
            .and_then(|terminal_bundle| Self::donation_signed_tx(&terminal_bundle.tx));
        let mint_consignment = match terminal_bundle {
            Some(terminal_bundle) => Self::is_mint_consignment(
                &self.rgb_stock(txn)?,
                valid_consignment.schema_id().try_into()?,
                valid_consignment.contract_id(),
                terminal_bundle,
            )?,
            None => false,
        };

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        let tte_data = txn.get_transfer_transport_endpoints_data(transfer.idx)?;
        let mode = if tte_data.is_empty() {
            ReceiveMode::OutOfBand {
                media_file_paths: vec![],
            }
        } else {
            let proxy_url =
                Self::used_recipient_proxy_endpoint(&tte_data).expect("there should be 1 used TTE");
            let consignment_meta_path = self.get_receive_consignment_meta_path(&recipient_id);
            let meta: ReceivedConsignmentMeta =
                serde_json::from_str(&fs::read_to_string(&consignment_meta_path)?)
                    .map_err(InternalError::from)?;
            let transfer_id = transfer
                .proxy_transfer_id
                .clone()
                .or(meta.transfer_id)
                .ok_or_else(|| Error::Internal {
                    details: s!("missing transfer ID"),
                })?;
            ReceiveMode::Proxy(ProxyReceiveData {
                proxy_url,
                transfer_id,
                sender_proxy: meta.sender_transport_endpoint,
                sender_pubkey: meta.sender_pubkey,
                consignment_hash: meta.consignment_hash,
                recipient_encryption: transfer.recipient_encryption,
                batch: meta.batch,
            })
        };
        let recipient_seed = batch_transfer
            .operation_secret_key
            .clone()
            .unwrap_or_default();
        self.ack_consignment(
            txn,
            batch_transfer,
            &transfer,
            &mut updated_batch_transfer,
            &mode,
            &recipient_seed,
            signed_tx,
            mint_consignment,
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

        // collect the (shared) sender proxy endpoint and the IDs of the transfers registered on
        // it; out-of-band recipients have no sender proxy endpoint, as they are not registered
        // there (naming them in ack.get would make the proxy reject the whole request)
        let mut transfer_ids: Vec<String> = vec![];
        let mut sender_proxy: Option<String> = None;
        for asset_transfer_data in &batch_transfer_data.asset_transfers_data {
            for transfer in &asset_transfer_data.transfers {
                let transfer_sender_proxy = txn
                    .get_transfer_transport_endpoints_data(transfer.idx)?
                    .into_iter()
                    .find(|(tte, _)| tte.kind == TransportEndpointKind::SenderProxy)
                    .map(|(_, ce)| ce.endpoint);
                let Some(transfer_sender_proxy) = transfer_sender_proxy else {
                    continue;
                };
                // a SenderProxy endpoint is only ever saved for a proxy recipient transfer, which
                // always has a transfer ID: a missing one is corrupted data, and skipping it would
                // just leave the batch waiting until its expiration, so surface it instead
                let tid = transfer
                    .proxy_transfer_id
                    .clone()
                    .ok_or_else(|| Error::Internal {
                        details: format!(
                            "transfer {} has a sender proxy endpoint but no transfer ID",
                            transfer.idx
                        ),
                    })?;
                transfer_ids.push(tid);
                if sender_proxy.is_none() {
                    sender_proxy = Some(transfer_sender_proxy);
                }
            }
        }
        let Some(sender_proxy) = sender_proxy else {
            // no sender proxy registered (e.g. donation): nothing to wait for here
            return Ok(None);
        };

        // ack.get is authenticated: sign SHA-256(sorted recipient IDs ‖ timestamp) with the
        // operation (sender) key; the proxy verifies it against the registered sender_pk and
        // rejects timestamps outside its tolerance window
        let seed: [u8; 32] = batch_transfer
            .operation_secret_key
            .clone()
            .unwrap_or_default()
            .try_into()
            .map_err(|_| Error::Internal {
                details: s!("invalid operation secret key"),
            })?;
        let sk = ed25519_from_seed(&seed);
        let proxy_client = ProxyClient::new(&sender_proxy)?;
        let ack_res = get_ack_signed(&proxy_client, &transfer_ids, &sk)?;
        debug!(self.logger(), "ACK get response: {:?}", ack_res);
        if let Some(err) = ack_res.error {
            // the sender proxy no longer knows the operation (e.g. it dropped its data): the ACKs
            // cannot be retrieved anymore and whether the TX was released cannot be known, so
            // settle via the chain if a recipient broadcast it, otherwise keep waiting until the
            // broadcast deadline has passed on the proxy's clock too, past which the proxy never
            // hands out a TX even if it recovers the operation and recipients stop broadcasting
            // it (v1.0-changes-spec.md 6.2)
            check_get_ack_error(err)?;
            return self
                .settle_untrusted_batch(
                    txn,
                    batch_transfer,
                    "the sender proxy no longer knows the operation",
                    Error::OperationNotFound,
                )
                .map(Some);
        }
        let ack_data = ack_res.result.ok_or_else(|| Error::Proxy {
            details: s!("missing ack.get result"),
        })?;

        for asset_transfer_data in batch_transfer_data.asset_transfers_data.iter_mut() {
            for transfer in asset_transfer_data.transfers.iter_mut() {
                if transfer.ack.is_some() {
                    continue;
                }
                let tte_data = txn.get_transfer_transport_endpoints_data(transfer.idx)?;
                // a transfer with no transport endpoints at all has nothing to poll; recipients that
                // never responded on the proxy are filtered out below by the missing ACK/signature
                if tte_data.is_empty() {
                    continue;
                }
                // transport endpoints are only saved for a proxy recipient transfer, which always
                // has a transfer ID: a missing one is corrupted data, and skipping it would just
                // leave the batch waiting until its expiration, so surface it instead
                let transfer_id =
                    transfer
                        .proxy_transfer_id
                        .clone()
                        .ok_or_else(|| Error::Internal {
                            details: format!(
                                "transfer {} has transport endpoints but no transfer ID",
                                transfer.idx
                            ),
                        })?;
                debug!(self.logger(), "Transfer ID: {transfer_id}");
                // a transfer absent from the response has not been responded to yet (its ACK/NACK
                // data was already fetched in bulk above)
                let ack = ack_data.acks.get(&transfer_id).copied();
                let sig = ack_data.signatures.get(&transfer_id).cloned();
                let (Some(ack), Some(sig_hex)) = (ack, sig) else {
                    continue;
                };

                // verify the recipient's ACK signature
                let pubkey = transfer
                    .recipient_pubkey
                    .as_ref()
                    .ok_or_else(|| Error::Internal {
                        details: s!("missing recipient pubkey"),
                    })?;
                let vk = verifying_key_from_bytes(pubkey)?;
                let consignment_hash =
                    transfer
                        .consignment_hash
                        .clone()
                        .ok_or_else(|| Error::Internal {
                            details: s!("missing consignment hash"),
                        })?;
                let digest = Self::ack_digest(&transfer_id, ack, &consignment_hash);
                let sig_bytes =
                    hex::decode(&sig_hex).map_err(|_| Error::InvalidContentSignature)?;
                if !verify_ed25519(&vk, &digest, &sig_bytes) {
                    // the proxy served an ACK the recipient never signed, so it is corrupt or
                    // forging ACKs and nothing it says can be trusted. The batch cannot be failed
                    // right away either, as the proxy may have released the TX and ack.clean would
                    // be refused (due to the stored ACK), so, as v1.0-changes-spec.md 6.3
                    // prescribes, it is settled like one the proxy lost (6.2), surfacing the error
                    // while waiting
                    return self
                        .settle_untrusted_batch(
                            txn,
                            batch_transfer,
                            "the sender proxy serves an invalid ACK signature",
                            Error::InvalidContentSignature,
                        )
                        .map(Some);
                }

                let mut updated_transfer: DbTransferActMod = transfer.clone().into();
                updated_transfer.ack = ActiveValue::Set(Some(ack));
                updated_transfer.ack_signature = ActiveValue::Set(Some(sig_bytes.clone()));
                txn.update_transfer(&mut updated_transfer)?;
                transfer.ack = Some(ack);
                transfer.ack_signature = Some(sig_bytes);
            }
        }

        let mut batch_transfer_transfers: Vec<DbTransfer> = vec![];
        batch_transfer_data
            .asset_transfers_data
            .iter()
            .for_each(|atd| batch_transfer_transfers.extend(atd.transfers.clone()));
        self.try_complete_batch(txn, batch_transfer, &batch_transfer_transfers)
    }

    fn try_complete_batch(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        transfers: &[DbTransfer],
    ) -> Result<Option<DbBatchTransfer>, Error> {
        if transfers.iter().any(|t| t.ack == Some(false)) {
            return Ok(Some(
                self.fail_batch_transfer(txn, batch_transfer)
                    .map_err(|e| match e {
                        Error::MultisigTransferStatusMismatch => Error::MultisigUnexpectedData {
                            details: s!("hub reports accepted=true but recipient sent a NACK"),
                        },
                        other => other,
                    })?,
            ));
        }
        if !transfers.iter().all(|t| t.ack == Some(true)) {
            return Ok(None);
        }

        // every recipient has ACKed: broadcast the transaction if transfer hasn't expired

        // don't broadcast a transfer that has expired: past its expiration the recipient is allowed
        // to fail it, so broadcasting now could complete a transfer the recipient has already given
        // up on. When every ACK came (signed) from the sender proxy, the proxy released the TX to
        // the recipients, so the transfer cannot simply be failed anymore: the TX is broadcast
        // while no invoice has expired, while past that a recipient that didn't obtain the TX
        // (v1.0-changes-spec.md 6.3) may have failed the transfer, so the transfer only progresses
        // if the TX is found on-chain and can otherwise only be failed after spending one of its
        // inputs (v1.0-changes-spec.md 6.1)
        let released = transfers.iter().all(|t| t.ack_signature.is_some());
        let now = now().unix_timestamp();
        if !released && batch_transfer.is_expired(now) {
            debug!(
                self.logger(),
                "Transfer expired before broadcast, failing it instead of broadcasting"
            );
            return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?));
        }
        if released && Self::any_invoice_expired(transfers, now) {
            if let Some(updated) = self.complete_batch_if_tx_on_chain(txn, batch_transfer)? {
                return Ok(Some(updated));
            }
            // the TX can never confirm once one of its inputs has been spent by another confirmed
            // transaction, which is the only way such an operation can fail
            if self.tx_inputs_spent_elsewhere(batch_transfer)? {
                info!(
                    self.logger(),
                    "An input of the TX has been spent by another confirmed transaction, failing the transfer"
                );
                return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?));
            }
            warn!(
                self.logger(),
                "Every recipient ACKed but an invoice has expired: not broadcasting the TX"
            );
            return Ok(None);
        }
        match self.set_hub_accept_status(batch_transfer.idx)? {
            Some(true) => {}
            Some(false) => return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?)),
            None => return Ok(None),
        }
        Ok(Some(self.broadcast_batch(txn, batch_transfer)?))
    }

    /// Whether the invoice expiration of any recipient has passed, accounting for the clock skew
    /// with the recipient.
    fn any_invoice_expired(transfers: &[DbTransfer], now: i64) -> bool {
        transfers
            .iter()
            .filter_map(|t| t.invoice_expiration)
            .any(|exp| now + PROXY_CLOCK_SKEW_MARGIN_SECS as i64 >= exp)
    }

    /// Broadcast the TX of an outgoing batch transfer (a no-op on the network if a recipient
    /// already did), update the RGB state and move the transfer to `WaitingConfirmations`.
    fn broadcast_batch(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<DbBatchTransfer, Error> {
        let txid = batch_transfer
            .txid
            .as_ref()
            .expect("batch transfer should have a TXID");
        let transfer_dir = self.get_transfers_dir().join(txid);
        let signed_psbt = self.get_signed_psbt(&transfer_dir)?;
        let mut stock = self.rgb_stock(txn)?;
        let fascia = read_fascia(&transfer_dir.join(FASCIA_FILE))?;
        self.broadcast_and_update_rgb(txn, &mut stock, &signed_psbt, fascia)?;
        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        updated_batch_transfer.status = ActiveValue::Set(TransferStatus::WaitingConfirmations);

        txn.update_batch_transfer(&mut updated_batch_transfer)
    }

    /// Settle an outgoing batch transfer whose sender proxy can no longer be relied upon to tell
    /// whether the signed TX was released (it lost the operation, or serves an ACK with an invalid
    /// signature), as `v1.0-changes-spec.md` 6.2 and 6.3 prescribe: complete it if its TX is
    /// on-chain, otherwise fail it once its broadcast deadline (expiration plus the sender proxy
    /// retention grace) has passed on the proxy's clock too, past which the proxy never hands out
    /// a TX and recipients that obtained it stop broadcasting it, accepting the residual risk of a
    /// TX broadcast later anyway.
    ///
    /// While the transfer has to keep waiting, `waiting_error` is returned, so that the refresh
    /// reports why the transfer cannot progress and a fail attempt is denied.
    ///
    /// `reason` describes why the proxy cannot be relied upon, for the logs.
    fn settle_untrusted_batch(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        reason: &str,
        waiting_error: Error,
    ) -> Result<DbBatchTransfer, Error> {
        if let Some(updated) = self.complete_batch_if_tx_on_chain(txn, batch_transfer)? {
            return Ok(updated);
        }
        if !broadcast_deadline_passed_beyond_skew_margin(batch_transfer) {
            return Err(waiting_error);
        }
        warn!(
            self.logger(),
            "Failing a transfer past its broadcast deadline as {reason}: its TX can still be broadcast if it was released"
        );
        self.fail_batch_transfer(txn, batch_transfer)
    }

    /// Complete an outgoing batch transfer waiting for its counterparty if its TX is on-chain
    /// (mempool included), which happens when a recipient obtained the TX from the sender proxy
    /// and broadcast it without the sender learning about the ACKs.
    ///
    /// Finding the TX guarantees the transfer cannot be failed. Not finding it gives no
    /// guarantee: a recipient may still hold the TX and broadcast it later.
    fn complete_batch_if_tx_on_chain(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        let Some(txid) = &batch_transfer.txid else {
            return Ok(None);
        };
        if self.indexer().get_tx_confirmations(txid)?.is_none() {
            return Ok(None);
        }
        info!(
            self.logger(),
            "TX of a transfer waiting for the counterparty found on-chain"
        );
        Ok(Some(self.broadcast_batch(txn, batch_transfer)?))
    }

    fn provide_out_of_band_ack_impl(
        &mut self,
        txn: &DbTxn,
        recipient_id: String,
    ) -> Result<Option<OperationResult>, Error> {
        let db_data = txn.get_db_data(false)?;

        // recipient IDs are unique per transfer, so this identifies a single recipient transfer
        let transfer = db_data
            .transfers
            .iter()
            .find(|t| t.recipient_id.as_deref() == Some(recipient_id.as_str()))
            .cloned()
            .ok_or(Error::CannotProvideOutOfBandAck {
                details: s!("no transfer found for the provided recipient ID"),
            })?;
        let asset_transfer = db_data
            .asset_transfers
            .iter()
            .find(|at| at.idx == transfer.asset_transfer_idx)
            .cloned()
            .expect("transfer should have an asset transfer");
        let batch_transfer = db_data
            .batch_transfers
            .iter()
            .find(|bt| bt.idx == asset_transfer.batch_transfer_idx)
            .cloned()
            .expect("asset transfer should have a batch transfer");

        // check if the transfer can receive an out-of-band ACK
        if batch_transfer.incoming {
            return Err(Error::CannotProvideOutOfBandAck {
                details: s!("only outgoing transfers can be ACKed"),
            });
        }
        if batch_transfer.status != TransferStatus::WaitingCounterparty {
            return Err(Error::CannotProvideOutOfBandAck {
                details: s!("only WaitingCounterparty transfers can be ACKed"),
            });
        }
        // a RECIPIENT-facing proxy endpoint means the consignment/ACK are exchanged automatically
        // (via refresh); the sender's own SenderProxy endpoint (v1.0, for ACK retrieval) doesn't
        // make the recipient automated, so it must not block an out-of-band ACK
        let tte_data = txn.get_transfer_transport_endpoints_data(transfer.idx)?;
        if tte_data
            .iter()
            .any(|(tte, _)| tte.kind == TransportEndpointKind::RecipientProxy)
        {
            return Err(Error::CannotProvideOutOfBandAck {
                details: s!(
                    "recipient uses an automated transport; use refresh() to complete it instead"
                ),
            });
        }

        // record the out-of-band ACK (if not there already)
        if transfer.ack != Some(true) {
            let mut updated_transfer: DbTransferActMod = transfer.clone().into();
            updated_transfer.ack = ActiveValue::Set(Some(true));
            txn.update_transfer(&mut updated_transfer)?;
        }

        // gather the batch's transfers, reflecting the ACK just recorded, and try to complete the
        // batch (broadcasting only once every recipient, out-of-band or automated transport, has
        // ACKed)
        let batch_transfer_data =
            batch_transfer.get_transfers(&db_data.asset_transfers, &db_data.transfers)?;
        let mut transfers: Vec<DbTransfer> = vec![];
        batch_transfer_data
            .asset_transfers_data
            .iter()
            .for_each(|atd| transfers.extend(atd.transfers.clone()));
        // reflect the ACK in-memory, since `db_data` was loaded before the ACK was written to the
        // DB and `get_transfers` thus returns stale copies
        for t in transfers.iter_mut() {
            if t.idx == transfer.idx {
                t.ack = Some(true);
            }
        }

        let Some(updated) = self.try_complete_batch(txn, &batch_transfer, &transfers)? else {
            // other recipients still need to ACK: leave the batch WaitingCounterparty
            return Ok(None);
        };
        if updated.status != TransferStatus::WaitingConfirmations {
            // the batch was failed instead of broadcast (e.g. it expired)
            return Err(Error::CannotProvideOutOfBandAck {
                details: s!("transfer could not be completed"),
            });
        }

        let txid = updated
            .txid
            .clone()
            .expect("broadcast batch transfer should have a TXID");

        // recover the entropy used at send time for the operation result
        let transfer_dir = self.get_transfers_dir().join(&txid);
        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        let serialized_info = fs::read_to_string(info_file)?;
        let info_contents: InfoBatchTransfer =
            serde_json::from_str(&serialized_info).map_err(InternalError::from)?;

        Ok(Some(OperationResult {
            txid,
            batch_transfer_idx: updated.idx,
            entropy: info_contents.entropy,
        }))
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

        // an incoming transfer with a broadcast deadline (released via the sender proxy) whose TX
        // is not confirmed is failed once past the deadline plus the confirmation margin, while
        // until the deadline its TX is broadcast again if missing (v1.0-changes-spec.md 6.4)
        if incoming
            && batch_transfer.broadcast_deadline.is_some()
            && confirmations.unwrap_or(0) == 0
        {
            if confirmation_deadline_passed(batch_transfer) {
                info!(
                    self.logger(),
                    "TX not confirmed past the broadcast deadline, failing the transfer"
                );
                return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?));
            }
            if confirmations.is_none()
                && batch_transfer
                    .broadcast_deadline
                    .is_some_and(|deadline| now().unix_timestamp() <= deadline)
            {
                let (_, transfer) = batch_transfer
                    .get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;
                if let Some(recipient_id) = &transfer.recipient_id {
                    self.rebroadcast_received_tx(recipient_id);
                }
            }
        }

        let Some(confirmations) = confirmations else {
            return Ok(None);
        };

        if confirmations < batch_transfer.min_confirmations as u64 {
            if batch_transfer.status == TransferStatus::WaitingBroadcast {
                let mut updated_batch_transfer: DbBatchTransferActMod =
                    batch_transfer.clone().into();
                updated_batch_transfer.status =
                    ActiveValue::Set(TransferStatus::WaitingConfirmations);
                return Ok(Some(
                    txn.update_batch_transfer(&mut updated_batch_transfer)?,
                ));
            }
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
            let valid_consignment =
                self.load_received_valid_transfer(batch_transfer, &recipient_id)?;
            let mut stock = self.rgb_stock(txn)?;
            stock.accept_transfer(valid_consignment.clone(), &self.blockchain_resolver())?;
            self.delete_valid_consignment(&recipient_id)?;
            let asset_schema: AssetSchema = valid_consignment.schema_id().try_into()?;
            match asset_schema {
                AssetSchema::Ifa => {
                    let contract_id = valid_consignment.contract_id();
                    let contract_wrapper =
                        stock.contract_wrapper::<InflatableFungibleAsset>(contract_id)?;
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
                AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa | AssetSchema::Bfa => {}
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
        // a transfer from the previous proxy protocol can neither be received nor ACKed anymore,
        // and failing it right away, without waiting for the expiration, is safe: an incoming one
        // was never ACKed, so its sender never broadcasts; an outgoing one is only ever broadcast
        // by this wallet, which won't happen anymore
        if transfer.is_legacy() {
            info!(
                self.logger(),
                "Transfer from the previous proxy protocol cannot be completed, failing it"
            );
            return Ok(Some(self.fail_batch_transfer(txn, transfer)?));
        }
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
        let incoming = transfer.incoming;
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
            // a proxy recipient that ACKed polls the sender proxy for the TX (or expiration); an
            // out-of-band recipient instead waits for the sender to broadcast and watches the chain
            // (as WaitingConfirmations does), since it has no sender proxy to poll
            TransferStatus::WaitingBroadcast => {
                let (_, incoming_transfer) =
                    transfer.get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;
                if incoming_transfer.uses_out_of_band_exchange() {
                    // out-of-band: the sender broadcasts once every recipient ACKs; watch the chain
                    self.wait_confirmations(txn, transfer, db_data, incoming, skip_sync)
                } else {
                    // proxy recipient: if the TX is already on-chain (broadcast by the sender when the
                    // batch completed, or by an earlier poll) settle via the chain; otherwise poll the
                    // sender proxy for the TX (or expiration)
                    match self.wait_confirmations(txn, transfer, db_data, incoming, skip_sync)? {
                        Some(updated) => Ok(Some(updated)),
                        None => self.wait_broadcast(txn, transfer, db_data),
                    }
                }
            }
            TransferStatus::WaitingConfirmations => {
                self.wait_confirmations(txn, transfer, db_data, incoming, skip_sync)
            }
            _ => Ok(None),
        }
    }

    /// A proxy recipient that has ACKed polls the sender proxy until the signed TX is
    /// released, then decrypts and broadcasts it.
    fn wait_broadcast(
        &mut self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        debug!(self.logger(), "Waiting broadcast...");

        let (_, transfer) =
            batch_transfer.get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;

        // a legacy transfer was ACKed on the recipient proxy, so the sender may still broadcast the
        // TX (which can be detected on-chain) until the expiration, after which the transfer is
        // failed
        if batch_transfer.is_legacy() {
            let now = now().unix_timestamp();
            if batch_transfer.is_expired(now) {
                info!(
                    self.logger(),
                    "Expired transfer from the previous proxy protocol, failing it"
                );
                return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?));
            }
            debug!(self.logger(), "Waiting for the sender to broadcast");
            return Ok(None);
        }

        // sender proxy endpoint (where the signed TX is released); without one there is nothing
        // to ask for the TX, the sender broadcasts it and wait_confirmations detects it on-chain
        let mut sender_proxy = None;
        for (tte, ce) in txn.get_transfer_transport_endpoints_data(transfer.idx)? {
            if tte.kind == TransportEndpointKind::SenderProxy {
                sender_proxy = Some(ce.endpoint);
                break;
            }
        }
        let Some(sender_proxy) = sender_proxy else {
            debug!(
                self.logger(),
                "No sender proxy, waiting for the sender to broadcast"
            );
            return Ok(None);
        };

        let consignment_hash =
            transfer
                .consignment_hash
                .clone()
                .ok_or_else(|| Error::Internal {
                    details: s!("missing consignment hash"),
                })?;
        let recipient_seed = batch_transfer
            .operation_secret_key
            .clone()
            .unwrap_or_default();

        let transfer_id = transfer
            .proxy_transfer_id
            .clone()
            .ok_or_else(|| Error::Internal {
                details: s!("missing transfer ID"),
            })?;
        // the TX is not on-chain (wait_confirmations is tried before this on every refresh):
        // past the broadcast deadline plus the confirmation margin the transfer is failed, unless
        // the proxy still releases the TX (v1.0-changes-spec.md 6.4)
        let fail_if_past_deadline = |wallet: &mut Self| -> Result<Option<DbBatchTransfer>, Error> {
            if confirmation_deadline_passed(batch_transfer) {
                info!(
                    wallet.logger(),
                    "TX not confirmed past the broadcast deadline, failing the transfer"
                );
                return Ok(Some(wallet.fail_batch_transfer(txn, batch_transfer)?));
            }
            Ok(None)
        };
        let res = match self.sign_and_post_ack(
            &sender_proxy,
            &transfer_id,
            true,
            &consignment_hash,
            &recipient_seed,
        ) {
            Ok(r) => r,
            // the operation expired without releasing the TX, which never will be: fail the
            // transfer
            Err(Error::OperationExpired) => {
                return Ok(Some(self.fail_batch_transfer(txn, batch_transfer)?));
            }
            // the sender proxy no longer knows the operation: a TX released before can still be
            // broadcast (and detected on-chain) until the broadcast deadline. An operation never
            // registered there (e.g. mint) has no broadcast deadline: the TX is broadcast by the
            // sender and wait_confirmations (tried before this on every refresh) detects it
            Err(Error::OperationNotFound) => return fail_if_past_deadline(self),
            // no definite answer: fail the transfer once past the broadcast deadline
            Err(e) => {
                if let Some(updated) = fail_if_past_deadline(self)? {
                    return Ok(Some(updated));
                }
                return Err(e);
            }
        };

        let mut updated_batch_transfer: DbBatchTransferActMod = batch_transfer.clone().into();
        // a transfer ACKed via a proxy not returning the operation expiration yet has no broadcast
        // deadline, set it as soon as it's known
        let mut deadline_changed = false;
        if batch_transfer.broadcast_deadline.is_none()
            && let Some(deadline) = broadcast_deadline(
                res.expires_at.as_deref(),
                sender_proxy_retention_grace(&sender_proxy)?,
            )
        {
            updated_batch_transfer.broadcast_deadline = ActiveValue::Set(Some(deadline));
            deadline_changed = true;
        }

        match (res.signed_tx, res.signed_tx_hash, res.sender_pk) {
            (Some(signed_tx), Some(signed_tx_hash), Some(sender_pubkey)) => {
                let recipient_id = transfer
                    .recipient_id
                    .clone()
                    .expect("transfer should have a recipient ID");
                self.broadcast_received_tx(
                    &recipient_id,
                    &signed_tx,
                    &signed_tx_hash,
                    &sender_pubkey,
                    &recipient_seed,
                )?;
                updated_batch_transfer.status =
                    ActiveValue::Set(TransferStatus::WaitingConfirmations);
                Ok(Some(
                    txn.update_batch_transfer(&mut updated_batch_transfer)?,
                ))
            }
            // still waiting on the other recipients (or a non-compliant proxy)
            _ => {
                if deadline_changed {
                    // the status is unchanged, the deadline is checked on the next refresh
                    txn.update_batch_transfer(&mut updated_batch_transfer)?;
                    return Ok(None);
                }
                fail_if_past_deadline(self)
            }
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
            // a failed refresh must not leave its partial changes behind (e.g. the colorings of a
            // consignment validated before its ACK failed), as the next one would redo them
            match txn.with_savepoint(|txn| {
                self.refresh_transfer(txn, transfer, &db_data, &filter, skip_sync)
            }) {
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
        let mut input_btc_amt: u64 = 0;
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
            if assignments_collected.mint < assignments_needed.mint
                && asset_allocations
                    .iter()
                    .any(|a| matches!(a.assignment, Assignment::MintRight))
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

            let utxo_btc_amt = unspent
                .utxo
                .btc_amount
                .parse::<u64>()
                .expect("DB should contain a valid BTC amount");
            input_btc_amt = input_btc_amt
                .checked_add(utxo_btc_amt)
                .expect("total input BTC value cannot exceed u64::MAX");

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

        let mut psbt = builder.finish().map_err(|e| match e {
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
        self.add_non_witness_utxos(&mut psbt);

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
                        free_utxos.sort_by_key(|u| {
                            u.utxo
                                .btc_amount
                                .parse::<u64>()
                                .expect("DB should contain a valid BTC amount")
                        });
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

    #[allow(clippy::too_many_arguments)]
    fn prepare_rgb_psbt(
        &self,
        txn: &DbTxn,
        psbt: &mut Psbt,
        transfer_info_map: &mut BTreeMap<String, InfoAssetTransfer>,
        transfer_dir: PathBuf,
        donation: bool,
        unspents: Vec<LocalUnspent>,
        stock: &mut RgbStock<'_>,
        min_confirmations: u8,
        expiration_timestamp: Option<i64>,
        btc_change: Option<BtcChange>,
        sender_transport_endpoint: Option<String>,
        operation_secret_key: Option<Vec<u8>>,
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
        let mut opid = None;
        for (asset_id, transfer_info) in transfer_info_map.iter_mut() {
            let asset_utxos = transfer_info.asset_spend.input_outpoints.iter().cloned();
            let mut all_opout_state_vec = Vec::new();
            for (explicit_seal, opout_state_map) in stock.contract_assignments_for(
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
            let mut asset_transition_builder = stock.transition_builder(
                transfer_info.asset_info.contract_id,
                transfer_info.main_transition.clone().type_name(),
            )?;
            // on a retry the assignments needed are increased by the rejected ones, so that the
            // selection looks past them, but mint rights can't be partially spent: adding more
            // than the operation needs would be useless and, for a mint (whose schema allows a
            // single mint right input), invalid. Any further one selected is moved as
            // extra state instead
            let inputs_needed = AssignmentsCollection {
                mint: transfer_info.original_assignments_needed.mint,
                ..transfer_info.assignments_needed.clone()
            };
            for (outpoint, opout, state) in all_opout_state_vec {
                let mut should_add_as_input = !rejected.contains(&opout);
                if should_add_as_input {
                    should_add_as_input =
                        inputs_added.opout_contributes(&opout, &state, &inputs_needed);
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
                    Assignment::MintRight => {
                        seal = self.get_beneficiary_seal(&recipient.local_recipient_data);
                        asset_transition_builder =
                            asset_transition_builder.add_rights(RGB_STATE_MINT_RIGHT, seal)?;
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
                if change.mint > 0 {
                    for _ in 0..change.mint {
                        asset_transition_builder =
                            asset_transition_builder.add_rights(RGB_STATE_MINT_RIGHT, seal)?;
                    }
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
                        .add_global_state(RGB_GLOBAL_BURNED_ASSET, Amount::from(burn))
                        .unwrap();
                    if let Some(burn_reason) = transfer_info.burn_reason {
                        asset_transition_builder = asset_transition_builder
                            .add_global_state(RGB_GLOBAL_BURN_REASON, burn_reason)?;
                    }
                }
                TypeOfTransition::Mint => {
                    let amount_to_mint = transfer_info
                        .recipients
                        .iter()
                        .map(|r| r.assignment.main_amount())
                        .sum::<u64>();
                    let after_block = transfer_info.after_block.ok_or_else(|| Error::Internal {
                        details: s!("mint transfer is missing the external chain block"),
                    })?;
                    asset_transition_builder = asset_transition_builder
                        .add_global_state(RGB_GLOBAL_ISSUED_SUPPLY, Amount::from(amount_to_mint))
                        .unwrap()
                        .add_metadata(RGB_METADATA_AFTER_BLOCK, BlockNumber::from(after_block))?;
                }
                _ => {}
            }

            let transition = asset_transition_builder.complete_transition()?;
            if transfer_info.main_transition == TypeOfTransition::Mint {
                opid = Some(transition.id().to_string());
            }
            all_transitions
                .entry(transfer_info.asset_info.contract_id)
                .or_default()
                .push(transition.clone());
            psbt.push_rgb_transition(transition)
                .map_err(InternalError::from)?;
            asset_beneficiaries.insert(asset_id.clone(), beneficiaries);
        }

        for (id, state) in stock.assignments_by_contract(prev_outputs.clone())? {
            if transfer_info_map.contains_key(&id.to_string()) {
                continue;
            }
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
            let schema = stock.contract_schema(cid)?;
            for (outpoint, opout, state) in opout_state_map {
                let transition_type = schema.default_transition_for_assignment(&opout.ty);
                let mut extra_builder = stock.transition_builder_raw(cid, transition_type)?;
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
        write_fascia(&transfer_dir.join(FASCIA_FILE), &fascia)?;

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

            // every operation is checked, so that none spends a rejected allocation: a recipient
            // would refuse it, and an operation with no recipient (an inflation or a burn) would
            // carry rejected history anyway
            let reject_list_source =
                self.asset_reject_list_source(stock, &transfer_info.asset_info)?;
            let should_build_dag = reject_list_source.is_some();

            #[cfg(test)]
            let should_build_dag = if skip_build_dag() {
                false
            } else {
                should_build_dag
            };

            if should_build_dag {
                let (_, dag_data, _) = stock.transfer_from_fascia_with_dag(
                    transfer_info.asset_info.contract_id,
                    transfer_info.beneficiaries_witness.clone(),
                    transfer_info.beneficiaries_blinded.clone(),
                    transfer_info.requested_opids(&fascia),
                    &fascia,
                )?;

                let reject_list = self.get_reject_list(reject_list_source.as_ref().unwrap())?;
                let asset_opouts = input_opouts
                    .get(&transfer_info.asset_info.contract_id)
                    .unwrap();
                let asset_input_opouts = asset_opouts.keys().cloned().collect();
                let to_reject = rejected_opouts(&dag_data, &reject_list, &asset_input_opouts)?;
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
            sender_transport_endpoint,
            operation_secret_key,
        };
        let serialized_info =
            serde_json::to_string(&info_batch_transfer).map_err(InternalError::from)?;
        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        atomic_write(&info_file, serialized_info.as_bytes())?;

        Ok(PrepareRgbPsbtResult::Success(Box::new(
            BeginOperationData {
                psbt: psbt.clone(),
                transfer_dir: transfer_dir.clone(),
                info_batch_transfer,
                batch_transfer_idx: None,
                opid,
            },
        )))
    }

    #[allow(clippy::too_many_arguments)]
    fn post_transfer_data(
        &self,
        recipients: &mut Vec<LocalRecipient>,
        asset_transfer_dir: PathBuf,
        medias: HashSet<Media>,
        sender_sk: &ed25519_dalek::SigningKey,
        sender_pubkey_hex: &str,
        sender_transport_endpoint: Option<&str>,
        consignment_hash: &str,
        batch: bool,
    ) -> Result<(), Error> {
        let consignment_path = self.get_send_consignment_path_impl(&asset_transfer_dir);
        // media are identified per transfer by media_id, signing and encryption are deterministic,
        // so the same digest yields idempotent media.post calls within a transfer.
        let media_count = medias
            .iter()
            .map(|m| m.get_digest())
            .collect::<HashSet<_>>()
            .len();
        for recipient in recipients {
            let recipient_id = &recipient.recipient_id;
            if recipient.transport_endpoints.is_empty() {
                debug!(
                    self.logger(),
                    "No transport endpoints (out-of-band exchange), skipping post for recipient ID: {recipient_id}"
                );
                continue;
            }
            let sender_transport_endpoint =
                sender_transport_endpoint.ok_or_else(|| Error::Internal {
                    details: s!("missing sender transport endpoint"),
                })?;
            let recipient_vk = verifying_key_from_bytes(&recipient.public_key)?;
            let derived = derive_shared(sender_sk, &recipient_vk)?;
            let encrypt = recipient.encryption != Encryption::Unsupported;
            let (invoice_id, transfer_id) = recipient.proxy_ids(sender_pubkey_hex);

            // the (optionally encrypted) consignment, signed along with the hash of the uploaded
            // bytes, is the same for every transport endpoint
            let enc_tmp = if encrypt {
                Some(encrypt_to_tempfile(
                    &consignment_path,
                    &derived.sym_key,
                    consignment_hash,
                )?)
            } else {
                None
            };
            let post_path: &Path = match &enc_tmp {
                Some(tmp) => tmp.path(),
                None => consignment_path.as_path(),
            };
            let blob_hash = hash_file(post_path)?;
            let consignment_data = PostConsignmentData {
                invoice_id,
                transfer_id: transfer_id.clone(),
                sender_transport_endpoint: sender_transport_endpoint.to_string(),
                sender_pk: sender_pubkey_hex.to_string(),
                consignment_hash: consignment_hash.to_string(),
                batch,
                signature: hex::encode(sign_ed25519(
                    sender_sk,
                    &sha256_digest(
                        post_consignment_message(&transfer_id, consignment_hash, &blob_hash)
                            .as_bytes(),
                    ),
                )),
            };

            let mut found_valid = false;
            // the error of the last endpoint that refused to create the transfer, if any
            let mut refusal = None;
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
                let proxy_client = ProxyClient::new(&proxy_url)?;

                // skip a proxy that would refuse some of the media after the consignment is posted,
                // or that doesn't tell how many it accepts (unreachable or invalid max_media)
                if media_count > 0 {
                    let max_media = proxy_client
                        .get_info()
                        .ok()
                        .and_then(|res| res.result)
                        .as_ref()
                        .and_then(proxy_max_media);
                    let Some(max_media) = max_media else {
                        debug!(self.logger(), "Proxy {proxy_url} doesn't report max media");
                        continue;
                    };
                    if media_count > max_media {
                        debug!(
                            self.logger(),
                            "Proxy {proxy_url} accepts {max_media} media, {media_count} needed"
                        );
                        refusal = Some(Error::ProxyTooManyMedia);
                        continue;
                    }
                }

                match self.post_consignment_with_pow(&proxy_client, &consignment_data, post_path)? {
                    Ok(()) => {}
                    // the transfer cannot be created on this transport endpoint
                    Err(
                        e @ (Error::InvoiceNotReserved
                        | Error::InvoiceExpired
                        | Error::InvoiceQuotaExceeded
                        | Error::TransferIDDeleted
                        | Error::ProxyDraining),
                    ) => {
                        refusal = Some(e);
                        continue;
                    }
                    Err(e @ Error::ProxyCannotChange { .. }) => return Err(e),
                    Err(_) => continue,
                }

                // post the (optionally encrypted) media attachment(s)
                for media in &medias {
                    let digest = media.get_digest();
                    let media_id = derived.media_id(&digest);
                    let media_tmp = if encrypt {
                        Some(encrypt_to_tempfile(
                            Path::new(&media.file_path),
                            &derived.sym_key,
                            &digest,
                        )?)
                    } else {
                        None
                    };
                    let media_post_path: &Path = match &media_tmp {
                        Some(tmp) => tmp.path(),
                        None => Path::new(&media.file_path),
                    };
                    let media_blob_hash = hash_file(media_post_path)?;
                    let media_signature = hex::encode(sign_ed25519(
                        sender_sk,
                        &sha256_digest(
                            post_media_message(&transfer_id, &media_id, &media_blob_hash)
                                .as_bytes(),
                        ),
                    ));
                    let media_res = proxy_client.post_media(
                        &transfer_id,
                        &media_id,
                        media_post_path,
                        &media_signature,
                    )?;
                    debug!(self.logger(), "Attachment POST response: {:?}", media_res);
                    if let Some(err) = media_res.error {
                        return Err(map_proxy_error(err));
                    }
                }

                transport_endpoint.used = true;
                found_valid = true;
                break;
            }
            if !found_valid {
                return Err(refusal.unwrap_or(Error::NoValidTransportEndpoint));
            }
        }

        Ok(())
    }

    /// Post a consignment, computing the proof of work the recipient proxy requires: a proxy under
    /// attack can raise the difficulty at any time, in which case it reports the current one and
    /// the post is retried with a new proof of work, a limited number of times.
    ///
    /// The outer result is for errors that should stop the operation, the inner one for errors
    /// specific to the proxy, which allow trying another transport endpoint.
    fn post_consignment_with_pow(
        &self,
        proxy_client: &ProxyClient,
        data: &PostConsignmentData,
        consignment_path: &Path,
    ) -> Result<Result<(), Error>, Error> {
        const MAX_POW_ATTEMPTS: u8 = 3;
        let mut difficulty = 0;
        for _ in 0..MAX_POW_ATTEMPTS {
            let pow_timestamp = now().unix_timestamp().to_string();
            let pow_nonce = solve_pow(&data.transfer_id, &pow_timestamp, difficulty);
            let res = proxy_client.post_consignment(
                data,
                consignment_path,
                &pow_timestamp,
                &pow_nonce,
            )?;
            debug!(self.logger(), "Consignment POST response: {:?}", res);
            let Some(err) = res.error else {
                // skip proxies with a non-compliant API
                return Ok(match res.result {
                    Some(_) => Ok(()),
                    None => Err(Error::Proxy {
                        details: s!("missing consignment.post result"),
                    }),
                });
            };
            match pow_difficulty_required(&err) {
                Some(required) if required <= MAX_POW_DIFFICULTY => {
                    debug!(
                        self.logger(),
                        "Proof of work difficulty {required} required, retrying"
                    );
                    difficulty = required;
                }
                Some(required) => {
                    warn!(
                        self.logger(),
                        "Proof of work difficulty {required} exceeds the maximum accepted"
                    );
                    return Ok(Err(map_proxy_error(err)));
                }
                None => return Ok(Err(map_proxy_error(err))),
            }
        }
        Ok(Err(Error::Proxy {
            details: s!("proof of work not accepted"),
        }))
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

    /// Hash (SHA-256) the plaintext consignment generated for each asset of the batch transfer in
    /// the given transfer directory, keyed by asset ID. Every asset must have its consignment on
    /// disk already, so this is only for operations producing one (a transfer, a burn or a mint),
    /// once the consignments have been generated.
    fn consignment_hashes(
        &self,
        transfer_dir: &Path,
        info_contents: &InfoBatchTransfer,
    ) -> Result<HashMap<String, String>, Error> {
        let mut consignment_hashes = HashMap::new();
        for asset_id in info_contents.transfers.keys() {
            let atd = self.get_asset_transfer_dir(transfer_dir, asset_id);
            let cpath = self.get_send_consignment_path_impl(&atd);
            consignment_hashes.insert(asset_id.clone(), hash_file(&cpath)?);
        }
        Ok(consignment_hashes)
    }

    fn save_transfers(
        &mut self,
        txn: &DbTxn,
        txid: String,
        info_contents: &InfoBatchTransfer,
        status: TransferStatus,
        consignment_hashes: &HashMap<String, String>,
        psbt: &Psbt,
    ) -> Result<i32, Error> {
        let batch_transfer = DbBatchTransferActMod {
            txid: ActiveValue::Set(Some(txid.clone())),
            status: ActiveValue::Set(status),
            expiration: ActiveValue::Set(info_contents.expiration_timestamp),
            created_at: ActiveValue::Set(info_contents.created_at),
            min_confirmations: ActiveValue::Set(info_contents.min_confirmations),
            incoming: ActiveValue::Set(false),
            operation_secret_key: ActiveValue::Set(info_contents.operation_secret_key.clone()),
            ..Default::default()
        };
        let batch_transfer_idx = txn.set_batch_transfer(batch_transfer)?;
        let sender_pubkey_hex = match &info_contents.operation_secret_key {
            Some(seed) => {
                let seed: [u8; 32] = seed.clone().try_into().map_err(|_| Error::Internal {
                    details: s!("invalid operation secret key"),
                })?;
                Some(hex::encode(
                    ed25519_from_seed(&seed).verifying_key().to_bytes(),
                ))
            }
            None => None,
        };

        // until the TX is broadcast, reserve all its inputs, including the ones with no
        // allocations (e.g. added to pay the fee), so they can't be selected by other operations
        if matches!(
            status,
            TransferStatus::Initiated | TransferStatus::WaitingCounterparty
        ) {
            self.reserve_colored_txos(txn, psbt, batch_transfer_idx)?;
        }

        let mut change_utxo_idx: Option<i32> = None;

        for (asset_id, transfer_info) in info_contents.transfers.iter() {
            // hash of the plaintext consignment for this asset, if already generated: a non-dry-run
            // begin saves the transfers before generating the consignments (see the backfill in
            // update_or_save_transfers) and an inflation never produces one
            let consignment_hash = consignment_hashes.get(asset_id).cloned();
            let asset_transfer = DbAssetTransferActMod {
                user_driven: ActiveValue::Set(true),
                batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
                asset_id: ActiveValue::Set(Some(asset_id.clone())),
                transition_type: ActiveValue::Set(Some(transfer_info.main_transition)),
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
            if transfer_info.change.mint > 0 {
                let txo_idx =
                    self.get_change_utxo_idx(txn, &txid, info_contents, &mut change_utxo_idx)?;
                for _ in 0..transfer_info.change.mint {
                    let db_coloring = DbColoringActMod {
                        txo_idx: ActiveValue::Set(txo_idx),
                        asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                        r#type: ActiveValue::Set(ColoringType::Change),
                        assignment: ActiveValue::Set(Assignment::MintRight),
                        ..Default::default()
                    };
                    txn.set_coloring(db_coloring)?;
                }
            }

            for recipient in &transfer_info.recipients {
                // the transfer ID on the proxies, only for a proxy recipient
                let proxy_transfer_id = match &sender_pubkey_hex {
                    Some(sender_pk) if !recipient.public_key.is_empty() => {
                        Some(recipient.proxy_ids(sender_pk).1)
                    }
                    _ => None,
                };
                let (rcpt_id, rcpt_type, req_ass) = match transfer_info.main_transition {
                    TypeOfTransition::Inflate => {
                        let local_witness_data = if let LocalRecipientData::Witness(lwd) =
                            &recipient.local_recipient_data
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
                            recipient.assignment.clone(),
                        )
                    }
                    TypeOfTransition::Burn => (
                        None,
                        None,
                        Assignment::Fungible(transfer_info.original_assignments_needed.fungible),
                    ),
                    TypeOfTransition::Transfer | TypeOfTransition::Mint => (
                        Some(recipient.recipient_id.clone()),
                        None,
                        recipient.assignment.clone(),
                    ),
                };

                let recipient_pubkey = if recipient.public_key.is_empty() {
                    None
                } else {
                    Some(recipient.public_key.clone())
                };
                let transfer = DbTransferActMod {
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    requested_assignment: ActiveValue::Set(Some(req_ass)),
                    recipient_id: ActiveValue::Set(rcpt_id),
                    recipient_type: ActiveValue::Set(rcpt_type),
                    recipient_pubkey: ActiveValue::Set(recipient_pubkey),
                    consignment_hash: ActiveValue::Set(consignment_hash.clone()),
                    recipient_encryption: ActiveValue::Set(recipient.encryption),
                    proxy_transfer_id: ActiveValue::Set(proxy_transfer_id),
                    invoice_expiration: ActiveValue::Set(recipient.invoice_expiration),
                    ..Default::default()
                };
                let transfer_idx = txn.set_transfer(transfer)?;
                let out_of_band = recipient.transport_endpoints.is_empty();
                for transport_endpoint in &recipient.transport_endpoints {
                    self.save_transfer_transport_endpoint(txn, transfer_idx, transport_endpoint)?;
                }
                // the sender proxy endpoint (where the ACK is collected and the signed TX released)
                // only concerns proxy recipients: an out-of-band recipient is not registered there
                if let (Some(endpoint), false) =
                    (&info_contents.sender_transport_endpoint, out_of_band)
                {
                    let url = self.convert_transport_endpoint_url(endpoint)?;
                    self.save_sender_transport_endpoint(txn, transfer_idx, &url)?;
                }
            }
        }

        for (asset_id, txo_assignments) in &info_contents.extra_allocations {
            // the allocations of other assets are moved along by transfer transitions
            let asset_transfer = DbAssetTransferActMod {
                user_driven: ActiveValue::Set(false),
                batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
                asset_id: ActiveValue::Set(Some(asset_id.clone())),
                transition_type: ActiveValue::Set(Some(TypeOfTransition::Transfer)),
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
        consignment_hashes: &HashMap<String, String>,
        sync_tte_used: bool,
        psbt: &Psbt,
    ) -> Result<i32, Error> {
        // in a send-to-oneself, the txid is shared between send and receive transfers; only the
        // outgoing batch transfer should be updated here, incoming ones must be ignored so the send
        // transfers are created
        if let Some(existing) = txn
            .get_batch_transfers_by_txid(&txid)?
            .into_iter()
            .find(|batch_transfer| !batch_transfer.incoming)
        {
            let mut updated: DbBatchTransferActMod = existing.clone().into();
            updated.status = ActiveValue::Set(status);
            txn.update_batch_transfer(&mut updated)?;

            // a non-dry-run begin saves the transfers before generating the consignments, so their
            // consignment_hash is None: backfill it now that the consignments exist (for a transfer
            // or a burn, an inflation never produces one)
            for (asset_id, consignment_hash) in consignment_hashes {
                for transfer in txn.get_transfers_for_asset(existing.idx, asset_id)? {
                    if transfer.consignment_hash.is_none() {
                        let mut updated_transfer: DbTransferActMod = transfer.into();
                        updated_transfer.consignment_hash =
                            ActiveValue::Set(Some(consignment_hash.clone()));
                        txn.update_transfer(&mut updated_transfer)?;
                    }
                }
            }

            if sync_tte_used {
                let asset_transfers = txn.iter_asset_transfers()?;
                let transfers = txn.iter_transfers()?;
                let batch_data = existing.get_transfers(&asset_transfers, &transfers)?;
                // copy the used flags set while posting consignments to the DB rows persisted by
                // a non-dry-run begin; only actual recipients have transport endpoints, so
                // non-user-driven asset transfers (extra allocations) are never visited
                let db_transfers: Vec<&DbTransfer> = batch_data
                    .asset_transfers_data
                    .iter()
                    .flat_map(|a| a.transfers.iter())
                    .collect();
                for recipient in info_contents.transfers.values().flat_map(|t| &t.recipients) {
                    let db_transfer = db_transfers
                        .iter()
                        .find(|t| {
                            t.recipient_id.as_deref() == Some(recipient.recipient_id.as_str())
                        })
                        .expect("transfer should be set");
                    let tte_data = txn.get_transfer_transport_endpoints_data(db_transfer.idx)?;
                    for (tte, te) in tte_data {
                        // only the RecipientProxy kind is of interest here
                        if tte.kind != TransportEndpointKind::RecipientProxy {
                            continue;
                        }
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
            Ok(existing.idx)
        } else {
            self.save_transfers(txn, txid, info_contents, status, consignment_hashes, psbt)
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

    fn get_transfer_begin_data<'txn>(
        &mut self,
        txn: &'txn DbTxn,
        fee_rate: u64,
    ) -> Result<
        (
            FeeRate,
            Vec<LocalUnspent>,
            Vec<LocalUnspent>,
            RgbStock<'txn>,
        ),
        Error,
    > {
        let fee_rate_checked = self.check_fee_rate(fee_rate)?;

        let db_data = txn.get_db_data(false)?;

        let utxos = txn.get_unspent_txos(db_data.txos)?;

        let mut unspents = txn.get_rgb_allocations(
            utxos,
            Some(db_data.colorings),
            Some(db_data.batch_transfers),
            Some(db_data.asset_transfers),
            Some(db_data.transfers),
        )?;
        // UTXOs reserved to be spent by a pending operation can't be used
        let reserved: HashSet<Outpoint> = self
            .get_reserved_colored_outpoints(txn)?
            .into_iter()
            .collect();
        unspents.retain(|u| !reserved.contains(&u.utxo.outpoint()));

        #[cfg(test)]
        let input_unspents = mock_input_unspents(self, &unspents);
        #[cfg(not(test))]
        let input_unspents = self.get_input_unspents(&unspents)?;

        let stock = self.rgb_stock(txn)?;

        Ok((fee_rate_checked, unspents, input_unspents, stock))
    }

    /// Check that a recipient `assignment` is valid for an asset with the given `schema`.
    fn check_recipient_assignment(
        assignment: &Assignment,
        schema: AssetSchema,
    ) -> Result<(), Error> {
        if !schema.supports_assignment(assignment) {
            return Err(Error::InvalidAssignment);
        }
        if matches!(
            assignment,
            Assignment::Fungible(0) | Assignment::InflationRight(0)
        ) {
            return Err(Error::InvalidAmountZero);
        }
        Ok(())
    }

    /// Parse and validate the given recipient into a [`LocalRecipient`]. A recipient with no
    /// transport endpoints uses the out-of-band exchange.
    #[allow(clippy::too_many_arguments)]
    fn parse_recipient(
        &self,
        recipient: &Recipient,
        schema: AssetSchema,
        chainnet: ChainNet,
        witness_recipients: &mut Vec<(ScriptBuf, u64)>,
        recipient_vout: &mut u32,
        operation_sk: &ed25519_dalek::SigningKey,
        now_ts: i64,
    ) -> Result<LocalRecipient, Error> {
        let out_of_band = recipient.transport_endpoints.is_empty();
        if !out_of_band {
            self.check_transport_endpoints(&recipient.transport_endpoints)?;
        }
        Self::check_recipient_assignment(&recipient.assignment, schema)?;
        let mut transport_endpoints: Vec<LocalTransportEndpoint> = vec![];
        let mut found_valid = false;
        if out_of_band {
            if !self.supports_out_of_band_exchange() {
                return Err(Error::UnsupportedTransportType);
            }
            // out-of-band exchange: no transport endpoint is stored (its absence marks the
            // transfer as out-of-band)
            found_valid = true;
        } else {
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
        }

        if !found_valid {
            return Err(Error::InvalidTransportEndpoints {
                details: s!("no valid transport endpoints"),
            });
        }

        let xchainnet_beneficiary = XChainNet::<Beneficiary>::from_str(&recipient.recipient_id)
            .map_err(|_| Error::InvalidRecipientID)?;

        if xchainnet_beneficiary.chain_network() != chainnet {
            return Err(Error::InvalidRecipientNetwork);
        }

        // decode and validate the recipient's Ed25519 public key, needed to encrypt data for and
        // verify the ACK from a proxy recipient; the out-of-band exchange has no use for it, so
        // it is ignored there
        let (public_key, recipient_vk) = if out_of_band {
            (vec![], None)
        } else {
            let public_key_hex =
                recipient
                    .public_key
                    .as_deref()
                    .ok_or_else(|| Error::InvalidPubkey {
                        details: s!("missing recipient public key"),
                    })?;
            let public_key = hex::decode(public_key_hex).map_err(|_| Error::InvalidPubkey {
                details: s!("recipient public key is not valid hex"),
            })?;
            let recipient_vk = verifying_key_from_bytes(&public_key)?;
            (public_key, Some(recipient_vk))
        };

        // a transfer to a proxy recipient cannot be created after the invoice expiration
        let invoice_expiration = if out_of_band {
            None
        } else {
            let invoice_expiration =
                recipient
                    .expiration_timestamp
                    .ok_or_else(|| Error::InvalidRecipientData {
                        details: s!("missing invoice expiration"),
                    })? as i64;
            if invoice_expiration <= now_ts {
                return Err(Error::InvoiceExpired);
            }
            Some(invoice_expiration)
        };

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
                    // the blinding of a proxy recipient seal is derived from the keys of both
                    // parties (v1.0-changes-spec.md A.11), so the sender cannot choose it
                    let blinding = if out_of_band {
                        witness_data.blinding
                    } else {
                        if witness_data.blinding.is_some() {
                            return Err(Error::InvalidRecipientData {
                                details: s!(
                                    "cannot provide a blinding for a witness recipient with transport endpoints"
                                ),
                            });
                        }
                        let recipient_vk =
                            recipient_vk.as_ref().expect("set for a proxy recipient");
                        Some(derive_shared(operation_sk, recipient_vk)?.seal_blinding)
                    };
                    let local_witness_data = LocalWitnessData {
                        amount_sat: witness_data.amount_sat,
                        blinding,
                        vout: *recipient_vout,
                    };
                    *recipient_vout += 1;
                    LocalRecipientData::Witness(local_witness_data)
                } else {
                    return Err(Error::InvalidRecipientData {
                        details: s!("missing witness data for a witness recipient"),
                    });
                }
            }
        };

        Ok(LocalRecipient {
            recipient_id: recipient.recipient_id.clone(),
            local_recipient_data,
            assignment: recipient.assignment.clone(),
            transport_endpoints,
            public_key,
            encryption: recipient.encryption,
            invoice_expiration,
        })
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

    /// Prepare the PSBT of an operation on a single asset (an inflation, a burn or a mint),
    /// retrying, as a send does, without the inputs found rejected by the asset's reject list:
    /// more inputs are then selected to make up for them, failing if there aren't enough.
    #[allow(clippy::too_many_arguments)]
    fn prepare_single_asset_psbt(
        &mut self,
        txn: &DbTxn,
        transfer_info: InfoAssetTransfer,
        transfer_dir: PathBuf,
        unspents: Vec<LocalUnspent>,
        input_unspents: &[LocalUnspent],
        witness_recipients: &Vec<(ScriptBuf, u64)>,
        fee_rate_checked: FeeRate,
        min_confirmations: u8,
        expiration_timestamp: Option<i64>,
        sender_transport_endpoint: Option<String>,
        operation_secret_key: Option<Vec<u8>>,
        stock: &mut RgbStock<'_>,
        dry_run: bool,
    ) -> Result<BeginOperationData, Error> {
        let asset_id = transfer_info.asset_info.contract_id.to_string();
        let mut rejected = HashSet::new();
        let mut transfer_info_map = BTreeMap::from([(asset_id.clone(), transfer_info.clone())]);
        loop {
            match self.prepare_transfer_psbt(
                txn,
                &mut transfer_info_map,
                transfer_dir.clone(),
                false,
                unspents.clone(),
                input_unspents,
                witness_recipients,
                fee_rate_checked,
                min_confirmations,
                expiration_timestamp,
                sender_transport_endpoint.clone(),
                operation_secret_key.clone(),
                stock,
                &mut rejected,
                dry_run,
            )? {
                PrepareTransferPsbtResult::Retry => {
                    // start over from the original transfer info, with the assignments needed
                    // increased by the rejected ones
                    let assignments_needed =
                        transfer_info_map[&asset_id].assignments_needed.clone();
                    let asset_spend = self.select_rgb_inputs(
                        asset_id.clone(),
                        &assignments_needed,
                        input_unspents.to_vec(),
                    )?;
                    transfer_info_map = BTreeMap::from([(
                        asset_id.clone(),
                        InfoAssetTransfer {
                            asset_spend,
                            assignments_needed,
                            ..transfer_info.clone()
                        },
                    )]);
                }
                PrepareTransferPsbtResult::Success(begin_operation_data) => {
                    return Ok(*begin_operation_data);
                }
            }
        }
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
        sender_transport_endpoint: Option<String>,
        operation_secret_key: Option<Vec<u8>>,
        stock: &mut RgbStock<'_>,
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
            stock,
            min_confirmations,
            expiration_timestamp,
            btc_change,
            sender_transport_endpoint,
            operation_secret_key,
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
        atomic_write(
            &new_transfer_dir.join(UNSIGNED_PSBT_FILE),
            begin_operation_data.psbt.to_string().as_bytes(),
        )?;

        // update transfer_dir to the new (renamed) directory
        let mut begin_operation_data = begin_operation_data;
        begin_operation_data.transfer_dir = new_transfer_dir;

        if !dry_run {
            // save transfer to DB with Initiated status to reserve the UTXOs; no consignment has
            // been generated yet, their hashes get backfilled when the operation ends
            let batch_transfer_idx = self.save_transfers(
                txn,
                txid,
                &begin_operation_data.info_batch_transfer,
                TransferStatus::Initiated,
                &HashMap::new(),
                &begin_operation_data.psbt,
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
        consignment_hashes: &HashMap<String, String>,
        fascia: Fascia,
        sync_tte_used: bool,
    ) -> Result<i32, Error> {
        let mut stock = self.rgb_stock(txn)?;
        self.broadcast_and_update_rgb(txn, &mut stock, psbt, fascia)?;
        self.update_or_save_transfers(
            txn,
            txid,
            info_contents,
            status,
            consignment_hashes,
            sync_tte_used,
            psbt,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn send_begin_impl(
        &mut self,
        txn: &DbTxn,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        expiration_timestamp: Option<i64>,
        sender_transport_endpoint: Option<String>,
        dry_run: bool,
    ) -> Result<BeginOperationData, Error> {
        if recipient_map.is_empty() || recipient_map.values().any(|v| v.is_empty()) {
            return Err(Error::InvalidRecipientMap);
        }
        if let Some(expiration_timestamp) = expiration_timestamp {
            check_operation_expiration(expiration_timestamp)?;
        }
        if let Some(endpoint) = &sender_transport_endpoint {
            self.convert_transport_endpoint_url(endpoint)?;
        }

        let operation_sk = gen_ed25519();
        let operation_secret_key = operation_sk.to_bytes().to_vec();
        let now_ts = now().unix_timestamp();
        // earliest invoice expiration among the proxy recipients, bounding the operation one
        let mut min_invoice_expiration: Option<i64> = None;

        let (fee_rate_checked, unspents, input_unspents, mut stock) =
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
            for recipient in recipients {
                // a proxy recipient posts its ACK to (and gets the signed TX from) the sender
                // proxy, so one is needed
                if !recipient.transport_endpoints.is_empty() && sender_transport_endpoint.is_none()
                {
                    return Err(Error::InvalidTransportEndpoint {
                        details: s!("missing sender transport endpoint"),
                    });
                }
                let local_recipient = self.parse_recipient(
                    recipient,
                    schema,
                    chainnet,
                    &mut witness_recipients,
                    &mut recipient_vout,
                    &operation_sk,
                    now_ts,
                )?;
                if let Some(invoice_expiration) = local_recipient.invoice_expiration {
                    min_invoice_expiration = Some(
                        min_invoice_expiration
                            .map_or(invoice_expiration, |m| std::cmp::min(m, invoice_expiration)),
                    );
                }
                local_recipients
                    .entry(asset_id.clone())
                    .or_default()
                    .push(local_recipient);

                recipient
                    .assignment
                    .add_to_assignments(&mut original_assignments_needed);
            }
            // a single mint right is enough to send any number of them, as the schema allows
            // mint rights to be duplicated (a transfer needs at least as many outputs as inputs)
            original_assignments_needed.mint = min(original_assignments_needed.mint, 1);
            let contract_id = ContractId::from_str(asset_id).expect("invalid contract ID");
            assets_data.insert(
                asset_id.clone(),
                (
                    AssetInfo {
                        contract_id,
                        schema,
                        reject_list_url: asset.reject_list_url,
                    },
                    original_assignments_needed,
                ),
            );
        }

        // the operation must expire sufficiently before every invoice, so its TX can only be
        // released while the recipients are still waiting for it (v1.0-changes-spec.md 6.1)
        if !donation
            && let (Some(expiration), Some(min_invoice_expiration)) =
                (expiration_timestamp, min_invoice_expiration)
            && expiration > min_invoice_expiration - invoice_expiration_margin() as i64
        {
            return Err(Error::InvalidExpiration);
        }

        let receive_ids: Vec<String> = recipient_map
            .values()
            .flatten()
            .map(|r| r.recipient_id.clone())
            .collect();
        let transfer_dir = self.setup_transfer_directory(receive_ids)?;

        let mut rejected = HashSet::new();
        let mut transfer_info_map: BTreeMap<String, InfoAssetTransfer> = BTreeMap::new();
        let mut invoices_checked = false;

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
                    burn_reason: None,
                    after_block: None,
                };
                transfer_info_map.insert(asset_id.clone(), transfer_info);
            }

            // now that the assignments are confirmed available (select_rgb_inputs succeeded), ensure
            // each invoice can still accept a transfer on (one of) its proxies, proceeding only if
            // a proxy reports it as available. Done here (not earlier) so an InsufficientAssignments
            // error takes precedence
            if !invoices_checked {
                invoices_checked = true;
                for recipients in local_recipients.values() {
                    // out-of-band recipients have no proxy to check
                    for lr in recipients
                        .iter()
                        .filter(|lr| !lr.transport_endpoints.is_empty())
                    {
                        let (invoice_id, _) = lr.proxy_ids("");
                        let mut full = false;
                        let mut unknown = false;
                        let mut available = false;
                        for lte in lr.transport_endpoints.iter().filter(|e| e.usable) {
                            // a proxy that cannot be asked is skipped, as another one may still
                            // have the invoice available
                            let result = match ProxyClient::new(&lte.endpoint)
                                .and_then(|c| c.check_invoices(std::slice::from_ref(&invoice_id)))
                            {
                                Ok(res) => res.result,
                                Err(e) => {
                                    debug!(self.logger(), "Invoice check error: {e}");
                                    continue;
                                }
                            };
                            match result.and_then(|r| r.get(&invoice_id).map(|r| r.status)) {
                                Some(InvoiceStatus::Available) => {
                                    available = true;
                                    break;
                                }
                                Some(InvoiceStatus::Full) => full = true,
                                Some(InvoiceStatus::Unknown) => unknown = true,
                                None => {}
                            }
                        }
                        if !available {
                            return Err(if full {
                                Error::InvoiceQuotaExceeded
                            } else if unknown {
                                Error::InvoiceNotReserved
                            } else {
                                Error::NoValidTransportEndpoint
                            });
                        }
                    }
                }
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
                sender_transport_endpoint.clone(),
                Some(operation_secret_key.clone()),
                &mut stock,
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
        // a send ended again (e.g. retried after a crash) is completed only while it's still in
        // progress: one that has failed (which revoked its signed TX on the sender proxy) or
        // progressed must not be brought back to waiting for the counterparty. This is checked
        // before reading the transfer data, which doesn't parse for a send from before the proxy
        // protocol update (failed by the upgrade)
        let txid = signed_psbt.unsigned_tx.compute_txid().to_string();
        if let Some(existing) = txn
            .get_batch_transfers_by_txid(&txid)?
            .into_iter()
            .find(|batch_transfer| !batch_transfer.incoming)
            && !matches!(
                existing.status,
                TransferStatus::Initiated | TransferStatus::WaitingCounterparty
            )
        {
            return Err(Error::CannotEndTransfer { txid });
        }

        let (_, transfer_dir, mut info_contents, mut fascia) =
            self.get_transfer_end_data(signed_psbt)?;

        // the data must be of this operation: another one would be completed skipping its checks
        // and steps
        if !info_contents.is_operation(&TypeOfTransition::Transfer) {
            return Err(Error::WrongOperation { txid });
        }

        // extract the signed transaction (needed for its hash and, for donations, to embed it)
        let tx = signed_psbt
            .clone()
            .extract_tx()
            .map_err(InternalError::from)?;
        let tx_bytes = bdk_wallet::bitcoin::consensus::encode::serialize(&tx);
        let signed_tx_hash_raw = sha256_digest(&tx_bytes);
        let signed_tx_hash = hex::encode(signed_tx_hash_raw);

        // for donations, embed the signed transaction in the consignment so also the recipient can
        // broadcast it
        if info_contents.donation {
            fascia.update_witness_tx(tx.clone());
        }

        self.gen_consignments(txn, &fascia, &info_contents.transfers, &transfer_dir)?;

        let psbt_out = transfer_dir.join(SIGNED_PSBT_FILE);
        atomic_write(&psbt_out, signed_psbt.to_string().as_bytes())?;

        // sender operation key (one per operation, shared across all recipients)
        let seed: [u8; 32] = info_contents
            .operation_secret_key
            .clone()
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::Internal {
                details: s!("missing operation secret key"),
            })?;
        let sender_sk = ed25519_from_seed(&seed);
        let sender_pubkey_hex = hex::encode(sender_sk.verifying_key().to_bytes());
        // only needed by proxy recipients (checked by send_begin), absent for an all-out-of-band send
        let sender_transport_endpoint = info_contents.sender_transport_endpoint.clone();

        let total_recipients: usize = info_contents
            .transfers
            .values()
            .map(|t| t.recipients.len())
            .sum();
        let batch = total_recipients > 1;

        let consignment_hashes = self.consignment_hashes(&transfer_dir, &info_contents)?;

        // ACK preparation on the sender's own proxy (non-donation only, and only when at least one
        // recipient uses a proxy), before posting consignments
        let has_proxy_recipients = info_contents.transfers.values().any(|t| {
            t.recipients
                .iter()
                .any(|r| !r.transport_endpoints.is_empty())
        });
        // expiration plus the sender proxy retention grace, until which recipients may still
        // obtain and broadcast the signed TX (v1.0-changes-spec.md 6.2)
        let mut broadcast_deadline = None;
        // whether the operation was registered earlier and has expired since, so there is nothing
        // left to post
        let mut operation_expired = false;
        if !info_contents.donation && has_proxy_recipients {
            let mut transfer_data: HashMap<String, AckTransferData> = HashMap::new();
            for (asset_id, info_asset) in &info_contents.transfers {
                let consignment_hash = consignment_hashes[asset_id].clone();
                for recipient in &info_asset.recipients {
                    // out-of-band recipients never contact the sender proxy (their ACK and the
                    // signed TX are exchanged out-of-band), so they are not registered there
                    if recipient.transport_endpoints.is_empty() {
                        continue;
                    }
                    let vk = verifying_key_from_bytes(&recipient.public_key)?;
                    let derived = derive_shared(&sender_sk, &vk)?;
                    let enc_tx = encrypt_blob(&tx_bytes, &derived.sym_key, &signed_tx_hash_raw)?;
                    let (_, transfer_id) = recipient.proxy_ids(&sender_pubkey_hex);
                    transfer_data.insert(
                        transfer_id,
                        AckTransferData {
                            recipient_pk: hex::encode(&recipient.public_key),
                            consignment_hash: consignment_hash.clone(),
                            signed_tx: general_purpose::STANDARD.encode(&enc_tx),
                        },
                    );
                }
            }
            // a send with a counterparty always has an expiration (required by the public API)
            let expiration = info_contents
                .expiration_timestamp
                .ok_or_else(|| Error::Internal {
                    details: s!("missing expiration"),
                })?;
            let sender_transport_endpoint =
                sender_transport_endpoint
                    .as_deref()
                    .ok_or_else(|| Error::Internal {
                        details: s!("missing sender transport endpoint"),
                    })?;
            let sender_proxy_url =
                self.convert_transport_endpoint_url(sender_transport_endpoint)?;
            let retention_grace = sender_proxy_retention_grace(&sender_proxy_url)?;
            broadcast_deadline = Some(expiration + i64::from(retention_grace));
            let proxy_client = ProxyClient::new(&sender_proxy_url)?;
            let transfer_ids: Vec<String> = transfer_data.keys().cloned().collect();
            let expiration = expiration.to_string();
            // only needed by proxies with proxy-level authentication enabled
            let signature = self.online_data().as_ref().and_then(|online_data| {
                online_data.proxy_auth_signature(
                    &sender_proxy_url,
                    &prepare_ack_auth_message(
                        &sender_pubkey_hex,
                        &expiration,
                        &signed_tx_hash,
                        &transfer_data,
                    ),
                )
            });
            let res = proxy_client.prepare_ack(
                transfer_data,
                &expiration,
                &signed_tx_hash,
                &sender_pubkey_hex,
                signature,
            )?;
            if let Some(err) = res.error {
                match map_proxy_error(err) {
                    // a prior ACK clean (e.g. by another multisig cosigner) nulled the signed TX
                    // for this operation; a cosigner now completing it tries to re-supply the
                    // signed TX, which is expected and should be ignored
                    //
                    // signed_tx is deterministic (given the same tx bytes and keys) and every other
                    // field still matches what's registered, meaning this error can only happen in
                    // the described case (nulled) and is not a real conflict, so continue posting
                    // the consignment(s) and updating local state
                    Error::SignedTxCleared => {}
                    // the operation has expired: if it was registered earlier (by this wallet
                    // before a crash, or by another multisig cosigner) only the local state is
                    // left to update, from which the refresh settles or fails the operation,
                    // otherwise it can no longer be started
                    e @ Error::ExpirationInvalid => {
                        if !sender_proxy_knows_operation(&proxy_client, &transfer_ids, &sender_sk)?
                        {
                            return Err(e);
                        }
                        operation_expired = true;
                    }
                    e => return Err(e),
                }
            }
        }

        // an expired operation can no longer progress, so its consignments are not posted
        if !operation_expired {
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
                    AssetSchema::Nia | AssetSchema::Cfa | AssetSchema::Ifa | AssetSchema::Bfa => {
                        None
                    }
                };

                // post consignment(s) and optional media(s)
                let asset_transfer_dir = self.get_asset_transfer_dir(&transfer_dir, asset_id);
                let consignment_hash = consignment_hashes[asset_id].clone();
                self.post_transfer_data(
                    &mut info_contents_asset.recipients,
                    asset_transfer_dir,
                    self.get_asset_medias(txn, asset.media_idx, token)?,
                    &sender_sk,
                    &sender_pubkey_hex,
                    sender_transport_endpoint.as_deref(),
                    &consignment_hash,
                    batch,
                )?;
            }
        }

        let sync_tte_used = true;
        let batch_transfer_idx = if info_contents.donation {
            self.finalize_transfer_end(
                txn,
                txid.clone(),
                signed_psbt,
                &info_contents,
                TransferStatus::WaitingConfirmations,
                &consignment_hashes,
                fascia,
                sync_tte_used,
            )?
        } else {
            let batch_transfer_idx = self.update_or_save_transfers(
                txn,
                txid.clone(),
                &info_contents,
                TransferStatus::WaitingCounterparty,
                &consignment_hashes,
                sync_tte_used,
                signed_psbt,
            )?;
            if broadcast_deadline.is_some() {
                let batch_transfer = txn
                    .get_batch_transfers_by_txid(&txid)?
                    .into_iter()
                    .find(|bt| bt.idx == batch_transfer_idx)
                    .expect("batch transfer has just been saved");
                let mut updated: DbBatchTransferActMod = batch_transfer.into();
                updated.broadcast_deadline = ActiveValue::Set(broadcast_deadline);
                txn.update_batch_transfer(&mut updated)?;
            }
            batch_transfer_idx
        };

        // test-only: simulate a wallet crash after the consignment has been posted to
        // the proxy and the transfer has been saved, but before the enclosing DB transaction
        // is committed, so the save is rolled back while the proxy keeps the consignment.
        #[cfg(test)]
        if mock_send_end_crash() {
            return Err(Error::Internal {
                details: s!("simulated wallet crash in send_end"),
            });
        }

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
        let mut psbt = tx_builder.finish().map_err(|e| match e {
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
        self.add_non_witness_utxos(&mut psbt);

        if !dry_run {
            self.reserve_vanilla_txos(txn, &psbt, WalletTransactionType::SendBtc)?;
        }

        Ok(psbt)
    }

    fn send_btc_end_impl(&mut self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<String, Error> {
        self.check_vanilla_end(txn, signed_psbt, WalletTransactionType::SendBtc)?;
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
        if !schema.supports_inflation() {
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

        let (fee_rate_checked, unspents, input_unspents, mut stock) =
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
                public_key: vec![],
                encryption: Encryption::Unsupported,
                invoice_expiration: None,
            })
        }

        let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
        let asset_info = AssetInfo {
            contract_id,
            schema,
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
            burn_reason: None,
            after_block: None,
        };
        let receive_ids: Vec<String> = local_recipients
            .iter()
            .map(|lr| lr.recipient_id.clone())
            .collect();
        let transfer_dir = self.setup_transfer_directory(receive_ids)?;

        self.prepare_single_asset_psbt(
            txn,
            transfer_info,
            transfer_dir,
            unspents,
            &input_unspents,
            &witness_recipients,
            fee_rate_checked,
            min_confirmations,
            None,
            None,
            None,
            &mut stock,
            dry_run,
        )
    }

    /// Check that the operation of the given signed PSBT can still be ended. An operation saved by
    /// a non-dry-run begin is ended (e.g. again, retried after a crash) only while it's still
    /// initiated: one that has failed or progressed must not be broadcast again nor have its
    /// status brought back.
    fn check_end_initiated(&self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<(), Error> {
        let txid = signed_psbt.unsigned_tx.compute_txid().to_string();
        if let Some(existing) = txn
            .get_batch_transfers_by_txid(&txid)?
            .into_iter()
            .find(|batch_transfer| !batch_transfer.incoming)
            && existing.status != TransferStatus::Initiated
        {
            return Err(Error::CannotEndTransfer { txid });
        }
        Ok(())
    }

    fn inflate_end_impl(
        &mut self,
        txn: &DbTxn,
        signed_psbt: &Psbt,
    ) -> Result<OperationResult, Error> {
        self.check_end_initiated(txn, signed_psbt)?;
        let (txid, _transfer_dir, info_contents, fascia) =
            self.get_transfer_end_data(signed_psbt)?;

        // the data must be of this operation: another one would be completed skipping its checks
        // and steps
        if !info_contents.is_operation(&TypeOfTransition::Inflate) {
            return Err(Error::WrongOperation { txid });
        }

        // an inflation produces no consignment
        let batch_transfer_idx = self.finalize_transfer_end(
            txn,
            txid.clone(),
            signed_psbt,
            &info_contents,
            TransferStatus::WaitingConfirmations,
            &HashMap::new(),
            fascia,
            false,
        )?;

        let (asset_id, transfer_info) = info_contents.transfers.into_iter().next().unwrap();
        let inflation = transfer_info.original_assignments_needed.inflation;
        let db_asset = txn.get_asset(asset_id).unwrap().unwrap();
        let known_circulating_supply = db_asset
            .known_circulating_supply
            .as_ref()
            .unwrap()
            .parse::<u64>()
            .expect("DB should contain a valid known circulating supply");
        let updated_known_circulating_supply = known_circulating_supply
            .checked_add(inflation)
            .expect("known circulating supply plus inflation cannot exceed u64::MAX");
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
        burn_reason: Option<String>,
        fee_rate: u64,
        min_confirmations: u8,
        dry_run: bool,
    ) -> Result<BeginOperationData, Error> {
        let asset = txn.check_asset_exists(asset_id.clone())?;
        let schema = asset.schema;
        self.check_schema_support(&schema)?;
        if !schema.supports_burn() {
            return Err(Error::UnsupportedBurn {
                asset_schema: schema,
            });
        }

        if amount == 0 {
            return Err(Error::NoBurnAmount);
        }

        let burn_reason = burn_reason
            .map(|reason| self.check_burn_reason(reason))
            .transpose()?;

        let (fee_rate_checked, unspents, input_unspents, mut stock) =
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
            public_key: vec![],
            encryption: Encryption::Unsupported,
            invoice_expiration: None,
        }];

        let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
        let asset_info = AssetInfo {
            contract_id,
            schema,
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
            burn_reason,
            after_block: None,
        };
        let receive_ids: Vec<String> = local_recipients
            .iter()
            .map(|lr| lr.recipient_id.clone())
            .collect();
        let transfer_dir = self.setup_transfer_directory(receive_ids)?;

        self.prepare_single_asset_psbt(
            txn,
            transfer_info,
            transfer_dir,
            unspents,
            &input_unspents,
            &witness_recipients,
            fee_rate_checked,
            min_confirmations,
            None,
            None,
            None,
            &mut stock,
            dry_run,
        )
    }

    fn burn_end_impl(&mut self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<OperationResult, Error> {
        self.check_end_initiated(txn, signed_psbt)?;
        let (txid, transfer_dir, info_contents, mut fascia) =
            self.get_transfer_end_data(signed_psbt)?;

        // the data must be of this operation: another one would be completed skipping its checks
        // and steps
        if !info_contents.is_operation(&TypeOfTransition::Burn) {
            return Err(Error::WrongOperation { txid });
        }

        // embed the signed transaction, then generate the consignment proving the burn: a burn
        // has no recipient, so the consignment is not posted anywhere and is only made available
        // on disk (see the `consignment_path` of the operation)
        let tx = signed_psbt
            .clone()
            .extract_tx()
            .map_err(InternalError::from)?;
        fascia.update_witness_tx(tx);
        self.gen_consignments(txn, &fascia, &info_contents.transfers, &transfer_dir)?;
        let consignment_hashes = self.consignment_hashes(&transfer_dir, &info_contents)?;

        let batch_transfer_idx = self.finalize_transfer_end(
            txn,
            txid.clone(),
            signed_psbt,
            &info_contents,
            TransferStatus::WaitingConfirmations,
            &consignment_hashes,
            fascia,
            false,
        )?;

        Ok(OperationResult {
            txid,
            batch_transfer_idx,
            entropy: info_contents.entropy,
        })
    }

    /// The EVM RPC client for the chain the bridge of the given BFA contract lives on, checked
    /// to be connected to it, along with the contract's bridge location.
    ///
    /// A client on another chain would report meaningless block numbers and events for the
    /// bridge, so nothing about a mint is asked to an unchecked one.
    fn checked_evm_client(
        &self,
        stock: &RgbStock<'_>,
        contract_id: ContractId,
    ) -> Result<(EvmClient, BridgeLocation), Error> {
        let bridge_location = stock
            .contract_wrapper::<BridgedFungibleAsset>(contract_id)?
            .bridge_location();
        let BridgeLocation::Evm(EvmContract { chain_id, .. }) = &bridge_location;
        let evm_client = self.checked_evm_client_for_chain(*chain_id)?;
        Ok((evm_client, bridge_location))
    }

    /// The EVM RPC client configured for the chain with the given `chain_id`, checked to be
    /// connected to it.
    fn checked_evm_client_for_chain(&self, chain_id: u64) -> Result<EvmClient, Error> {
        checked_evm_client(self.evm_rpc_urls(), chain_id).inspect_err(|e| {
            if let Error::NoEvmRpcUrl { .. } = e {
                error!(
                    self.logger(),
                    "No EVM RPC URL configured for chain ID {chain_id}"
                );
            }
        })
    }

    /// The reject list of a known asset.
    ///
    /// The DB only records reject list URLs: a BFA reject list, which can be an EVM contract, is
    /// read off the contract instead.
    fn asset_reject_list_source(
        &self,
        stock: &RgbStock<'_>,
        asset_info: &AssetInfo,
    ) -> Result<Option<RejectListSource>, Error> {
        Ok(match asset_info.schema {
            AssetSchema::Bfa => stock
                .contract_wrapper::<BridgedFungibleAsset>(asset_info.contract_id)?
                .reject_list()
                .map(RejectListSource::from),
            AssetSchema::Ifa | AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa => asset_info
                .reject_list_url
                .clone()
                .map(RejectListSource::Url),
        })
    }

    fn mint_begin_impl(
        &mut self,
        txn: &DbTxn,
        asset_id: String,
        recipient: Recipient,
        fee_rate: u64,
        min_confirmations: u8,
        sender_transport_endpoint: String,
    ) -> Result<BeginOperationData, Error> {
        let asset = txn.check_asset_exists(asset_id.clone())?;
        let schema = asset.schema;
        self.check_schema_support(&schema)?;
        if !schema.supports_mint() {
            return Err(Error::UnsupportedMint {
                asset_schema: schema,
            });
        }

        match recipient.assignment {
            Assignment::Fungible(_) => {}
            _ => return Err(Error::InvalidAssignment),
        };

        let operation_sk = gen_ed25519();
        let operation_secret_key = operation_sk.to_bytes().to_vec();

        let (fee_rate_checked, unspents, input_unspents, mut stock) =
            self.get_transfer_begin_data(txn, fee_rate)?;

        // refuse to spend the last mint right to avoid leaving the wallet unable to mint
        let mint_rights = input_unspents
            .iter()
            .flat_map(|u| &u.rgb_allocations)
            .filter(|a| {
                a.asset_id.as_ref() == Some(&asset_id)
                    && a.status.settled()
                    && a.assignment == Assignment::MintRight
            })
            .count();
        if mint_rights == 1 {
            return Err(Error::LastMintRight);
        }

        let assignments_needed = AssignmentsCollection {
            mint: 1,
            ..Default::default()
        };
        let asset_spend = self.select_rgb_inputs(
            asset_id.clone(),
            &assignments_needed,
            input_unspents.clone(),
        )?;

        let mut local_recipients = vec![];

        let chainnet: ChainNet = self.bitcoin_network().into();
        let mut witness_recipients: Vec<(ScriptBuf, u64)> = vec![];
        let mut recipient_vout = 1;
        // a mint recipient is always a proxy one
        if recipient.transport_endpoints.is_empty() {
            return Err(Error::InvalidTransportEndpoints {
                details: s!("no valid transport endpoints"),
            });
        }
        let now_ts = now().unix_timestamp();
        let local_recipient = self.parse_recipient(
            &recipient,
            schema,
            chainnet,
            &mut witness_recipients,
            &mut recipient_vout,
            &operation_sk,
            now_ts,
        )?;
        // the lock has to be included on the external chain by the invoice expiration, which is
        // therefore the mint's expiration: refuse an invoice leaving too little time for it. No
        // operation expiration is needed, as a mint is never registered on the sender proxy
        let expiration_timestamp = local_recipient
            .invoice_expiration
            .expect("set for a proxy recipient");
        if expiration_timestamp < now_ts + min_mint_lock_window() as i64 {
            return Err(Error::InvoiceExpired);
        }
        local_recipients.push(local_recipient);

        // the mint event can only follow the current external-chain block, since the lock
        // commits to this operation's ID: commit that block to the transition so validators can
        // bound their event query to it (see RGB_METADATA_AFTER_BLOCK). The bound is the latest
        // finalized block: validators refuse events at or below it, and no reorg can re-include
        // the lock at or below a finalized block. The block is taken from an RPC endpoint checked
        // to be on the bridge's chain: one from another chain could bound the query above the
        // lock, making the mint unconfirmable
        let contract_id = ContractId::from_str(&asset_id).expect("invalid contract ID");
        let (evm_client, _) = self.checked_evm_client(&stock, contract_id)?;
        let after_block = evm_client.finalized_block_number()?;
        debug!(self.logger(), "External chain block bound: {after_block}");

        let asset_info = AssetInfo {
            contract_id,
            schema,
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
            main_transition: TypeOfTransition::Mint,
            beneficiaries_blinded: vec![],
            beneficiaries_witness: vec![],
            burn_reason: None,
            after_block: Some(after_block),
        };
        let receive_ids: Vec<String> = local_recipients
            .iter()
            .map(|lr| lr.recipient_id.clone())
            .collect();
        let transfer_dir = self.setup_transfer_directory(receive_ids)?;

        let begin_operation_data = self.prepare_single_asset_psbt(
            txn,
            transfer_info.clone(),
            transfer_dir,
            unspents,
            &input_unspents,
            &witness_recipients,
            fee_rate_checked,
            min_confirmations,
            Some(expiration_timestamp),
            Some(sender_transport_endpoint.clone()),
            Some(operation_secret_key.clone()),
            &mut stock,
            false,
        )?;

        let (_txid, transfer_dir, info_contents, fascia) =
            self.get_transfer_end_data(&begin_operation_data.psbt)?;

        self.gen_consignments(txn, &fascia, &info_contents.transfers, &transfer_dir)?;

        // post consignment(s) and optional media(s)
        let mut post_recipients = transfer_info.recipients.clone();
        let asset_transfer_dir = self.get_asset_transfer_dir(&transfer_dir, &asset_id);
        // The consignment is posted now, before the TX is signed, so the recipient can verify the
        // mint operation (its OpId, seal and amount) before locking funds on the external chain.
        // Since the signed TX doesn't exist yet, the operation cannot be registered on the sender
        // proxy (ack.prepare): a mint recipient doesn't ACK, and the TX is broadcast by
        // mint_end. Once the PSBT is signed before the lock (bridge v2), this can register the
        // operation and follow the regular ACK flow, as send does.
        let seed: [u8; 32] =
            operation_secret_key
                .clone()
                .try_into()
                .map_err(|_| Error::Internal {
                    details: s!("invalid operation secret key"),
                })?;
        let sender_sk = ed25519_from_seed(&seed);
        let sender_pubkey_hex = hex::encode(sender_sk.verifying_key().to_bytes());
        let consignment_hash =
            hash_file(&self.get_send_consignment_path_impl(&asset_transfer_dir))?;
        self.post_transfer_data(
            &mut post_recipients,
            asset_transfer_dir,
            self.get_asset_medias(txn, asset.media_idx, None)?,
            &sender_sk,
            &sender_pubkey_hex,
            Some(&sender_transport_endpoint),
            &consignment_hash,
            false,
        )?;

        Ok(begin_operation_data)
    }

    /// The details of the mint the given batch transfer data and fascia carry, as its sender:
    /// what its lock on the external chain has to match.
    fn sent_mint(
        &self,
        stock: &RgbStock<'_>,
        info_contents: &InfoBatchTransfer,
        fascia: &Fascia,
    ) -> Result<ReceivedMint, Error> {
        let transfer_info = info_contents
            .transfers
            .values()
            .find(|t| t.main_transition == TypeOfTransition::Mint)
            .ok_or_else(|| Error::Internal {
                details: s!("not a mint transfer"),
            })?;
        let amount = transfer_info
            .recipients
            .iter()
            .map(|r| r.assignment.main_amount())
            .sum::<u64>();
        let after_block = transfer_info.after_block.ok_or_else(|| Error::Internal {
            details: s!("mint transfer is missing the external chain block"),
        })?;
        let contract_id = transfer_info.asset_info.contract_id;
        let opid = fascia
            .bundles()
            .iter()
            .filter(|(id, _)| **id == contract_id)
            .flat_map(|(_, bundle)| bundle.known_transitions.iter())
            .find(|kt| kt.transition.transition_type == TS_MINT)
            .map(|kt| kt.opid)
            .ok_or_else(|| Error::Internal {
                details: s!("mint transition missing from fascia"),
            })?;
        let BridgeLocation::Evm(EvmContract { chain_id, address }) = stock
            .contract_wrapper::<BridgedFungibleAsset>(contract_id)?
            .bridge_location();
        Ok(ReceivedMint {
            opid: opid.to_string(),
            amount,
            after_block,
            evm_chain_id: chain_id,
            bridge_contract: address.to_string(),
        })
    }

    /// The deadline for the lock of the mint the given batch transfer data carries, as its
    /// sender: the expiration of the recipient's invoice, the one the recipient fails the transfer
    /// by (when no lock was included by then), falling back to the transfer expiration.
    fn sent_mint_deadline(info_contents: &InfoBatchTransfer) -> Option<i64> {
        info_contents
            .transfers
            .values()
            .find(|t| t.main_transition == TypeOfTransition::Mint)
            .and_then(|t| t.recipients.first())
            .and_then(|r| r.invoice_expiration)
            .or(info_contents.expiration_timestamp)
    }

    /// Whether the lock of `mint` was included on the external chain by `expiration`: `None` if
    /// not, otherwise whether it's finalized.
    fn mint_lock_included_by(
        &self,
        mint: &ReceivedMint,
        expiration: i64,
    ) -> Result<Option<bool>, Error> {
        let evm_client = self.checked_evm_client_for_chain(mint.evm_chain_id)?;
        let address = EvmAddress::from_str(&mint.bridge_contract).map_err(|e| Error::Internal {
            details: format!("invalid bridge contract address: {e}"),
        })?;
        let opid = OpId::from_str(&mint.opid).map_err(|e| Error::Internal {
            details: format!("invalid mint operation ID: {e}"),
        })?;
        Ok(evm_client
            .funds_in_inclusion(&address, &opid, mint.amount, mint.after_block)?
            .filter(|inclusion| inclusion.timestamp as i64 <= expiration)
            .map(|inclusion| inclusion.finalized))
    }

    /// Whether an expired mint is still waited for, for its lock to be finalized: the lock was
    /// included on the external chain by the expiration, which the deadline applies to, and the
    /// finality grace (see [`MINT_FINALITY_GRACE_SECS`]) hasn't passed yet.
    fn mint_in_finality_grace(&self, mint: &ReceivedMint, expiration: i64, now: i64) -> bool {
        if now > expiration + MINT_FINALITY_GRACE_SECS {
            return false;
        }
        match self.mint_lock_included_by(mint, expiration) {
            Ok(included) => included.is_some(),
            // the lock cannot be looked up (e.g. the EVM RPC endpoint is unreachable): keep
            // waiting rather than failing a mint that may have been paid, until the grace passes
            Err(e) => {
                warn!(
                    self.logger(),
                    "Cannot check the lock of expired mint {}: {e}", mint.opid
                );
                true
            }
        }
    }

    /// The mint the given batch transfer carries, if any, as its recipient (once checked) or as
    /// its sender (until mint_end), along with the deadline for its lock: the expiration of the
    /// recipient's invoice (see [`Self::sent_mint_deadline`]).
    fn batch_transfer_mint(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
    ) -> Result<Option<(ReceivedMint, i64)>, Error> {
        if batch_transfer.incoming {
            let (_, transfer) = batch_transfer
                .get_incoming_transfer(&db_data.asset_transfers, &db_data.transfers)?;
            let (Some(recipient_id), Some(expiration)) =
                (&transfer.recipient_id, batch_transfer.expiration)
            else {
                return Ok(None);
            };
            return Ok(self
                .read_received_mint(recipient_id)?
                .map(|mint| (mint, expiration)));
        }
        if !Self::is_pending_mint(batch_transfer, db_data) {
            return Ok(None);
        }
        let txid = batch_transfer
            .txid
            .as_ref()
            .expect("set for a pending mint");
        let transfer_dir = self.get_transfer_dir(txid);
        let info_contents: InfoBatchTransfer =
            serde_json::from_str(&fs::read_to_string(transfer_dir.join(TRANSFER_DATA_FILE))?)
                .map_err(InternalError::from)?;
        let fascia = read_fascia(&transfer_dir.join(FASCIA_FILE))?;
        let stock = self.rgb_stock(txn)?;
        let Some(deadline) = Self::sent_mint_deadline(&info_contents) else {
            return Ok(None);
        };
        Ok(Some((
            self.sent_mint(&stock, &info_contents, &fascia)?,
            deadline,
        )))
    }

    /// Whether the given batch transfer has expired. A mint expires by the deadline for its lock
    /// (see [`Self::batch_transfer_mint`]), unless its lock was included by then, in which case it
    /// expires once the finality grace has passed (see [`Self::mint_in_finality_grace`]).
    fn is_expired_with_mint_grace(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        db_data: &DbData,
        now: i64,
    ) -> Result<bool, Error> {
        Ok(
            match self.batch_transfer_mint(txn, batch_transfer, db_data)? {
                Some((mint, deadline)) => {
                    now > deadline && !self.mint_in_finality_grace(&mint, deadline, now)
                }
                None => incoming_expired(batch_transfer, now),
            },
        )
    }

    // check the external chain carries a finalized lock event matching the mint in the given
    // fascia, erroring with MintEventNotFound otherwise
    fn check_mint_event(
        &self,
        txn: &DbTxn,
        info_contents: &InfoBatchTransfer,
        fascia: &Fascia,
    ) -> Result<(), Error> {
        let stock = self.rgb_stock(txn)?;
        let mint = self.sent_mint(&stock, info_contents, fascia)?;
        let contract_id = info_contents
            .transfers
            .values()
            .find(|t| t.main_transition == TypeOfTransition::Mint)
            .expect("checked by sent_mint")
            .asset_info
            .contract_id;
        let (evm_client, bridge_location) = self.checked_evm_client(&stock, contract_id)?;
        debug!(
            self.logger(),
            "Checking for the mint event of operation {} at bridge location {:?}",
            mint.opid,
            bridge_location
        );
        let anchor = ExternalAnchor::MintEvent {
            opid: OpId::from_str(&mint.opid).expect("valid opid"),
            amount: mint.amount,
            after_block: mint.after_block,
        };
        if !evm_client.is_confirmed(&bridge_location, &anchor)? {
            return Err(Error::MintEventNotFound { opid: mint.opid });
        }
        Ok(())
    }

    fn mint_end_impl(&mut self, txn: &DbTxn, signed_psbt: &Psbt) -> Result<OperationResult, Error> {
        // a mint ended again (e.g. retried after a crash) is completed only while it's still
        // initiated: one that has failed (which released its inputs) or progressed must not be
        // broadcast or brought back to waiting for confirmations
        self.check_end_initiated(txn, signed_psbt)?;
        let (txid, transfer_dir, info_contents, fascia) =
            self.get_transfer_end_data(signed_psbt)?;

        // the data must be of this operation: another one would be completed skipping its checks
        // and steps
        if !info_contents.is_operation(&TypeOfTransition::Mint) {
            return Err(Error::WrongOperation { txid });
        }

        // don't broadcast a mint that has expired: past its expiration the recipient is allowed
        // to fail the transfer (and reuse its seal), so broadcasting now could allocate the minted
        // assets to a seal the recipient has already given up on. Unlike a send, no ACK gates the
        // broadcast, so this is the only place the sender can honor the deadline. The deadline is
        // the recipient's invoice expiration, the one the recipient knows, and applies to the
        // inclusion of the lock on the external chain: a lock included by then is waited for by
        // the recipient past it, until the finality grace has passed (see
        // MINT_FINALITY_GRACE_SECS), so the mint can be broadcast until then. As the inclusion is
        // judged by the timestamp of the external chain block, which both parties read, their
        // clocks don't need to agree
        let now = now().unix_timestamp();
        let deadline = Self::sent_mint_deadline(&info_contents).unwrap_or(now);
        if deadline < now {
            if now > deadline + MINT_FINALITY_GRACE_SECS {
                return Err(Error::TransferExpired);
            }
            let stock = self.rgb_stock(txn)?;
            let mint = self.sent_mint(&stock, &info_contents, &fascia)?;
            if self.mint_lock_included_by(&mint, deadline)?.is_none() {
                return Err(Error::TransferExpired);
            }
        }

        self.check_mint_event(txn, &info_contents, &fascia)?;

        // the consignments were generated (and posted) by mint_begin
        let consignment_hashes = self.consignment_hashes(&transfer_dir, &info_contents)?;
        let batch_transfer_idx = self.finalize_transfer_end(
            txn,
            txid.clone(),
            signed_psbt,
            &info_contents,
            TransferStatus::WaitingConfirmations,
            &consignment_hashes,
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
#[cfg(feature = "_online")]
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
    /// Transfers are eligible if they remain in a fallible status after a `refresh` has been
    /// performed. A transfer in status [`TransferStatus::WaitingBroadcast`] is an exception: it can
    /// only be failed once it has expired, since the TX may still be broadcast before then. For a
    /// transfer ACKed via the sender proxy this is once its broadcast deadline (the operation
    /// expiration plus the sender proxy retention grace) and a further confirmation margin have
    /// passed.
    ///
    /// An outgoing transfer is considered expired only once its expiration has passed on the
    /// sender proxy's clock as well (a fixed clock skew margin is added), as the proxy may release
    /// its TX until then. A mint not yet ended is the exception, as it is never registered on the
    /// sender proxy: it is considered expired as soon as the deadline for its lock (the expiration
    /// of the recipient's invoice) has passed. A mint (sent or received) whose lock was included on
    /// the external chain by the deadline is instead considered expired only once 24 hours have
    /// passed since the deadline, leaving time for the lock to be finalized. When a
    /// `batch_transfer_idx` is provided, the signed TX of an outgoing transfer is revoked on the
    /// sender proxy before failing it: if a recipient already obtained it, the transfer cannot be
    /// failed. If the sender proxy no longer knows the operation (e.g. it lost its data), the
    /// transfer can only be failed once its broadcast deadline (its expiration plus the sender
    /// proxy retention grace) has passed on the proxy's clock too, and only if its TX is not found
    /// on-chain: this accepts the residual risk of a recipient that obtained the TX before the data
    /// loss broadcasting it later, which would spend the inputs of the failed transfer. Ruling that
    /// out requires invalidating the TX by spending one of its inputs, which this method does not
    /// do.
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
        self.persist_and_commit(txn)?;
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
        self.persist_and_commit(txn)?;
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
    ///
    /// Transfers created with the previous proxy protocol, which is not backward compatible, are
    /// failed since they can no longer be completed, unless their TX is found on-chain (in which
    /// case they proceed to wait for confirmations). A transfer waiting for the counterparty's
    /// broadcast is only failed once expired, as the counterparty may still broadcast until then.
    ///
    /// An outgoing transfer whose sender proxy no longer knows the operation, or serves an ACK
    /// with an invalid signature (the refresh then reports [`Error::InvalidContentSignature`] for
    /// it), is failed once its broadcast deadline (its expiration plus the sender proxy retention
    /// grace) has passed on the proxy's clock as well (a fixed clock skew margin is added), unless
    /// its TX is found on-chain first, in which case it proceeds to wait for confirmations. While
    /// waiting, the refresh reports [`Error::OperationNotFound`] for a transfer the proxy no longer
    /// knows. This accepts the residual risk of a recipient that obtained the TX broadcasting it
    /// later.
    ///
    /// An incoming transfer whose signed TX is released via the sender proxy is NACKed if the
    /// sender proxy retention grace is shorter than a minimum (1 day). Its TX is broadcast as soon
    /// as it's obtained and again, if missing, until the broadcast deadline; if the TX is not
    /// confirmed once the broadcast deadline and a further confirmation margin (1 day) have passed,
    /// the transfer is failed.
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
        self.persist_and_commit(txn)?;
        info!(self.logger(), "Refresh completed");
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxy_error(code: i64) -> JsonRpcError {
        JsonRpcError {
            code,
            message: s!("msg"),
            data: None,
        }
    }

    // the sender proxy's ack.clean response decides whether a deliberate fail can proceed
    #[test]
    fn clean_ack_outcome_decisions() {
        // the signed TXs were revoked
        assert_matches!(clean_ack_outcome(None).unwrap(), CleanAckOutcome::Revoked);
        // the proxy never had or no longer has the operation
        assert_matches!(
            clean_ack_outcome(Some(proxy_error(-104))).unwrap(),
            CleanAckOutcome::Unknown
        );
        // all the recipients already ACKed (-107): the TX may be in a recipient's hands
        assert_matches!(
            clean_ack_outcome(Some(proxy_error(-107))).unwrap(),
            CleanAckOutcome::Released
        );
        // any other response is surfaced rather than read as "nothing to revoke": a rejected
        // signature, or an error this version doesn't know (e.g. -500 from a proxy with a
        // different error set)
        assert_matches!(
            clean_ack_outcome(Some(proxy_error(-502))).unwrap_err(),
            Error::InvalidEndpointAuthSignature
        );
        assert_matches!(
            clean_ack_outcome(Some(proxy_error(-500))).unwrap_err(),
            Error::Proxy { .. }
        );
    }

    // only a lost operation is tolerated when polling ack.get
    #[test]
    fn check_get_ack_error_decisions() {
        assert!(check_get_ack_error(proxy_error(-104)).is_ok());
        assert_matches!(
            check_get_ack_error(proxy_error(-502)).unwrap_err(),
            Error::InvalidEndpointAuthSignature
        );
        assert_matches!(
            check_get_ack_error(proxy_error(-500)).unwrap_err(),
            Error::Proxy { .. }
        );
    }
}
