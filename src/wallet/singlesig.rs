//! RGB singlesig wallet module.
//!
//! This module defines the methods of the [`Wallet`] structure.

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
        bdk_network: &BdkNetwork,
    ) -> Result<(WalletDescriptors, bool), Error> {
        let xpub_rgb = str_to_xpub(&self.account_xpub_colored, bdk_network)?;
        let xpub_btc = str_to_xpub(&self.account_xpub_vanilla, bdk_network)?;
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
    fn wallet_specific_consistency_checks(&mut self) -> Result<(), Error> {
        self.sync_db_txos(true, false)?;
        let bdk_utxos: Vec<String> = self
            .bdk_wallet()
            .list_unspent()
            .map(|u| u.outpoint.to_string())
            .collect();
        let bdk_utxos: HashSet<String> = HashSet::from_iter(bdk_utxos);
        let db_utxos: Vec<String> = self
            .database()
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
    pub(crate) fn watch_only(&self) -> bool {
        self.keys.mnemonic.is_none()
    }

    /// Create a new RGB singlesig wallet based on the provided [`WalletData`] and
    /// [`SinglesigKeys`].
    pub fn new(wallet_data: WalletData, keys: SinglesigKeys) -> Result<Self, Error> {
        let wdata = wallet_data.clone();
        let bdk_network = BdkNetwork::from(wdata.bitcoin_network);

        // wallet keys
        let (descs, watch_only) = keys.build_descriptors(&wdata.bitcoin_network, &bdk_network)?;

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
            bdk_network,
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
            .build_descriptors(
                &self.internals.wallet_data.bitcoin_network,
                &BdkNetwork::from(self.internals.wallet_data.bitcoin_network),
            )
            .expect("already succeeded at wallet creation")
            .0
    }

    fn sign_psbt_impl(
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

        self.update_backup_info(false)?;

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
        let reserved_idxs: Vec<i32> = self
            .database()
            .iter_reserved_txos()?
            .into_iter()
            .filter_map(|r| r.reserved_for)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if reserved_idxs.is_empty() {
            return Ok(vec![]);
        }
        let result = self
            .database()
            .get_wallet_transactions_by_idxs(&reserved_idxs)?
            .into_iter()
            .map(|wt| PendingVanillaTx {
                txid: wt.txid,
                r#type: wt.r#type,
            })
            .collect();
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
        let (wt, reservations) = self
            .database()
            .get_wallet_transaction_with_reserved_txos_by_txid(&txid)?
            .ok_or(Error::CannotAbortPendingVanillaTx)?;
        if reservations.is_empty() {
            return Err(Error::CannotAbortPendingVanillaTx);
        }
        self.database().del_wallet_transaction(wt.idx)?; // relies on cascade to delete reserved txos
        info!(self.logger(), "Abort pending vanilla TX completed");
        Ok(())
    }

    fn finalize_offline_issuance<T: IssuedAssetDetails>(
        &self,
        issue_data: &IssueData,
    ) -> Result<T, Error> {
        let mut runtime = self.rgb_runtime()?;
        let asset = self.import_and_save_contract(issue_data, &mut runtime)?;
        let result = T::from_issuance(self, &asset, issue_data)?;
        self.update_backup_info(false)?;
        Ok(result)
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
        self.issue_asset_nia_with_impl(ticker, name, precision, amounts, |issue_data| {
            self.finalize_offline_issuance(&issue_data)
        })
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
        self.issue_asset_uda_with_impl(
            ticker,
            name,
            details,
            precision,
            media_file_path,
            attachments_file_paths,
            |issue_data| {
                let mut runtime = self.rgb_runtime()?;
                let asset = self.import_and_save_contract(&issue_data, &mut runtime)?;
                let asset_uda = AssetUDA::get_asset_details(
                    self,
                    &asset,
                    issue_data.asset_data.token.map(|t| t.into()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )?;
                self.update_backup_info(false)?;
                Ok(asset_uda)
            },
        )
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
        self.issue_asset_cfa_with_impl(name, details, precision, amounts, file_path, |issue_data| {
            self.finalize_offline_issuance(&issue_data)
        })
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
        self.issue_asset_ifa_with_impl(
            ticker,
            name,
            precision,
            amounts,
            inflation_amounts,
            reject_list_url,
            |issue_data| self.finalize_offline_issuance(&issue_data),
        )
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

        let receive_data_internal = self.create_receive_data(
            asset_id,
            assignment,
            expiration_timestamp.map(|t| t as i64),
            transport_endpoints,
            RecipientType::Blind,
        )?;

        let batch_transfer_idx =
            self.store_receive_transfer(&receive_data_internal, min_confirmations)?;

        self.update_backup_info(false)?;

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

        let receive_data_internal = self.create_receive_data(
            asset_id,
            assignment,
            expiration_timestamp.map(|t| t as i64),
            transport_endpoints,
            RecipientType::Witness,
        )?;

        let batch_transfer_idx =
            self.store_receive_transfer(&receive_data_internal, min_confirmations)?;

        self.update_backup_info(false)?;

        info!(self.logger(), "Witness receive completed");
        Ok(ReceiveData {
            invoice: receive_data_internal.invoice_string,
            recipient_id: receive_data_internal.recipient_id,
            expiration_timestamp: receive_data_internal.expiration_timestamp.map(|t| t as u64),
            batch_transfer_idx,
        })
    }

    /// Prove ownership of an RGB asset by signing P2TR outputs in the consignment's witness TX.
    ///
    /// The signed message is `SHA256(txid || ":" || vout || ":" || message)`, binding the
    /// signature to the specific UTXO. The caller can include a contract ID, nonce, or any
    /// other context in `message`.
    ///
    /// The method finds all wallet-controlled P2TR outputs in the consignment's witness TX
    /// and signs each one. Each returned [`UtxoSignature`] contains the 32-byte x-only tweaked
    /// public key matching the P2TR output's scriptPubKey at bytes `[2..34]`.
    ///
    /// Returns an empty `Vec` if no owned P2TR outputs are found.
    ///
    /// A wallet with private keys (i.e. not watch-only) is required.
    pub fn prove_asset_ownership(
        &self,
        consignment: &RgbTransfer,
        message: &[u8],
    ) -> Result<Vec<UtxoSignature>, Error> {
        info!(
            self.logger(),
            "Proving asset ownership for {}...",
            consignment.contract_id()
        );
        if self.watch_only() {
            return Err(Error::WatchOnly);
        }

        let mnemonic_str = self
            .keys
            .mnemonic
            .as_ref()
            .expect("non-watch-only wallet should have a mnemonic");
        let bundle = consignment
            .bundled_witnesses()
            .last()
            .ok_or(Error::NoConsignment)?;
        let tx = bundle.pub_witness.tx().ok_or(Error::NoConsignment)?;
        let witness_txid = bundle.witness_id().to_string();
        let secp = Secp256k1::new();
        let mut signatures = Vec::new();

        // pre-compute account xprvs for both keychains
        let (rgb_account_xprv, _) = derive_account_xprv_from_mnemonic(
            &self.wallet_data().bitcoin_network,
            mnemonic_str,
            true,
            self.keys.witness_version,
        )?;
        let (vanilla_account_xprv, _) = derive_account_xprv_from_mnemonic(
            &self.wallet_data().bitcoin_network,
            mnemonic_str,
            false,
            self.keys.witness_version,
        )?;
        for (vout, output) in tx.output.iter().enumerate() {
            if !output.script_pubkey.is_p2tr() {
                continue;
            }
            let spk = output.script_pubkey.as_bytes();

            let (keychain, derivation_index) = match self
                .bdk_wallet()
                .derivation_of_spk(output.script_pubkey.clone())
            {
                Some(info) => info,
                None => continue,
            };
            let rgb = keychain == KeychainKind::External;
            let account_xprv = if rgb {
                &rgb_account_xprv
            } else {
                &vanilla_account_xprv
            };

            let keychain_index = if rgb {
                KEYCHAIN_RGB
            } else {
                self.keys.vanilla_keychain.unwrap_or(KEYCHAIN_BTC)
            };
            let child_path = vec![
                ChildNumber::from_normal_idx(keychain_index as u32).unwrap(),
                ChildNumber::from_normal_idx(derivation_index).unwrap(),
            ];
            let child_xprv = account_xprv.derive_priv(&secp, &child_path)?;
            let keypair = Keypair::from_secret_key(&secp, &child_xprv.private_key);
            let (xonly, _) = XOnlyPublicKey::from_keypair(&keypair);
            let tweaked_keypair = keypair.tap_tweak(&secp, None).to_keypair();
            let (tweaked_xonly, _) = xonly.tap_tweak(&secp, None);

            // verify our tweaked key matches the scriptPubKey
            if tweaked_xonly.serialize() != spk[2..34] {
                continue;
            }
            let outpoint = Outpoint {
                txid: witness_txid.clone(),
                vout: vout as u32,
            };
            let mut preimage = Vec::new();
            preimage.extend_from_slice(outpoint.txid.as_bytes());
            preimage.extend_from_slice(b":");
            preimage.extend_from_slice(outpoint.vout.to_string().as_bytes());
            preimage.extend_from_slice(b":");
            preimage.extend_from_slice(message);
            let msg_hash: sha256::Hash = Sha256Hash::hash(&preimage);

            let msg =
                bdk_wallet::bitcoin::secp256k1::Message::from_digest(msg_hash.to_byte_array());
            let sig = secp.sign_schnorr_no_aux_rand(&msg, &tweaked_keypair);
            signatures.push(UtxoSignature {
                outpoint,
                message: msg_hash.to_byte_array().to_vec(),
                signature: sig.as_ref().to_vec(),
                pubkey: tweaked_xonly.serialize().to_vec(),
            });
        }
        info!(self.logger(), "Prove asset ownership completed");
        Ok(signatures)
    }
}

/// Online APIs of the wallet.
#[cfg(any(feature = "electrum", feature = "esplora"))]
impl Wallet {
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
        let mut psbt = self.create_utxos_begin_impl(up_to, num, size, fee_rate, skip_sync, true)?;
        self.sign_psbt_impl(&mut psbt, None)?;
        let res = self.create_utxos_end_impl(&psbt, skip_sync)?;
        self.update_backup_info(false)?;
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
        let res = self.create_utxos_begin_impl(up_to, num, size, fee_rate, skip_sync, dry_run)?;
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
    /// Returns the number of created UTXOs, if `skip_sync` is set to true this will be 0.
    pub fn create_utxos_end(
        &mut self,
        online: Online,
        signed_psbt: String,
        skip_sync: bool,
    ) -> Result<u8, Error> {
        info!(self.logger(), "Creating UTXOs (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let res = self.create_utxos_end_impl(&psbt, skip_sync)?;
        info!(self.logger(), "Create UTXOs (end) completed");
        Ok(res)
    }

    /// Return the existing or freshly generated wallet [`Online`] data.
    ///
    /// Setting `skip_consistency_check` to false runs a check on UTXOs (BDK vs rgb-lib DB), assets
    /// (RGB vs rgb-lib DB) and medias (DB vs actual files) to try and detect possible
    /// inconsistencies in the wallet.
    /// Setting `skip_consistency_check` to true bypasses the check and allows operating an
    /// inconsistent wallet.
    ///
    /// <div class="warning">Warning: setting <tt>skip_consistency_check</tt> to true is dangerous,
    /// only do this if you know what you're doing!</div>
    pub fn go_online(
        &mut self,
        skip_consistency_check: bool,
        indexer_url: String,
    ) -> Result<Online, Error> {
        info!(self.logger(), "Going online...");
        let online = self.go_online_impl(skip_consistency_check, &indexer_url)?;
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
        let mut psbt = self.drain_to_begin_impl(address, fee_rate, true)?;
        self.sign_psbt_impl(&mut psbt, None)?;
        let tx = self.drain_to_end_impl(&psbt)?;
        self.update_backup_info(false)?;
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
        let psbt = self.drain_to_begin_impl(address, fee_rate, dry_run)?;
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
        let tx = self.drain_to_end_impl(&psbt)?;
        self.update_backup_info(false)?;
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
        skip_sync: bool,
    ) -> Result<OperationResult, Error> {
        info!(self.logger(), "Sending to: {:?}...", recipient_map);
        self.check_xprv()?;
        self.check_online(online)?;
        let mut begin_op_data = self.send_begin_impl(
            recipient_map,
            donation,
            fee_rate,
            min_confirmations,
            expiration_timestamp.map(|t| t as i64),
            true,
        )?;
        self.sign_psbt_impl(&mut begin_op_data.psbt, None)?;
        let res = self.send_end_impl(&begin_op_data.psbt, skip_sync)?;
        self.update_backup_info(false)?;
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
        let begin_op_data = self.send_begin_impl(
            recipient_map,
            donation,
            fee_rate,
            min_confirmations,
            expiration_timestamp.map(|t| t as i64),
            dry_run,
        )?;
        self.update_backup_info(false)?;
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
        skip_sync: bool,
    ) -> Result<OperationResult, Error> {
        info!(self.logger(), "Sending (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let res = self.send_end_impl(&psbt, skip_sync)?;
        self.update_backup_info(false)?;
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
        let mut psbt = self.send_btc_begin_impl(address, amount, fee_rate, skip_sync, true)?;
        self.sign_psbt_impl(&mut psbt, None)?;
        let res = self.send_btc_end_impl(&psbt, skip_sync)?;
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
        let res = self.send_btc_begin_impl(address, amount, fee_rate, skip_sync, dry_run)?;
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
    pub fn send_btc_end(
        &mut self,
        online: Online,
        signed_psbt: String,
        skip_sync: bool,
    ) -> Result<String, Error> {
        info!(self.logger(), "Sending BTC (end)...");
        self.check_online(online)?;
        let psbt = Psbt::from_str(&signed_psbt)?;
        let res = self.send_btc_end_impl(&psbt, skip_sync)?;
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
        let mut begin_op_data = self.inflate_begin_impl(
            asset_id,
            inflation_amounts,
            fee_rate,
            min_confirmations,
            true,
        )?;
        self.sign_psbt_impl(&mut begin_op_data.psbt, None)?;
        let res = self.inflate_end_impl(&begin_op_data.psbt)?;
        self.update_backup_info(false)?;
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
        let begin_operation_data = self.inflate_begin_impl(
            asset_id,
            inflation_amounts,
            fee_rate,
            min_confirmations,
            dry_run,
        )?;
        self.update_backup_info(false)?;
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
    /// The API syncs and doesn't provide a way to skip that.
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
        let res = self.inflate_end_impl(&psbt)?;
        self.update_backup_info(false)?;
        info!(self.logger(), "Inflate (end) completed");
        Ok(res)
    }
}
