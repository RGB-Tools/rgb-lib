//! RGB singlesig wallet module.
//!
//! This module defines the methods of the [`Wallet`] structure.

#[cfg(any(feature = "electrum", feature = "esplora"))]
use super::offline::{
    SWAP_OFFER_FILE, SWAP_PROPOSAL_FILE, SWAP_REQUEST_FILE, SwapDirection, swap_build_psbt,
    swap_ensure_not_expired, swap_ensure_state_matches, swap_invalid, swap_load_state,
    swap_mpc_entropy, swap_random_blinding, swap_require_rgb_destination,
    swap_restore_input_metadata, swap_save_state, swap_side_rgb_output_cost, swap_validate_legs,
    swap_validate_proposal_psbt, swap_validate_proxy_url,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use super::online::{
    swap_accept_transfer_from_file, swap_color_rgb_leg, swap_emit_asset_history,
    swap_emit_consignments, swap_ensure_inputs_confirmed, swap_fetch_consignment_to_file,
    swap_finalize_psbt, swap_import_asset_history, swap_record_outgoing, swap_select_inputs,
    swap_sign_psbt, swap_stage_rgb_leg, swap_validate_fascia_received_leg,
    swap_validate_received_swap_leg,
};
use super::*;

/// Keys for the singlesig wallet.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct SinglesigKeys {
    /// Wallet account-level xPub for the vanilla side of the wallet
    pub account_xpub_vanilla: String,
    /// Wallet account-level xPub for the colored side of the wallet
    pub account_xpub_colored: String,
    /// Keychain index for the vanilla-side of the wallet (default: 0)
    #[serde(deserialize_with = "from_str_or_number_optional")]
    pub vanilla_keychain: Option<u8>,
    /// Wallet master fingerprint
    pub master_fingerprint: String,
    /// Wallet mnemonic phrase
    pub mnemonic: Option<String>,
    /// Witness version these keys were derived with
    #[serde(default)]
    pub witness_version: WitnessVersion,
}

impl SinglesigKeys {
    pub(crate) fn build_descriptors(
        &self,
        bitcoin_network: &BitcoinNetwork,
    ) -> Result<(WalletDescriptors, bool), Error> {
        let network_kind = bitcoin_network.network_kind();
        let xpub_rgb = str_to_xpub(&self.account_xpub_colored, &network_kind)?;
        let xpub_btc = str_to_xpub(&self.account_xpub_vanilla, &network_kind)?;
        Ok(if let Some(mnemonic) = &self.mnemonic {
            let descs = get_descriptors(
                bitcoin_network,
                mnemonic,
                self.vanilla_keychain,
                &xpub_btc,
                &xpub_rgb,
                self.witness_version,
            )?;
            // check master fingerprint derived from mnemonic matches provided one
            let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)?;
            let master_xprv = Xpriv::new_master(*bitcoin_network, &mnemonic.to_seed("")).unwrap();
            let master_xpub = Xpub::from_priv(&Secp256k1::new(), &master_xprv);
            let master_fp = master_xpub.fingerprint();
            if master_fp
                != Fingerprint::from_str(&self.master_fingerprint)
                    .map_err(|_| Error::InvalidFingerprint)?
            {
                return Err(Error::FingerprintMismatch);
            }
            (descs, false)
        } else {
            let descs = get_descriptors_from_xpubs(
                bitcoin_network,
                &self.master_fingerprint,
                &xpub_rgb,
                &xpub_btc,
                self.vanilla_keychain,
                self.witness_version,
            )?;
            (descs, true)
        })
    }

    /// Create a new [`SinglesigKeys`] from a [`Keys`] object.
    pub fn from_keys(keys: &Keys, vanilla_keychain: Option<u8>) -> Self {
        Self {
            account_xpub_vanilla: keys.account_xpub_vanilla.clone(),
            account_xpub_colored: keys.account_xpub_colored.clone(),
            vanilla_keychain,
            master_fingerprint: keys.master_fingerprint.clone(),
            mnemonic: Some(keys.mnemonic.clone()),
            witness_version: keys.witness_version,
        }
    }

    /// Create a new [`SinglesigKeys`] from a [`Keys`] object without a mnemonic.
    pub fn from_keys_no_mnemonic(keys: &Keys, vanilla_keychain: Option<u8>) -> Self {
        Self {
            account_xpub_vanilla: keys.account_xpub_vanilla.clone(),
            account_xpub_colored: keys.account_xpub_colored.clone(),
            vanilla_keychain,
            master_fingerprint: keys.master_fingerprint.clone(),
            mnemonic: None,
            witness_version: keys.witness_version,
        }
    }
}

/// An RGB singlesig wallet.
///
/// Can be obtained with the [`Wallet::new`] method.
pub struct Wallet {
    pub(crate) internals: WalletInternals,
    pub(crate) keys: SinglesigKeys,
}

impl WalletCore for Wallet {
    fn internals(&self) -> &WalletInternals {
        &self.internals
    }

    fn internals_mut(&mut self) -> &mut WalletInternals {
        &mut self.internals
    }
}

impl WalletBackup for Wallet {}

impl WalletOffline for Wallet {}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl WalletOnline for Wallet {
    fn wallet_specific_consistency_checks(&mut self, txn: &DbTxn) -> Result<(), Error> {
        self.sync_wallet(
            txn,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FullScan,
            },
            false,
        )?;
        self.sync_wallet(
            txn,
            SyncOptions {
                keychain: SyncKeychain::Vanilla {
                    lookback: self.vanilla_sync_lookback(),
                },
                strategy: SyncStrategy::FullScan,
            },
            false,
        )?;
        let bdk_utxos: Vec<String> = self
            .bdk_wallet()
            .list_unspent()
            .map(|u| u.outpoint.to_string())
            .collect();
        let bdk_utxos: HashSet<String> = HashSet::from_iter(bdk_utxos);
        let db_utxos: Vec<String> = txn
            .iter_txos()?
            .into_iter()
            .filter(|t| !t.spent && t.exists)
            .map(|u| u.outpoint().to_string())
            .collect();
        let db_utxos: HashSet<String> = HashSet::from_iter(db_utxos);
        let diff = db_utxos.difference(&bdk_utxos);
        if diff.clone().count() > 0 {
            return Err(Error::Inconsistency {
                details: format!("spent bitcoins with another wallet: {diff:?}"),
            });
        }
        Ok(())
    }
}

/// Common offline APIs of the wallet.
impl RgbWalletOpsOffline for Wallet {}

/// Common online APIs of the wallet.
#[cfg(any(feature = "electrum", feature = "esplora"))]
impl RgbWalletOpsOnline for Wallet {}

/// Offline APIs of the wallet.
impl Wallet {
    /// Create a new RGB singlesig wallet based on the provided [`WalletData`] and
    /// [`SinglesigKeys`].
    pub fn new(wallet_data: WalletData, keys: SinglesigKeys) -> Result<Self, Error> {
        let wdata = wallet_data.clone();

        // wallet keys
        let (descs, watch_only) = keys.build_descriptors(&wdata.bitcoin_network)?;

        // wallet directory and file logging setup
        let (wallet_dir, logger, _logger_guard) =
            setup_new_wallet(&wallet_data, &keys.master_fingerprint)?;

        // setup the BDK wallet
        let (bdk_wallet, bdk_database) = setup_bdk(
            &wdata,
            &wallet_dir,
            descs.colored,
            descs.vanilla,
            watch_only,
            BdkNetwork::from(wdata.bitcoin_network),
        )?;

        // setup RGB
        setup_rgb(&wallet_dir, wdata.supported_schemas, wdata.bitcoin_network)?;

        // setup rgb-lib DB
        let database = setup_db(&wallet_dir)?;

        info!(logger, "New wallet completed");
        Ok(Self {
            internals: WalletInternals {
                wallet_data,
                logger,
                _logger_guard,
                database: Arc::new(database),
                wallet_dir,
                bdk_wallet,
                bdk_database,
                #[cfg(any(feature = "electrum", feature = "esplora"))]
                online_data: None,
            },
            keys,
        })
    }

    /// Return the bitcoin keys of the wallet.
    pub fn get_keys(&self) -> SinglesigKeys {
        self.keys.clone()
    }

    /// Return the descriptors of the wallet.
    pub fn get_descriptors(&self) -> WalletDescriptors {
        self.keys
            .build_descriptors(&self.internals.wallet_data.bitcoin_network)
            .expect("already succeeded at wallet creation")
            .0
    }

    pub(crate) fn sign_psbt_impl(
        &self,
        psbt: &mut Psbt,
        sign_options: Option<SignOptions>,
    ) -> Result<(), Error> {
        let sign_options = sign_options.unwrap_or_default();
        self.bdk_wallet()
            .sign(psbt, sign_options)
            .map_err(InternalError::from)?;
        Ok(())
    }

    /// Sign a PSBT, optionally providing BDK sign options.
    pub fn sign_psbt(
        &self,
        unsigned_psbt: String,
        sign_options: Option<SignOptions>,
    ) -> Result<String, Error> {
        info!(self.logger(), "Signing PSBT...");
        let mut psbt = Psbt::from_str(&unsigned_psbt)?;
        self.sign_psbt_impl(&mut psbt, sign_options)?;
        info!(self.logger(), "Sign PSBT completed");
        Ok(psbt.to_string())
    }

    /// Return a new Bitcoin address from the vanilla wallet.
    pub fn get_address(&mut self) -> Result<String, Error> {
        info!(self.logger(), "Getting address...");
        let address = self.get_new_addresses(KeychainKind::Internal, 1)?;
        let txn = self.database().begin_transaction()?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Get address completed");
        Ok(address.to_string())
    }

    /// List the pending vanilla transactions that have reserved TXOs in the wallet.
    ///
    /// A vanilla transaction becomes "pending" when the caller invokes a vanilla `_begin` method
    /// (e.g. [`send_btc_begin`](Wallet::send_btc_begin)) with `dry_run = false`. The reserved
    /// TXOs are freed when the matching `_end` method is called or via
    /// [`abort_pending_vanilla_tx`](Wallet::abort_pending_vanilla_tx).
    pub fn list_pending_vanilla_txs(&self) -> Result<Vec<PendingVanillaTx>, Error> {
        info!(self.logger(), "Listing pending vanilla TXs...");
        let txn = self.database().begin_transaction()?;
        let reserved_idxs: Vec<i32> = txn
            .iter_reserved_txos()?
            .into_iter()
            .filter_map(|r| r.reserved_for)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let result = if reserved_idxs.is_empty() {
            vec![]
        } else {
            txn.get_wallet_transactions_by_idxs(&reserved_idxs)?
                .into_iter()
                .map(|wt| PendingVanillaTx {
                    txid: wt.txid,
                    r#type: wt.r#type,
                })
                .collect()
        };
        txn.commit()?;
        info!(self.logger(), "List pending vanilla TXs completed");
        Ok(result)
    }

    /// Abort a pending vanilla transaction, releasing the TXOs it reserved.
    ///
    /// Errors with [`Error::CannotAbortPendingVanillaTx`] if no pending vanilla transaction with
    /// the given `txid` is found (e.g. because it was never created by the wallet, was already
    /// aborted or has been broadcast).
    pub fn abort_pending_vanilla_tx(&self, txid: String) -> Result<(), Error> {
        info!(self.logger(), "Aborting pending vanilla TX {}...", txid);
        let txn = self.database().begin_transaction()?;
        let (wt, reservations) = txn
            .get_wallet_transaction_with_reserved_txos_by_txid(&txid)?
            .ok_or(Error::CannotAbortPendingVanillaTx)?;
        if reservations.is_empty() {
            return Err(Error::CannotAbortPendingVanillaTx);
        }
        txn.del_wallet_transaction(wt.idx)?; // relies on cascade to delete reserved txos
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Abort pending vanilla TX completed");
        Ok(())
    }

    fn finalize_offline_issuance<T: IssuedAssetDetails>(
        &self,
        txn: &DbTxn,
        issue_data: &IssueData,
    ) -> Result<T, Error> {
        let mut runtime = self.rgb_runtime()?;
        let asset = self.import_and_save_contract(txn, issue_data, &mut runtime)?;
        T::from_issuance(txn, self, &asset, issue_data)
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
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetNIA, Error> {
        info!(self.logger(), "Issuing NIA...");
        let txn = self.database().begin_transaction()?;
        let issue_data = self.create_nia_contract(&txn, ticker, name, precision, amounts)?;
        let res = self.finalize_offline_issuance(&txn, &issue_data)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Issue asset NIA completed");
        Ok(res)
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
        ticker: String,
        name: String,
        details: Option<String>,
        precision: u8,
        media_file_path: Option<String>,
        attachments_file_paths: Vec<String>,
    ) -> Result<AssetUDA, Error> {
        info!(self.logger(), "Issuing UDA...");
        let txn = self.database().begin_transaction()?;
        let issue_data = self.create_uda_contract(
            &txn,
            ticker,
            name,
            details,
            precision,
            media_file_path,
            attachments_file_paths,
        )?;
        let res = self.finalize_offline_issuance(&txn, &issue_data)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Issue asset UDA completed");
        Ok(res)
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
        name: String,
        details: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, Error> {
        info!(self.logger(), "Issuing CFA...");
        let txn = self.database().begin_transaction()?;
        let issue_data =
            self.create_cfa_contract(&txn, name, details, precision, amounts, file_path)?;
        let res = self.finalize_offline_issuance(&txn, &issue_data)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Issue asset CFA completed");
        Ok(res)
    }

    /// Issue a new RGB IFA asset with the provided `ticker`, `name`, `precision`, `amounts` and
    /// `inflation_amounts`, then return it.
    ///
    /// At least 1 amount needs to be provided and the sum of all amounts cannot exceed the maximum
    /// `u64` value.
    ///
    /// If `amounts` contains more than 1 element, each one will be issued as a separate allocation
    /// for the same asset (on a separate UTXO that needs to be already available).
    ///
    /// The `inflation_amounts` can be empty. If provided the sum of its elements plus the sum of
    /// `amounts` cannot exceed the maximum `u64` value.
    pub fn issue_asset_ifa(
        &self,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
        inflation_amounts: Vec<u64>,
        reject_list_url: Option<String>,
    ) -> Result<AssetIFA, Error> {
        info!(self.logger(), "Issuing IFA...");
        let txn = self.database().begin_transaction()?;
        let issue_data = self.create_ifa_contract(
            &txn,
            ticker,
            name,
            precision,
            amounts,
            inflation_amounts,
            reject_list_url,
        )?;
        let res = self.finalize_offline_issuance(&txn, &issue_data)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Issue asset IFA completed");
        Ok(res)
    }

    /// Blind an UTXO to receive RGB assets and return the resulting [`ReceiveData`].
    ///
    /// An optional asset ID can be specified, which will be embedded in the invoice, resulting in
    /// the refusal of the transfer is the asset doesn't match.
    ///
    /// An optional amount can be specified, which will be embedded in the invoice. It will not be
    /// checked when accepting the transfer.
    ///
    /// An optional expiration UTC timestamp can be specified, which will set the expiration of the
    /// invoice and the transfer.
    ///
    /// Each endpoint in the provided `transport_endpoints` list will be used as RGB data exchange
    /// medium. The list needs to contain at least 1 endpoint and a maximum of 3. Strings
    /// specifying invalid endpoints and duplicate ones will cause an error to be raised. A valid
    /// endpoint string encodes an
    /// [`RgbTransport`](https://docs.rs/rgb-invoicing/latest/rgbinvoice/enum.RgbTransport.html).
    /// At the moment the only supported variant is JsonRpc (e.g. `rpc://127.0.0.1` or
    /// `rpcs://example.com`).
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    pub fn blind_receive(
        &mut self,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, Error> {
        info!(
            self.logger(),
            "Receiving via blinded UTXO for asset '{:?}' with expiration '{:?}'...",
            asset_id,
            expiration_timestamp,
        );
        let txn = self.database().begin_transaction()?;
        let receive_data_internal = self.create_receive_data(
            &txn,
            asset_id,
            assignment,
            expiration_timestamp.map(|t| t as i64),
            transport_endpoints,
            RecipientType::Blind,
        )?;
        let batch_transfer_idx =
            self.store_receive_transfer(&txn, &receive_data_internal, min_confirmations)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Blind receive completed");
        Ok(ReceiveData {
            invoice: receive_data_internal.invoice_string,
            recipient_id: receive_data_internal.recipient_id,
            expiration_timestamp: receive_data_internal.expiration_timestamp.map(|t| t as u64),
            batch_transfer_idx,
        })
    }

    /// Create an address to receive RGB assets and return the resulting [`ReceiveData`].
    ///
    /// An optional asset ID can be specified, which will be embedded in the invoice, resulting in
    /// the refusal of the transfer is the asset doesn't match.
    ///
    /// An optional amount can be specified, which will be embedded in the invoice. It will not be
    /// checked when accepting the transfer.
    ///
    /// An optional expiration UTC timestamp can be specified, which will set the expiration of the
    /// invoice and the transfer.
    ///
    /// Each endpoint in the provided `transport_endpoints` list will be used as RGB data exchange
    /// medium. The list needs to contain at least 1 endpoint and a maximum of 3. Strings
    /// specifying invalid endpoints and duplicate ones will cause an error to be raised. A valid
    /// endpoint string encodes an
    /// [`RgbTransport`](https://docs.rs/rgb-invoicing/latest/rgbinvoice/enum.RgbTransport.html).
    /// At the moment the only supported variant is JsonRpc (e.g. `rpc://127.0.0.1` or
    /// `rpcs://example.com`).
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    pub fn witness_receive(
        &mut self,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, Error> {
        info!(
            self.logger(),
            "Receiving via witness TX for asset '{:?}' with expiration '{:?}'...",
            asset_id,
            expiration_timestamp,
        );
        let txn = self.database().begin_transaction()?;
        let receive_data_internal = self.create_receive_data(
            &txn,
            asset_id,
            assignment,
            expiration_timestamp.map(|t| t as i64),
            transport_endpoints,
            RecipientType::Witness,
        )?;
        let batch_transfer_idx =
            self.store_receive_transfer(&txn, &receive_data_internal, min_confirmations)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Witness receive completed");
        Ok(ReceiveData {
            invoice: receive_data_internal.invoice_string,
            recipient_id: receive_data_internal.recipient_id,
            expiration_timestamp: receive_data_internal.expiration_timestamp.map(|t| t as u64),
            batch_transfer_idx,
        })
    }
}

/// Online APIs of the wallet.
#[cfg(any(feature = "electrum", feature = "esplora"))]
impl Wallet {
    pub(crate) fn watch_only(&self) -> bool {
        self.keys.mnemonic.is_none()
    }

    fn check_xprv(&self) -> Result<(), Error> {
        if self.watch_only() {
            error!(self.logger(), "Invalid operation for a watch only wallet");
            return Err(Error::WatchOnly);
        }
        Ok(())
    }

    /// Create new UTXOs.
    ///
    /// This calls [`create_utxos_begin`](Wallet::create_utxos_begin), signs the resulting PSBT and
    /// finally calls [`create_utxos_end`](Wallet::create_utxos_end).
    ///
    /// A wallet with private keys is required.
    pub fn create_utxos(
        &mut self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<u8, Error> {
        info!(self.logger(), "Creating UTXOs...");
        self.check_xprv()?;
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let mut psbt =
            self.create_utxos_begin_impl(&txn, up_to, num, size, fee_rate, skip_sync, true)?;
        self.sign_psbt_impl(&mut psbt, None)?;
        let res = self.create_utxos_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Create UTXOs completed");
        Ok(res)
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
    /// If `dry_run` is true, the wallet does not reserve the selected vanilla TXOs. The returned
    /// PSBT can still be signed and completed with
    /// [`create_utxos_end`](Wallet::create_utxos_end) but concurrent vanilla operations may try
    /// to spend the same inputs.
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`create_utxos_end`](Wallet::create_utxos_end) function.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed.
    pub fn create_utxos_begin(
        &mut self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
        dry_run: bool,
    ) -> Result<String, Error> {
        info!(self.logger(), "Creating UTXOs (begin)...");
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let res =
            self.create_utxos_begin_impl(&txn, up_to, num, size, fee_rate, skip_sync, dry_run)?;
        if !dry_run {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Create UTXOs (begin) completed");
        Ok(res.to_string())
    }

    /// Broadcast the provided PSBT to create new UTXOs.
    ///
    /// The provided PSBT, prepared with the [`create_utxos_begin`](Wallet::create_utxos_begin)
    /// function, needs to have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns the number of created UTXOs.
    pub fn create_utxos_end(&mut self, online: Online, signed_psbt: String) -> Result<u8, Error> {
        info!(self.logger(), "Creating UTXOs (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let txn = self.database().begin_transaction()?;
        let res = self.create_utxos_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Create UTXOs (end) completed");
        Ok(res)
    }

    /// Return the existing or freshly generated wallet [`Online`] data.
    ///
    /// See [`OnlineOptions`] for details on the available options.
    pub fn go_online(&mut self, online_options: OnlineOptions) -> Result<Online, Error> {
        info!(self.logger(), "Going online...");
        let online = self.go_online_impl(&online_options)?;
        info!(self.logger(), "Go online completed");
        Ok(online)
    }

    /// Send bitcoin funds to the provided address.
    ///
    /// This calls [`drain_to_begin`](Wallet::drain_to_begin), signs the resulting PSBT and finally
    /// calls [`drain_to_end`](Wallet::drain_to_end).
    ///
    /// A wallet with private keys is required.
    pub fn drain_to(
        &mut self,
        online: Online,
        address: String,
        fee_rate: u64,
    ) -> Result<String, Error> {
        info!(self.logger(), "Draining to '{}'...", address);
        self.check_xprv()?;
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let mut psbt = self.drain_to_begin_impl(&txn, address, fee_rate, true)?;
        self.sign_psbt_impl(&mut psbt, None)?;
        let tx = self.drain_to_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Drain completed");
        Ok(tx.compute_txid().to_string())
    }

    /// Prepare the PSBT to send all bitcoin funds to the provided `address` with the provided
    /// `fee_rate` (in sat/vB).
    ///
    /// <div class="warning">Warning: draining all funds is a destructive and irreversible
    /// operation, only do this if you know what you're doing! After draining the wallet will not
    /// be usable anymore.</div>
    ///
    /// If `dry_run` is true, the wallet does not reserve the selected vanilla TXOs. The returned
    /// PSBT can still be signed and completed with [`drain_to_end`](Wallet::drain_to_end) but
    /// concurrent vanilla operations may try to spend the same inputs.
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`drain_to_end`](Wallet::drain_to_end) function.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed.
    pub fn drain_to_begin(
        &mut self,
        online: Online,
        address: String,
        fee_rate: u64,
        dry_run: bool,
    ) -> Result<String, Error> {
        info!(self.logger(), "Draining (begin) to '{}'...", address);
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let psbt = self.drain_to_begin_impl(&txn, address, fee_rate, dry_run)?;
        if !dry_run {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Drain (begin) completed");
        Ok(psbt.to_string())
    }

    /// Broadcast the provided PSBT to send bitcoin funds.
    ///
    /// The provided PSBT, prepared with the [`drain_to_begin`](Wallet::drain_to_begin) function,
    /// needs to have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns the TXID of the transaction that's been broadcast.
    pub fn drain_to_end(&mut self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger(), "Draining (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let txn = self.database().begin_transaction()?;
        let tx = self.drain_to_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Drain (end) completed");
        Ok(tx.compute_txid().to_string())
    }

    /// Send RGB assets.
    ///
    /// This calls [`send_begin`](Wallet::send_begin), signs the resulting PSBT and finally calls
    /// [`send_end`](Wallet::send_end).
    ///
    /// A wallet with private keys is required.
    pub fn send(
        &mut self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        expiration_timestamp: Option<u64>,
    ) -> Result<OperationResult, Error> {
        info!(self.logger(), "Sending to: {:?}...", recipient_map);
        self.check_xprv()?;
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let mut begin_op_data = self.send_begin_impl(
            &txn,
            recipient_map,
            donation,
            fee_rate,
            min_confirmations,
            expiration_timestamp.map(|t| t as i64),
            true,
        )?;
        self.sign_psbt_impl(&mut begin_op_data.psbt, None)?;
        let res = self.send_end_impl(&txn, &begin_op_data.psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Send completed");
        Ok(res)
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
    /// An optional expiration UTC timestamp can be specified, which will set the expiration of the
    /// transfer.
    ///
    /// If `dry_run` is true, the wallet does not persist the transfer in
    /// [`TransferStatus::Initiated`]. The returned [`SendBeginResult::batch_transfer_idx`] is None
    /// in that case. The PSBT and on-disk transfer data under the wallet directory are still
    /// produced. [`send_end`](Wallet::send_end) can still complete the operation and will persist
    /// the transfer.
    ///
    /// This API requires to be online since it checks the validity and reachability of the
    /// transport endpoints.
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`send_end`](Wallet::send_end) function to complete the send operation.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed and operation details.
    pub fn send_begin(
        &mut self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        expiration_timestamp: Option<u64>,
        dry_run: bool,
    ) -> Result<SendBeginResult, Error> {
        info!(self.logger(), "Sending (begin) to: {:?}...", recipient_map);
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let begin_op_data = self.send_begin_impl(
            &txn,
            recipient_map,
            donation,
            fee_rate,
            min_confirmations,
            expiration_timestamp.map(|t| t as i64),
            dry_run,
        )?;
        if !dry_run {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Send (begin) completed");
        Ok(SendBeginResult {
            psbt: begin_op_data.psbt.to_string(),
            batch_transfer_idx: begin_op_data.batch_transfer_idx,
            details: SendDetails {
                fascia_path: begin_op_data
                    .transfer_dir
                    .join(FASCIA_FILE)
                    .to_string_lossy()
                    .to_string(),
                min_confirmations,
                entropy: begin_op_data.info_batch_transfer.entropy,
                is_donation: donation,
            },
        })
    }

    /// Complete the send operation by saving the PSBT to disk, POSTing consignments to the RGB
    /// proxy server, saving the transfer to DB and broadcasting the provided PSBT, if appropriate.
    ///
    /// The provided PSBT, prepared with the [`send_begin`](Wallet::send_begin) function, needs to
    /// have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a [`OperationResult`].
    pub fn send_end(
        &mut self,
        online: Online,
        signed_psbt: String,
    ) -> Result<OperationResult, Error> {
        info!(self.logger(), "Sending (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let txn = self.database().begin_transaction()?;
        let res = self.send_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Send (end) completed");
        Ok(res)
    }

    /// Send bitcoins using the vanilla wallet.
    ///
    /// This calls [`send_btc_begin`](Wallet::send_btc_begin), signs the resulting PSBT and finally
    /// calls [`send_btc_end`](Wallet::send_btc_end).
    ///
    /// A wallet with private keys and [`Online`] data are required.
    pub fn send_btc(
        &mut self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<String, Error> {
        info!(self.logger(), "Sending BTC...");
        self.check_xprv()?;
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let mut psbt =
            self.send_btc_begin_impl(&txn, address, amount, fee_rate, skip_sync, true)?;
        self.sign_psbt_impl(&mut psbt, None)?;
        let res = self.send_btc_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Send BTC completed");
        Ok(res)
    }

    /// Prepare the PSBT to send the specified `amount` of bitcoins (in sats) using the vanilla
    /// wallet to the specified Bitcoin `address` with the specified `fee_rate` (in sat/vB).
    ///
    /// If `dry_run` is true, the wallet does not reserve the selected vanilla TXOs. The returned
    /// PSBT can still be signed and completed with [`send_btc_end`](Wallet::send_btc_end) but
    /// concurrent vanilla operations may try to spend the same inputs.
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`send_btc_end`](Wallet::send_btc_end) function.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed.
    pub fn send_btc_begin(
        &mut self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
        dry_run: bool,
    ) -> Result<String, Error> {
        info!(self.logger(), "Sending BTC (begin)...");
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let res = self.send_btc_begin_impl(&txn, address, amount, fee_rate, skip_sync, dry_run)?;
        if !dry_run {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Send BTC (begin) completed");
        Ok(res.to_string())
    }

    /// Broadcast the provided PSBT to send bitcoins using the vanilla wallet.
    ///
    /// The provided PSBT, prepared with the [`send_btc_begin`](Wallet::send_btc_begin) function,
    /// needs to have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns the TXID of the broadcasted transaction.
    pub fn send_btc_end(&mut self, online: Online, signed_psbt: String) -> Result<String, Error> {
        info!(self.logger(), "Sending BTC (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let txn = self.database().begin_transaction()?;
        let res = self.send_btc_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Send BTC (end) completed");
        Ok(res)
    }

    /// Inflate RGB assets.
    ///
    /// This calls [`inflate_begin`](Wallet::inflate_begin), signs the resulting PSBT and finally
    /// calls [`inflate_end`](Wallet::inflate_end).
    ///
    /// A wallet with private keys is required.
    pub fn inflate(
        &mut self,
        online: Online,
        asset_id: String,
        inflation_amounts: Vec<u64>,
        fee_rate: u64,
        min_confirmations: u8,
    ) -> Result<OperationResult, Error> {
        info!(
            self.logger(),
            "Inflating amounts: {:?}...", inflation_amounts
        );
        self.check_xprv()?;
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let mut begin_op_data = self.inflate_begin_impl(
            &txn,
            asset_id,
            inflation_amounts,
            fee_rate,
            min_confirmations,
            true,
        )?;
        self.sign_psbt_impl(&mut begin_op_data.psbt, None)?;
        let res = self.inflate_end_impl(&txn, &begin_op_data.psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Inflate completed");
        Ok(res)
    }

    /// Prepare the PSBT to inflate RGB assets according to the given inflation amounts, with the
    /// provided `fee_rate` (in sat/vB).
    ///
    /// For every amount in `inflation_amounts` a new UTXO allocating the requested
    /// asset amount will be created. The sum of its elements plus the known circulating supply
    /// cannot exceed the maximum `u64` value.
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    ///
    /// If `dry_run` is true, the wallet does not persist the transfer in
    /// [`TransferStatus::Initiated`]. The returned [`InflateBeginResult::batch_transfer_idx`] is
    /// None in that case. The PSBT and on-disk transfer data under the wallet directory are still
    /// produced. [`inflate_end`](Wallet::inflate_end) can still complete the operation and will
    /// persist the transfer.
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`inflate_end`](Wallet::inflate_end) function for broadcasting.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed and operation details.
    pub fn inflate_begin(
        &mut self,
        online: Online,
        asset_id: String,
        inflation_amounts: Vec<u64>,
        fee_rate: u64,
        min_confirmations: u8,
        dry_run: bool,
    ) -> Result<InflateBeginResult, Error> {
        info!(
            self.logger(),
            "Inflating (begin) amounts: {:?}...", inflation_amounts
        );
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let begin_operation_data = self.inflate_begin_impl(
            &txn,
            asset_id,
            inflation_amounts,
            fee_rate,
            min_confirmations,
            dry_run,
        )?;
        if !dry_run {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Inflate (begin) completed");
        Ok(InflateBeginResult {
            psbt: begin_operation_data.psbt.to_string(),
            batch_transfer_idx: begin_operation_data.batch_transfer_idx,
            details: InflateDetails {
                fascia_path: begin_operation_data
                    .transfer_dir
                    .join(FASCIA_FILE)
                    .to_string_lossy()
                    .to_string(),
                min_confirmations,
                entropy: begin_operation_data.info_batch_transfer.entropy,
            },
        })
    }

    /// Complete the inflate operation by broadcasting the provided PSBT and saving the transfer to
    /// DB.
    ///
    /// The provided PSBT, prepared with the [`inflate_begin`](Wallet::inflate_begin) function,
    /// needs to have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a [`OperationResult`].
    pub fn inflate_end(
        &mut self,
        online: Online,
        signed_psbt: String,
    ) -> Result<OperationResult, Error> {
        info!(self.logger(), "Inflating (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let txn = self.database().begin_transaction()?;
        let res = self.inflate_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Inflate (end) completed");
        Ok(res)
    }

    /// Burn RGB assets.
    ///
    /// This calls [`burn_begin`](Wallet::burn_begin), signs the resulting PSBT and finally
    /// calls [`burn_end`](Wallet::burn_end).
    ///
    /// A wallet with private keys is required.
    pub fn burn(
        &mut self,
        online: Online,
        asset_id: String,
        amount: u64,
        fee_rate: u64,
        min_confirmations: u8,
    ) -> Result<OperationResult, Error> {
        info!(self.logger(), "Burning amount: {}...", amount);
        self.check_xprv()?;
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let mut begin_op_data =
            self.burn_begin_impl(&txn, asset_id, amount, fee_rate, min_confirmations, true)?;
        self.sign_psbt_impl(&mut begin_op_data.psbt, None)?;
        let res = self.burn_end_impl(&txn, &begin_op_data.psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Burn completed");
        Ok(res)
    }

    /// Prepare the PSBT to burn RGB assets according to the given amount, with the provided
    /// `fee_rate` (in sat/vB).
    ///
    /// The amount of assets to burn is specified by the `amount` parameter and cannot be zero.
    ///
    /// If `dry_run` is true, the wallet does not persist the transfer in
    /// [`TransferStatus::Initiated`]. The returned [`BurnBeginResult::batch_transfer_idx`] is None
    /// in that case. The PSBT and on-disk transfer data under the wallet directory are still
    /// produced. [`burn_end`](Wallet::burn_end) can still complete the operation and will persist
    /// the transfer.
    ///
    /// Signing of the returned PSBT needs to be carried out separately. The signed PSBT then needs
    /// to be fed to the [`burn_end`](Wallet::burn_end) function for broadcasting.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a PSBT ready to be signed and operation details.
    pub fn burn_begin(
        &mut self,
        online: Online,
        asset_id: String,
        amount: u64,
        fee_rate: u64,
        min_confirmations: u8,
        dry_run: bool,
    ) -> Result<BurnBeginResult, Error> {
        info!(self.logger(), "Burning (begin) amount: {}...", amount);
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let begin_operation_data =
            self.burn_begin_impl(&txn, asset_id, amount, fee_rate, min_confirmations, dry_run)?;
        if !dry_run {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Burn (begin) completed");
        Ok(BurnBeginResult {
            psbt: begin_operation_data.psbt.to_string(),
            batch_transfer_idx: begin_operation_data.batch_transfer_idx,
            details: BurnDetails {
                fascia_path: begin_operation_data
                    .transfer_dir
                    .join(FASCIA_FILE)
                    .to_string_lossy()
                    .to_string(),
                min_confirmations,
                entropy: begin_operation_data.info_batch_transfer.entropy,
            },
        })
    }

    /// Complete the burn operation by broadcasting the provided PSBT and saving the transfer to DB.
    ///
    /// The provided PSBT, prepared with the [`burn_begin`](Wallet::burn_begin) function, needs to
    /// have already been signed.
    ///
    /// This doesn't require the wallet to have private keys.
    ///
    /// Returns a [`OperationResult`].
    pub fn burn_end(
        &mut self,
        online: Online,
        signed_psbt: String,
    ) -> Result<OperationResult, Error> {
        info!(self.logger(), "Burning (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let txn = self.database().begin_transaction()?;
        let res = self.burn_end_impl(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Burn (end) completed");
        Ok(res)
    }

    // ─── On-chain swap ─────────────────────────────────────────────────────────

    /// Create a maker offer for an on-chain swap.
    ///
    /// The maker specifies what they give (`maker_gives`) and what they want in return
    /// (`maker_receives`). `network_fee_sat` is the total miner fee the taker will reserve in
    /// the swap transaction. `proxy_url` is required; consignments produced during the swap are
    /// posted there so the counterparty can fetch them, and it is also used to publish the current
    /// asset history so the taker can validate the asset before accepting.
    ///
    /// This method is **offline** — no network connection is required.
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn create_swap_offer(
        &mut self,
        maker_gives: OnchainSwapLeg,
        maker_receives: OnchainSwapLeg,
        network_fee_sat: u64,
        expiration_timestamp: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<OnchainSwapOffer, Error> {
        info!(self.logger(), "Creating on-chain swap offer...");
        let txn = self.database().begin_transaction()?;
        let offer = self.create_swap_offer_impl(
            &txn,
            maker_gives,
            maker_receives,
            network_fee_sat,
            expiration_timestamp,
            proxy_url,
        )?;
        swap_save_state(self.wallet_dir(), &offer.swap_id, SWAP_OFFER_FILE, &offer)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Create swap offer completed");
        Ok(offer)
    }

    /// Accept a maker offer as the taker and return the taker's request message.
    ///
    /// The taker validates the offer, selects their inputs, and derives the receive destination.
    /// The returned [`OnchainSwapRequest`] must be forwarded to the maker.
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn accept_swap_offer(
        &mut self,
        online: Online,
        offer: OnchainSwapOffer,
        min_confirmations: u8,
        skip_sync: bool,
    ) -> Result<OnchainSwapRequest, Error> {
        info!(self.logger(), "Accepting on-chain swap offer...");
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        swap_validate_legs(&offer.maker_gives, &offer.maker_receives)?;
        swap_validate_proxy_url(&offer.proxy_url)?;
        swap_ensure_not_expired(&offer)?;
        if offer.bitcoin_network != self.bitcoin_network() {
            return Err(Error::BitcoinNetworkMismatch);
        }
        let taker_gives = offer.maker_receives.clone();
        let taker_receives = offer.maker_gives.clone();
        let taker_inputs = swap_select_inputs(
            self,
            &txn,
            online,
            &taker_gives,
            swap_side_rgb_output_cost(&taker_receives, offer.rgb_output_sat)
                .checked_add(offer.network_fee_sat)
                .ok_or_else(|| swap_invalid("swap amounts overflow"))?,
            min_confirmations,
            skip_sync,
        )?;
        let (
            taker_btc_address,
            taker_rgb_recipient_id,
            taker_rgb_script_pubkey_hex,
            taker_rgb_blinding,
        ) = if matches!(taker_receives.kind, OnchainSwapLegKind::Btc) {
            // Use get_new_addresses directly (BDK-only, no DB) to avoid opening a second
            // connection while the outer transaction already holds the single pool connection.
            (
                Some(
                    self.get_new_addresses(KeychainKind::Internal, 1)?
                        .to_string(),
                ),
                None,
                None,
                None,
            )
        } else {
            let script = self
                .get_new_addresses(KeychainKind::External, 1)?
                .script_pubkey();
            let blinding = swap_random_blinding();
            (
                None,
                Some(recipient_id_from_script_buf(
                    script.clone(),
                    self.bitcoin_network(),
                )),
                Some(script.to_hex_string()),
                Some(blinding),
            )
        };
        let taker_change_script_pubkey_hex = self
            .get_new_addresses(KeychainKind::Internal, 1)?
            .script_pubkey()
            .to_hex_string();
        let request = OnchainSwapRequest {
            offer,
            taker_inputs,
            taker_btc_address,
            taker_rgb_recipient_id,
            taker_rgb_script_pubkey_hex,
            taker_rgb_blinding,
            taker_change_script_pubkey_hex,
        };
        swap_save_state(
            self.wallet_dir(),
            &request.offer.swap_id,
            SWAP_REQUEST_FILE,
            &request,
        )?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Accept swap offer completed");
        Ok(request)
    }

    /// Accept a taker request as the maker and return the maker's PSBT proposal.
    ///
    /// The maker validates the request, selects their inputs, builds the collaborative PSBT,
    /// colors any RGB leg they are sending, and partially signs the PSBT.
    /// The returned [`OnchainSwapProposal`] must be forwarded to the taker.
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn accept_swap_request(
        &mut self,
        online: Online,
        request: OnchainSwapRequest,
        min_confirmations: u8,
        skip_sync: bool,
    ) -> Result<OnchainSwapProposal, Error> {
        info!(self.logger(), "Accepting on-chain swap request...");
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let offer = request.offer.clone();
        let local_offer: OnchainSwapOffer =
            swap_load_state(self.wallet_dir(), &offer.swap_id, SWAP_OFFER_FILE)?;
        swap_ensure_state_matches(&local_offer, &offer, "offer")?;
        let direction = swap_validate_legs(&offer.maker_gives, &offer.maker_receives)?;
        swap_validate_proxy_url(&offer.proxy_url)?;
        swap_ensure_not_expired(&offer)?;
        if offer.bitcoin_network != self.bitcoin_network() {
            return Err(Error::BitcoinNetworkMismatch);
        }
        // Correctness: verify the taker's inputs are sufficiently confirmed before the maker
        // commits resources to building the PSBT.
        swap_ensure_inputs_confirmed(self, &request.taker_inputs, min_confirmations)?;
        swap_require_rgb_destination(
            &offer.maker_gives,
            &request.taker_rgb_script_pubkey_hex,
            &request.taker_rgb_blinding,
        )?;
        swap_require_rgb_destination(
            &offer.maker_receives,
            &offer.maker_rgb_script_pubkey_hex,
            &offer.maker_rgb_blinding,
        )?;
        let maker_inputs = swap_select_inputs(
            self,
            &txn,
            online,
            &offer.maker_gives,
            swap_side_rgb_output_cost(&offer.maker_receives, offer.rgb_output_sat),
            min_confirmations,
            skip_sync,
        )?;
        let maker_change_script_pubkey_hex = self
            .get_new_addresses(KeychainKind::Internal, 1)?
            .script_pubkey()
            .to_hex_string();
        let mut proposal = OnchainSwapProposal {
            request,
            maker_inputs,
            maker_change_script_pubkey_hex,
            psbt: String::new(),
            txid: String::new(),
            consignments: vec![],
            maker_history: None,
        };
        let (mut psbt, _maker_rgb_vout, taker_rgb_vout) = swap_build_psbt(&proposal)?;
        let mut consignments = vec![];
        let mut maker_history = None;
        if matches!(offer.maker_gives.kind, OnchainSwapLegKind::Rgb) {
            maker_history = swap_emit_asset_history(
                self,
                &offer.maker_gives,
                offer.proxy_url.as_deref(),
                &offer.swap_id,
            )?;
            let vout = taker_rgb_vout.ok_or_else(|| swap_invalid("missing taker RGB vout"))?;
            let blinding = proposal
                .request
                .taker_rgb_blinding
                .ok_or_else(|| swap_invalid("missing taker RGB blinding"))?;
            match direction {
                SwapDirection::RgbForBtc => {
                    let recipient_id = proposal
                        .request
                        .taker_rgb_recipient_id
                        .as_deref()
                        .ok_or_else(|| swap_invalid("missing taker RGB recipient ID"))?;
                    consignments = swap_color_rgb_leg(
                        &txn,
                        self,
                        &mut psbt,
                        &offer.maker_gives,
                        recipient_id,
                        vout,
                        blinding,
                        offer.proxy_url.as_deref(),
                        &offer.swap_id,
                    )?;
                }
                SwapDirection::RgbForRgb => {
                    swap_stage_rgb_leg(self, &mut psbt, &offer.maker_gives, vout, blinding)?;
                }
                SwapDirection::BtcForRgb => {
                    unreachable!("BTC-for-RGB cannot reach maker_gives==Rgb branch")
                }
            }
            let inputs = proposal
                .maker_inputs
                .iter()
                .chain(proposal.request.taker_inputs.iter())
                .cloned()
                .collect::<Vec<_>>();
            swap_restore_input_metadata(&mut psbt, &inputs)?;
        }
        if matches!(direction, SwapDirection::RgbForBtc) {
            swap_sign_psbt(self, &mut psbt)?;
        }
        proposal.txid = psbt.unsigned_tx.compute_txid().to_string();
        proposal.psbt = psbt.to_string();
        proposal.consignments = consignments;
        proposal.maker_history = maker_history;
        swap_save_state(
            self.wallet_dir(),
            &offer.swap_id,
            SWAP_PROPOSAL_FILE,
            &proposal,
        )?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Accept swap request completed");
        Ok(proposal)
    }

    /// Complete a maker proposal as the taker.
    ///
    /// The taker validates the PSBT, colors any RGB leg they are sending, signs the PSBT, and
    /// attempts to finalize it. The returned [`OnchainSwapCompletion`] must be forwarded to the
    /// maker (who calls [`process_swap_completion`](Wallet::process_swap_completion) before
    /// broadcasting) or broadcast directly for single-RGB swaps.
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn complete_swap_proposal(
        &mut self,
        online: Online,
        proposal: OnchainSwapProposal,
        min_confirmations: u8,
        skip_sync: bool,
    ) -> Result<OnchainSwapCompletion, Error> {
        info!(self.logger(), "Completing on-chain swap proposal...");
        self.check_online(online)?;
        let txn = self.database().begin_transaction()?;
        let offer = &proposal.request.offer;
        let local_request: OnchainSwapRequest =
            swap_load_state(self.wallet_dir(), &offer.swap_id, SWAP_REQUEST_FILE)?;
        swap_ensure_state_matches(&local_request, &proposal.request, "request")?;
        let direction = swap_validate_legs(&offer.maker_gives, &offer.maker_receives)?;
        swap_validate_proxy_url(&offer.proxy_url)?;
        swap_ensure_not_expired(offer)?;
        if offer.bitcoin_network != self.bitcoin_network() {
            return Err(Error::BitcoinNetworkMismatch);
        }
        let mut psbt = Psbt::from_str(&proposal.psbt)?;
        swap_validate_proposal_psbt(&proposal, &psbt)?;
        if proposal.txid != psbt.unsigned_tx.compute_txid().to_string() {
            return Err(swap_invalid("swap proposal txid mismatch"));
        }
        let mut consignments = proposal.consignments.clone();
        let mut taker_history = None;
        let (_expected_psbt, maker_rgb_vout, taker_rgb_vout) = swap_build_psbt(&proposal)?;
        let all_inputs = proposal
            .maker_inputs
            .iter()
            .chain(proposal.request.taker_inputs.iter())
            .cloned()
            .collect::<Vec<_>>();
        swap_restore_input_metadata(&mut psbt, &all_inputs)?;
        match direction {
            SwapDirection::BtcForRgb => {
                let vout = maker_rgb_vout.ok_or_else(|| swap_invalid("missing maker RGB vout"))?;
                let blinding = offer
                    .maker_rgb_blinding
                    .ok_or_else(|| swap_invalid("missing maker RGB blinding"))?;
                let recipient_id = offer
                    .maker_rgb_recipient_id
                    .as_deref()
                    .ok_or_else(|| swap_invalid("missing maker RGB recipient ID"))?;
                consignments.extend(swap_color_rgb_leg(
                    &txn,
                    self,
                    &mut psbt,
                    &offer.maker_receives,
                    recipient_id,
                    vout,
                    blinding,
                    offer.proxy_url.as_deref(),
                    &offer.swap_id,
                )?);
                swap_restore_input_metadata(&mut psbt, &all_inputs)?;
            }
            SwapDirection::RgbForBtc => {
                let vout = taker_rgb_vout.ok_or_else(|| swap_invalid("missing taker RGB vout"))?;
                let blinding = proposal
                    .request
                    .taker_rgb_blinding
                    .ok_or_else(|| swap_invalid("missing taker RGB blinding"))?;
                let recipient_id = proposal
                    .request
                    .taker_rgb_recipient_id
                    .as_deref()
                    .ok_or_else(|| swap_invalid("missing taker RGB recipient ID"))?;
                swap_validate_received_swap_leg(
                    self,
                    &proposal.consignments,
                    &offer.maker_gives,
                    &proposal.txid,
                    vout,
                    blinding,
                    recipient_id,
                )?;
            }
            SwapDirection::RgbForRgb => {
                let maker_history = proposal
                    .maker_history
                    .as_ref()
                    .ok_or_else(|| swap_invalid("missing maker asset history"))?;
                swap_import_asset_history(self, &txn, maker_history)?;
                taker_history = swap_emit_asset_history(
                    self,
                    &offer.maker_receives,
                    offer.proxy_url.as_deref(),
                    &offer.swap_id,
                )?;
                let maker_recv_vout =
                    maker_rgb_vout.ok_or_else(|| swap_invalid("missing maker RGB vout"))?;
                let maker_recv_blinding = offer
                    .maker_rgb_blinding
                    .ok_or_else(|| swap_invalid("missing maker RGB blinding"))?;
                let taker_beneficiaries = swap_stage_rgb_leg(
                    self,
                    &mut psbt,
                    &offer.maker_receives,
                    maker_recv_vout,
                    maker_recv_blinding,
                )?;
                let mpc_entropy = swap_mpc_entropy(&offer.swap_id);
                let fascia = self.color_psbt_finalize(&mut psbt, Some(mpc_entropy))?;
                let taker_recv_vout =
                    taker_rgb_vout.ok_or_else(|| swap_invalid("missing taker RGB vout"))?;
                let taker_recv_blinding = proposal
                    .request
                    .taker_rgb_blinding
                    .ok_or_else(|| swap_invalid("missing taker RGB blinding"))?;
                swap_validate_fascia_received_leg(
                    &fascia,
                    &offer.maker_gives,
                    taker_recv_vout,
                    taker_recv_blinding,
                )?;
                self.consume_fascia(fascia.clone(), None)?;
                let witness_txid = psbt.get_txid();
                let recv_asset_id = offer
                    .maker_receives
                    .asset_id
                    .clone()
                    .expect("RGB leg has asset ID");
                let recv_contract_id = ContractId::from_str(&recv_asset_id)
                    .map_err(|e| swap_invalid(format!("invalid RGB asset ID: {e}")))?;
                let beneficiaries = taker_beneficiaries
                    .get(&recv_contract_id)
                    .cloned()
                    .ok_or_else(|| swap_invalid("missing taker beneficiaries"))?;
                let transfer =
                    self.generate_transfer(recv_contract_id, beneficiaries, witness_txid)?;
                let txid_str = witness_txid.to_string();
                swap_record_outgoing(&txn, &recv_asset_id, &txid_str, &psbt)?;
                let recv_recipient_id = offer
                    .maker_rgb_recipient_id
                    .as_deref()
                    .ok_or_else(|| swap_invalid("missing maker RGB recipient ID"))?;
                consignments.extend(swap_emit_consignments(
                    self,
                    vec![transfer],
                    offer.proxy_url.as_deref(),
                    &offer.swap_id,
                    &txid_str,
                    recv_recipient_id,
                    maker_recv_vout,
                    maker_recv_blinding,
                )?);
                swap_restore_input_metadata(&mut psbt, &all_inputs)?;
            }
        }
        let _ = taker_rgb_vout;
        self.sync_if_requested(&txn, Some(online), skip_sync, KeychainKind::Internal)?;
        self.sync_if_requested(&txn, Some(online), skip_sync, KeychainKind::External)?;
        swap_ensure_inputs_confirmed(self, &proposal.maker_inputs, min_confirmations)?;
        swap_ensure_inputs_confirmed(self, &proposal.request.taker_inputs, min_confirmations)?;
        swap_sign_psbt(self, &mut psbt)?;
        let finalized_psbt = swap_finalize_psbt(self, &psbt)?;
        let txid = psbt.unsigned_tx.compute_txid().to_string();
        let completion = OnchainSwapCompletion {
            proposal,
            psbt: psbt.to_string(),
            finalized_psbt,
            txid,
            consignments,
            taker_history,
        };
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Complete swap proposal completed");
        Ok(completion)
    }

    /// Process a swap completion as the maker after receiving it from the taker.
    ///
    /// For BTC-for-RGB and RGB-for-RGB swaps, the maker first validates the incoming RGB
    /// consignment and only then signs/finalizes the PSBT. For RGB-for-RGB swaps, the maker also
    /// consumes the taker's fascia and generates the consignment for the leg they are sending.
    ///
    /// The returned (possibly updated) [`OnchainSwapCompletion`] is what both parties should
    /// use when calling [`accept_swap_transfers`](Wallet::accept_swap_transfers).
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn process_swap_completion(
        &mut self,
        online: Online,
        completion: OnchainSwapCompletion,
    ) -> Result<OnchainSwapCompletion, Error> {
        info!(self.logger(), "Processing on-chain swap completion...");
        self.check_online(online)?;
        let offer = completion.proposal.request.offer.clone();
        let local_proposal: OnchainSwapProposal =
            swap_load_state(self.wallet_dir(), &offer.swap_id, SWAP_PROPOSAL_FILE)?;
        swap_ensure_state_matches(&local_proposal, &completion.proposal, "proposal")?;
        let direction = swap_validate_legs(&offer.maker_gives, &offer.maker_receives)?;
        swap_validate_proxy_url(&offer.proxy_url)?;
        let mut psbt = Psbt::from_str(&completion.psbt)?;
        swap_validate_proposal_psbt(&completion.proposal, &psbt)?;
        let txid = psbt.unsigned_tx.compute_txid().to_string();
        if completion.txid != txid {
            return Err(swap_invalid("swap completion txid mismatch"));
        }
        let all_inputs = completion
            .proposal
            .maker_inputs
            .iter()
            .chain(completion.proposal.request.taker_inputs.iter())
            .cloned()
            .collect::<Vec<_>>();
        swap_restore_input_metadata(&mut psbt, &all_inputs)?;
        let (_expected_psbt, maker_rgb_vout, taker_rgb_vout) =
            swap_build_psbt(&completion.proposal)?;

        match direction {
            SwapDirection::RgbForBtc => {
                info!(self.logger(), "Process swap completion completed");
                Ok(completion)
            }
            SwapDirection::BtcForRgb => {
                let recv_vout =
                    maker_rgb_vout.ok_or_else(|| swap_invalid("missing maker RGB vout"))?;
                let recv_blinding = offer
                    .maker_rgb_blinding
                    .ok_or_else(|| swap_invalid("missing maker RGB blinding"))?;
                let recv_recipient_id = offer
                    .maker_rgb_recipient_id
                    .as_deref()
                    .ok_or_else(|| swap_invalid("missing maker RGB recipient ID"))?;
                swap_validate_received_swap_leg(
                    self,
                    &completion.consignments,
                    &offer.maker_receives,
                    &txid,
                    recv_vout,
                    recv_blinding,
                    recv_recipient_id,
                )?;

                let txn = self.database().begin_transaction()?;
                self.sync_if_requested(&txn, Some(online), false, KeychainKind::Internal)?;
                self.sync_if_requested(&txn, Some(online), false, KeychainKind::External)?;
                swap_ensure_inputs_confirmed(self, &completion.proposal.maker_inputs, 0)?;
                swap_ensure_inputs_confirmed(self, &completion.proposal.request.taker_inputs, 0)?;
                swap_sign_psbt(self, &mut psbt)?;
                let finalized_psbt = swap_finalize_psbt(self, &psbt)?;
                let completion = OnchainSwapCompletion {
                    psbt: psbt.to_string(),
                    finalized_psbt,
                    ..completion
                };
                self.update_backup_info(&txn, false)?;
                txn.commit()?;
                info!(self.logger(), "Process swap completion completed");
                Ok(completion)
            }
            SwapDirection::RgbForRgb => {
                let recv_vout =
                    maker_rgb_vout.ok_or_else(|| swap_invalid("missing maker RGB vout"))?;
                let recv_blinding = offer
                    .maker_rgb_blinding
                    .ok_or_else(|| swap_invalid("missing maker RGB blinding"))?;
                let recv_recipient_id = offer
                    .maker_rgb_recipient_id
                    .as_deref()
                    .ok_or_else(|| swap_invalid("missing maker RGB recipient ID"))?;
                swap_validate_received_swap_leg(
                    self,
                    &completion.consignments,
                    &offer.maker_receives,
                    &txid,
                    recv_vout,
                    recv_blinding,
                    recv_recipient_id,
                )?;

                let txn = self.database().begin_transaction()?;
                let taker_history = completion
                    .taker_history
                    .as_ref()
                    .ok_or_else(|| swap_invalid("missing taker asset history"))?;
                swap_import_asset_history(self, &txn, taker_history)?;
                let fascia = self.fascia_from_finalized_psbt(&psbt)?;
                self.consume_fascia(fascia, None)?;
                let witness_txid = psbt.get_txid();
                let send_vout =
                    taker_rgb_vout.ok_or_else(|| swap_invalid("missing taker RGB vout"))?;
                let send_blinding = completion
                    .proposal
                    .request
                    .taker_rgb_blinding
                    .ok_or_else(|| swap_invalid("missing taker RGB blinding"))?;
                let send_asset_id = offer
                    .maker_gives
                    .asset_id
                    .clone()
                    .expect("RGB leg has asset ID");
                let send_contract_id = ContractId::from_str(&send_asset_id)
                    .map_err(|e| swap_invalid(format!("invalid RGB asset ID: {e}")))?;
                let beneficiaries = vec![BuilderSeal::Revealed(GraphSeal::with_blinded_vout(
                    send_vout,
                    send_blinding,
                ))];
                let transfer =
                    self.generate_transfer(send_contract_id, beneficiaries, witness_txid)?;
                let txid_str = witness_txid.to_string();
                swap_record_outgoing(&txn, &send_asset_id, &txid_str, &psbt)?;
                let send_recipient_id = completion
                    .proposal
                    .request
                    .taker_rgb_recipient_id
                    .as_deref()
                    .ok_or_else(|| swap_invalid("missing taker RGB recipient ID"))?;
                let mut consignments = completion.consignments.clone();
                consignments.extend(swap_emit_consignments(
                    self,
                    vec![transfer],
                    offer.proxy_url.as_deref(),
                    &offer.swap_id,
                    &txid_str,
                    send_recipient_id,
                    send_vout,
                    send_blinding,
                )?);
                self.sync_if_requested(&txn, Some(online), false, KeychainKind::Internal)?;
                self.sync_if_requested(&txn, Some(online), false, KeychainKind::External)?;
                swap_ensure_inputs_confirmed(self, &completion.proposal.maker_inputs, 0)?;
                swap_ensure_inputs_confirmed(self, &completion.proposal.request.taker_inputs, 0)?;
                swap_sign_psbt(self, &mut psbt)?;
                let finalized_psbt = swap_finalize_psbt(self, &psbt)?;
                let completion = OnchainSwapCompletion {
                    psbt: psbt.to_string(),
                    finalized_psbt,
                    consignments,
                    ..completion
                };
                self.update_backup_info(&txn, false)?;
                txn.commit()?;
                info!(self.logger(), "Process swap completion completed");
                Ok(completion)
            }
        }
    }

    /// Broadcast the finalized Bitcoin transaction for a completed on-chain swap.
    ///
    /// The completion must be the latest processed completion for the swap, with
    /// `finalized_psbt` populated by the last signing party.
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn broadcast_swap_completion(
        &mut self,
        online: Online,
        completion: OnchainSwapCompletion,
    ) -> Result<String, Error> {
        info!(self.logger(), "Broadcasting on-chain swap completion...");
        self.check_online(online)?;
        let offer = &completion.proposal.request.offer;
        swap_validate_legs(&offer.maker_gives, &offer.maker_receives)?;
        swap_validate_proxy_url(&offer.proxy_url)?;
        let finalized_psbt = completion
            .finalized_psbt
            .as_deref()
            .ok_or_else(|| swap_invalid("swap completion is not finalized"))?;
        let psbt = Psbt::from_str(finalized_psbt)?;
        swap_validate_proposal_psbt(&completion.proposal, &psbt)?;
        let txid = psbt.unsigned_tx.compute_txid().to_string();
        if completion.txid != txid {
            return Err(swap_invalid("swap completion txid mismatch"));
        }

        let txn = self.database().begin_transaction()?;
        let tx = self.broadcast_psbt(&txn, &psbt)?;
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        let txid = tx.compute_txid().to_string();
        info!(self.logger(), "Broadcast swap completion completed");
        Ok(txid)
    }

    /// Accept the RGB transfers received from a completed on-chain swap.
    ///
    /// Each party calls this method with their own `role` once the swap transaction has been
    /// broadcast (or confirmed, depending on `min_confirmations`). The consignments embedded in
    /// the completion are validated and accepted into the wallet's RGB state.
    ///
    /// Returns the list of [`Assignment`]s received, or an empty list if this party is receiving
    /// only BTC.
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn accept_swap_transfers(
        &mut self,
        online: Online,
        completion: OnchainSwapCompletion,
        role: OnchainSwapRole,
        skip_sync: bool,
    ) -> Result<OnchainSwapReceiveResult, Error> {
        info!(self.logger(), "Accepting on-chain swap transfers...");
        self.check_online(online)?;
        let offer = &completion.proposal.request.offer;
        swap_validate_proxy_url(&offer.proxy_url)?;
        let txn = self.database().begin_transaction()?;
        self.sync_if_requested(&txn, Some(online), skip_sync, KeychainKind::External)?;
        let receives = match role {
            OnchainSwapRole::Maker => offer.maker_receives.clone(),
            OnchainSwapRole::Taker => offer.maker_gives.clone(),
        };
        if !matches!(receives.kind, OnchainSwapLegKind::Rgb) {
            let result = OnchainSwapReceiveResult {
                assignments: vec![],
            };
            self.update_backup_info(&txn, false)?;
            txn.commit()?;
            return Ok(result);
        }
        let mut assignments = vec![];
        let matching_consignments = completion
            .consignments
            .iter()
            .filter(|c| receives.asset_id.as_deref() == Some(c.asset_id.as_str()))
            .collect::<Vec<_>>();
        if matching_consignments.is_empty() {
            return Err(swap_invalid("missing RGB swap consignment"));
        }
        for consignment in matching_consignments {
            let local_path = swap_fetch_consignment_to_file(self, consignment)?;
            let mut accepted = swap_accept_transfer_from_file(
                self,
                &txn,
                &local_path,
                consignment.txid.clone(),
                consignment.vout,
                consignment.blinding,
                &consignment.recipient_id,
            )?;
            assignments.append(&mut accepted);
        }
        if assignments.is_empty() {
            return Err(swap_invalid(
                "RGB swap consignment did not assign any state",
            ));
        }
        let result = OnchainSwapReceiveResult { assignments };
        self.update_backup_info(&txn, false)?;
        txn.commit()?;
        info!(self.logger(), "Accept swap transfers completed");
        Ok(result)
    }
}
