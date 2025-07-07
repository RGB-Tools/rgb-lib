//! RGB Rust-only methods module
//!
//! This module defines additional utility methods that are not exposed via FFI

use super::*;

/// RGB asset-specific information to color a transaction
#[derive(Clone, Debug)]
pub struct AssetColoringInfo {
    /// Map of vouts and asset amounts to color the transaction outputs
    pub output_map: HashMap<u32, u64>,
    /// Static blinding to keep the transaction construction deterministic
    pub static_blinding: Option<u64>,
}

/// RGB information to color a transaction
#[derive(Clone, Debug)]
pub struct ColoringInfo {
    /// Asset-specific information
    pub asset_info_map: HashMap<ContractId, AssetColoringInfo>,
    /// Static blinding to keep the transaction construction deterministic
    pub static_blinding: Option<u64>,
    /// Nonce for offchain TXs ordering
    pub nonce: Option<u64>,
}

/// Map of contract ID and list of its beneficiaries
pub type AssetBeneficiariesMap = BTreeMap<ContractId, Vec<BuilderSeal<GraphSeal>>>;

/// Indexer protocol
#[derive(Clone, Debug)]
pub enum IndexerProtocol {
    /// An indexer implementing the electrum protocol
    Electrum,
    /// An indexer implementing the esplora protocol
    Esplora,
}

impl fmt::Display for IndexerProtocol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Return the indexer protocol for the provided URL.
/// An error is raised if the provided indexer URL is invalid or if the service is for the wrong
/// network or doesn't have the required functionality.
///
/// <div class="warning">This method is meant for special usage and is normally not needed, use
/// it only if you know what you're doing</div>
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn check_indexer_url(
    indexer_url: &str,
    bitcoin_network: BitcoinNetwork,
) -> Result<IndexerProtocol, Error> {
    let indexer = get_indexer(indexer_url, bitcoin_network)?;
    let indexer_protocol = match indexer {
        #[cfg(feature = "electrum")]
        Indexer::Electrum(_) => IndexerProtocol::Electrum,
        #[cfg(feature = "esplora")]
        Indexer::Esplora(_) => IndexerProtocol::Esplora,
    };

    Ok(indexer_protocol)
}

/// Check whether the provided URL points to a valid proxy.
/// An error is raised if the provided proxy URL is invalid or if the service is running an
/// unsupported protocol version.
///
/// <div class="warning">This method is meant for special usage and is normally not needed, use
/// it only if you know what you're doing</div>
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn check_proxy_url(proxy_url: &str) -> Result<(), Error> {
    check_proxy(proxy_url, None)
}

impl Wallet {
    /// Color a PSBT.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn color_psbt(
        &self,
        psbt_to_color: &mut Psbt,
        coloring_info: ColoringInfo,
    ) -> Result<(Fascia, AssetBeneficiariesMap), Error> {
        info!(self.logger, "Coloring PSBT...");
        let mut transaction = match psbt_to_color.clone().extract_tx() {
            Ok(tx) => tx,
            Err(ExtractTxError::MissingInputValue { tx }) => tx, // required for non-standard TXs
            Err(e) => return Err(InternalError::from(e).into()),
        };
        let mut opreturn_first = false;
        if transaction.output.iter().any(|o| o.script_pubkey.is_p2tr()) {
            opreturn_first = true;
        }

        let psbt = if !transaction
            .output
            .iter()
            .any(|o| o.script_pubkey.is_op_return())
        {
            let opreturn_output = TxOut {
                value: BdkAmount::ZERO,
                script_pubkey: ScriptBuf::new_op_return([]),
            };
            if opreturn_first {
                transaction.output.insert(0, opreturn_output);
            } else {
                transaction.output.push(opreturn_output);
            }
            Psbt::from_unsigned_tx(transaction).unwrap()
        } else {
            psbt_to_color.clone()
        };

        let runtime = self.rgb_runtime()?;

        let mut rgb_psbt = RgbPsbt::from_str(&psbt.to_string()).unwrap();

        let prev_outputs = rgb_psbt
            .inputs()
            .map(|txin| txin.previous_outpoint)
            .collect::<HashSet<RgbOutpoint>>();

        let mut all_transitions: HashMap<ContractId, Transition> = HashMap::new();
        let mut asset_beneficiaries: AssetBeneficiariesMap = bmap![];
        let assignment_name = FieldName::from("assetOwner");

        for (contract_id, asset_coloring_info) in coloring_info.asset_info_map.clone() {
            let schema = AssetSchema::get_from_contract_id(contract_id, &runtime)?;

            let mut asset_transition_builder =
                runtime.transition_builder(contract_id, "transfer")?;

            let mut asset_available_amt = 0;
            let mut uda_state = None;
            for (_, opout_state_map) in
                runtime.contract_assignments_for(contract_id, prev_outputs.iter().copied())?
            {
                for (opout, state) in opout_state_map {
                    if let AllocatedState::Amount(amt) = &state {
                        asset_available_amt += amt.as_u64();
                    } else if let AllocatedState::Data(_) = &state {
                        asset_available_amt = 1;
                        // there can be only a single state when contract is UDA
                        uda_state = Some(state.clone());
                    }
                    asset_transition_builder = asset_transition_builder.add_input(opout, state)?;
                }
            }

            let mut beneficiaries = vec![];
            let mut sending_amt = 0;
            for (mut vout, amount) in asset_coloring_info.output_map {
                if amount == 0 {
                    continue;
                }
                if opreturn_first {
                    vout += 1;
                }
                sending_amt += amount;
                if vout as usize > rgb_psbt.outputs().count() {
                    return Err(Error::InvalidColoringInfo {
                        details: s!("invalid vout in output_map, does not exist in the given PSBT"),
                    });
                }
                let graph_seal = if let Some(blinding) = asset_coloring_info.static_blinding {
                    GraphSeal::with_blinded_vout(vout, blinding)
                } else {
                    GraphSeal::new_random_vout(vout)
                };
                let seal = BuilderSeal::Revealed(graph_seal);
                beneficiaries.push(seal);

                match schema {
                    AssetSchema::Nia | AssetSchema::Cfa | AssetSchema::Ifa => {
                        asset_transition_builder = asset_transition_builder.add_fungible_state(
                            assignment_name.clone(),
                            seal,
                            amount,
                        )?;
                    }
                    AssetSchema::Uda => {
                        if let AllocatedState::Data(state) = uda_state.clone().unwrap() {
                            asset_transition_builder = asset_transition_builder
                                .add_data(assignment_name.clone(), seal, Allocation::from(state))
                                .map_err(Error::from)?;
                        }
                    }
                }
            }
            if sending_amt > asset_available_amt {
                return Err(Error::InvalidColoringInfo {
                    details: format!(
                        "total amount in output_map ({sending_amt}) greater than available ({asset_available_amt})"
                    ),
                });
            }

            if let Some(nonce) = coloring_info.nonce {
                asset_transition_builder = asset_transition_builder.set_nonce(nonce);
            }

            let transition = asset_transition_builder.complete_transition()?;
            all_transitions.insert(contract_id, transition);
            asset_beneficiaries.insert(contract_id, beneficiaries);
        }

        let (opreturn_index, _) = rgb_psbt
            .to_unsigned_tx()
            .outputs
            .iter()
            .enumerate()
            .find(|(_, o)| o.script_pubkey.is_op_return())
            .expect("psbt should have an op_return output");
        let (_, opreturn_output) = rgb_psbt
            .outputs_mut()
            .enumerate()
            .find(|(i, _)| i == &opreturn_index)
            .unwrap();
        opreturn_output
            .set_opret_host()
            .map_err(InternalError::from)?;
        if let Some(blinding) = coloring_info.static_blinding {
            opreturn_output
                .set_mpc_entropy(blinding)
                .map_err(InternalError::from)?;
        }

        for (contract_id, transition) in all_transitions {
            for opout in transition.inputs() {
                rgb_psbt
                    .set_rgb_contract_consumer(contract_id, opout, transition.id())
                    .map_err(InternalError::from)?;
            }
            rgb_psbt
                .push_rgb_transition(transition)
                .map_err(InternalError::from)?;
        }

        rgb_psbt.set_rgb_close_method(CloseMethod::OpretFirst);
        rgb_psbt.complete_construction();
        let fascia = rgb_psbt.rgb_commit().map_err(|e| Error::Internal {
            details: e.to_string(),
        })?;

        *psbt_to_color = Psbt::from_str(&rgb_psbt.to_string()).unwrap();

        info!(self.logger, "Color PSBT completed");
        Ok((fascia, asset_beneficiaries))
    }

    /// Color a PSBT, consume the RGB fascia and return the related consignment.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn color_psbt_and_consume(
        &self,
        psbt_to_color: &mut Psbt,
        coloring_info: ColoringInfo,
    ) -> Result<Vec<RgbTransfer>, Error> {
        info!(self.logger, "Coloring PSBT and consuming...");
        let (fascia, asset_beneficiaries) =
            self.color_psbt(psbt_to_color, coloring_info.clone())?;

        let rgb_psbt = RgbPsbt::from_str(&psbt_to_color.to_string()).unwrap();
        let witness_txid = rgb_psbt.txid();

        let mut runtime = self.rgb_runtime()?;
        runtime.consume_fascia(fascia, witness_txid, None)?;

        let mut transfers = vec![];
        for (contract_id, beneficiaries) in asset_beneficiaries {
            let mut beneficiaries_witness = vec![];
            let mut beneficiaries_blinded = vec![];
            for builder_seal in beneficiaries {
                match builder_seal {
                    BuilderSeal::Revealed(seal) => {
                        let explicit_seal = ExplicitSeal::with(witness_txid, seal.vout);
                        beneficiaries_witness.push(explicit_seal);
                    }
                    BuilderSeal::Concealed(secret_seal) => {
                        beneficiaries_blinded.push(secret_seal);
                    }
                };
            }
            transfers.push(runtime.transfer(
                contract_id,
                beneficiaries_witness,
                beneficiaries_blinded,
                Some(witness_txid),
            )?);
        }

        *psbt_to_color = Psbt::from_str(&rgb_psbt.to_string()).unwrap();

        info!(self.logger, "Color PSBT and consume completed");
        Ok(transfers)
    }

    /// Accept an RGB transfer using a TXID to retrieve its consignment.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn accept_transfer(
        &mut self,
        txid: String,
        vout: u32,
        consignment_endpoint: RgbTransport,
        blinding: u64,
    ) -> Result<(RgbTransfer, Vec<Assignment>), Error> {
        info!(self.logger, "Accepting transfer...");
        let witness_id = RgbTxid::from_str(&txid).map_err(|_| Error::InvalidTxid)?;
        let proxy_url = TransportEndpoint::try_from(consignment_endpoint)?.endpoint;

        let consignment_res = self.get_consignment(&proxy_url, txid.clone())?;
        let consignment_bytes = general_purpose::STANDARD
            .decode(consignment_res.consignment)
            .map_err(InternalError::from)?;
        let consignment = RgbTransfer::load(&consignment_bytes[..]).map_err(InternalError::from)?;

        let schema_id = consignment.schema_id().to_string();
        let asset_schema: AssetSchema = schema_id.try_into()?;
        self.check_schema_support(&asset_schema)?;
        debug!(
            self.logger,
            "Got consignment for asset with {} schema", asset_schema
        );

        let mut runtime = self.rgb_runtime()?;

        let graph_seal = GraphSeal::with_blinded_vout(vout, blinding);
        runtime.store_secret_seal(graph_seal)?;

        let resolver = OffchainResolver {
            witness_id,
            consignment: &IndexedConsignment::new(&consignment),
            fallback: self.blockchain_resolver(),
        };

        debug!(self.logger, "Validating consignment...");
        let (validation_status, validated_transfer) =
            match consignment
                .clone()
                .validate(&resolver, self.chain_net(), None)
            {
                Ok(consignment) => (
                    consignment.clone().into_validation_status(),
                    Some(consignment),
                ),
                Err(status) => (status, None),
            };
        let validity = validation_status.validity();
        debug!(self.logger, "Consignment validity: {:?}", validity);
        if validity != Validity::Valid {
            error!(
                self.logger,
                "Consignment has an invalid status: {validation_status:?}"
            );
            return Err(Error::InvalidConsignment);
        }

        let mut minimal_contract = consignment.clone().into_contract();
        minimal_contract.bundles = none!();
        minimal_contract.terminals = none!();
        let minimal_contract_validated =
            match minimal_contract.validate(self.blockchain_resolver(), self.chain_net(), None) {
                Ok(cons) => cons,
                Err(_) => unreachable!("already passed validation"),
            };
        runtime
            .import_contract(minimal_contract_validated, self.blockchain_resolver())
            .expect("failure importing validated contract");

        let received_rgb_assignments =
            self.extract_received_assignments(&consignment, witness_id, Some(vout), None);

        let _status = runtime.accept_transfer(validated_transfer.unwrap(), &resolver)?;

        info!(self.logger, "Accept transfer completed");
        Ok((consignment, received_rgb_assignments))
    }

    /// Consume an RGB fascia.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn consume_fascia(
        &self,
        fascia: Fascia,
        witness_txid: RgbTxid,
        witness_ord: Option<WitnessOrd>,
    ) -> Result<(), Error> {
        info!(self.logger, "Consuming fascia...");
        self.rgb_runtime()?
            .consume_fascia(fascia.clone(), witness_txid, witness_ord)?;
        info!(self.logger, "Consume fascia completed");
        Ok(())
    }

    /// Get the height for a Bitcoin TX.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn get_tx_height(&self, txid: String) -> Result<Option<u32>, Error> {
        info!(self.logger, "Getting TX height...");
        let txid = RgbTxid::from_str(&txid).map_err(|_| Error::InvalidTxid)?;
        let height = match self
            .blockchain_resolver()
            .resolve_pub_witness_ord(txid)
            .map_err(|e| Error::Network {
                details: e.to_string(),
            })? {
            WitnessOrd::Mined(witness_pos) => Some(witness_pos.height().get()),
            _ => None,
        };
        info!(self.logger, "Get TX height completed");
        Ok(height)
    }

    /// Update RGB witnesses.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn update_witnesses(
        &self,
        after_height: u32,
        force_witnesses: Vec<RgbTxid>,
    ) -> Result<UpdateRes, Error> {
        info!(self.logger, "Updating witnesses...");
        let update_res = self.rgb_runtime()?.update_witnesses(
            self.blockchain_resolver(),
            after_height,
            force_witnesses,
        )?;
        info!(self.logger, "Update witnesses completed");
        Ok(update_res)
    }

    /// Post a consignment to the proxy server.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn post_consignment<P: AsRef<Path>>(
        &self,
        proxy_url: &str,
        recipient_id: String,
        consignment_path: P,
        txid: String,
        vout: Option<u32>,
    ) -> Result<(), Error> {
        info!(self.logger, "Posting consignment...");
        let consignment_res = self.rest_client.clone().post_consignment(
            proxy_url,
            recipient_id.clone(),
            consignment_path,
            txid.clone(),
            vout,
        )?;
        debug!(
            self.logger,
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

        info!(self.logger, "Post consignment completed");
        Ok(())
    }

    /// Extract the metadata of a new RGB asset and save the asset into the DB.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn save_new_asset(
        &self,
        contract_id: ContractId,
        contract: Option<Contract>,
    ) -> Result<(), Error> {
        info!(self.logger, "Saving new asset...");
        let runtime = self.rgb_runtime()?;
        let contract = if let Some(contract) = contract {
            contract
        } else {
            runtime.export_contract(contract_id)?
        };

        let asset_schema = AssetSchema::get_from_contract_id(contract_id, &runtime)?;

        let timestamp = contract.genesis.timestamp;
        let (name, precision, issued_supply, ticker, details, media_idx, token) =
            match &asset_schema {
                AssetSchema::Nia => {
                    let contract = runtime.contract_wrapper::<NonInflatableAsset>(contract_id)?;
                    let spec = contract.spec();
                    let ticker = spec.ticker().to_string();
                    let name = spec.name().to_string();
                    let details = spec.details().map(|d| d.to_string());
                    let precision = spec.precision.into();
                    let issued_supply = contract.total_issued_supply().into();
                    let media_idx = if let Some(attachment) = contract.contract_terms().media {
                        Some(self.get_or_insert_media(
                            hex::encode(attachment.digest),
                            attachment.ty.to_string(),
                        )?)
                    } else {
                        None
                    };
                    (
                        name,
                        precision,
                        issued_supply,
                        Some(ticker),
                        details,
                        media_idx,
                        None,
                    )
                }
                AssetSchema::Uda => {
                    let contract = runtime.contract_wrapper::<UniqueDigitalAsset>(contract_id)?;
                    let spec = contract.spec();
                    let ticker = spec.ticker().to_string();
                    let name = spec.name().to_string();
                    let details = spec.details().map(|d| d.to_string());
                    let precision = spec.precision.into();
                    let issued_supply = 1;
                    let token_full =
                        Token::from_token_data(&contract.token_data(), self.get_media_dir());
                    (
                        name,
                        precision,
                        issued_supply,
                        Some(ticker),
                        details,
                        None,
                        Some(token_full),
                    )
                }
                AssetSchema::Cfa => {
                    let contract =
                        runtime.contract_wrapper::<CollectibleFungibleAsset>(contract_id)?;
                    let name = contract.name().to_string();
                    let details = contract.details().map(|d| d.to_string());
                    let precision = contract.precision().into();
                    let issued_supply = contract.total_issued_supply().into();
                    let media_idx = if let Some(attachment) = contract.contract_terms().media {
                        Some(self.get_or_insert_media(
                            hex::encode(attachment.digest),
                            attachment.ty.to_string(),
                        )?)
                    } else {
                        None
                    };
                    (
                        name,
                        precision,
                        issued_supply,
                        None,
                        details,
                        media_idx,
                        None,
                    )
                }
                AssetSchema::Ifa => {
                    let contract =
                        runtime.contract_wrapper::<InflatableFungibleAsset>(contract_id)?;
                    let spec = contract.spec();
                    let ticker = spec.ticker().to_string();
                    let name = spec.name().to_string();
                    let details = spec.details().map(|d| d.to_string());
                    let precision = spec.precision.into();
                    let issued_supply = contract.total_issued_supply().into();
                    let media_idx = if let Some(attachment) = contract.contract_terms().media {
                        Some(self.get_or_insert_media(
                            hex::encode(attachment.digest),
                            attachment.ty.to_string(),
                        )?)
                    } else {
                        None
                    };
                    (
                        name,
                        precision,
                        issued_supply,
                        Some(ticker),
                        details,
                        media_idx,
                        None,
                    )
                }
            };

        let db_asset = self.add_asset_to_db(
            contract_id.to_string(),
            &asset_schema,
            None,
            details,
            issued_supply,
            name,
            precision,
            ticker,
            timestamp,
            media_idx,
        )?;

        if let Some(token) = token {
            let db_token = DbTokenActMod {
                asset_idx: ActiveValue::Set(db_asset.idx),
                index: ActiveValue::Set(token.index),
                ticker: ActiveValue::Set(token.ticker),
                name: ActiveValue::Set(token.name),
                details: ActiveValue::Set(token.details),
                embedded_media: ActiveValue::Set(token.embedded_media.is_some()),
                reserves: ActiveValue::Set(token.reserves.is_some()),
                ..Default::default()
            };
            let token_idx = self.database.set_token(db_token)?;

            if let Some(media) = &token.media {
                self.save_token_media(token_idx, media.get_digest(), media.mime.clone(), None)?;
            }
            for (attachment_id, media) in token.attachments {
                self.save_token_media(
                    token_idx,
                    media.get_digest(),
                    media.mime.clone(),
                    Some(attachment_id),
                )?;
            }
        }

        self.update_backup_info(false)?;

        info!(self.logger, "Save new asset completed");
        Ok(())
    }

    /// List the Bitcoin unspents of the vanilla wallet, using BDK's objects, filtered by
    /// `min_confirmations`.
    ///
    /// <div class="warning">This method is meant for special usage, for most cases the method
    /// <code>list_unspents</code> is sufficient</div>
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn list_unspents_vanilla(
        &mut self,
        online: Online,
        min_confirmations: u8,
        skip_sync: bool,
    ) -> Result<Vec<LocalOutput>, Error> {
        info!(self.logger, "Listing unspents vanilla...");
        self.sync_if_requested(Some(online), skip_sync)?;

        let unspents = self.internal_unspents();

        let res = if min_confirmations > 0 {
            unspents
                .filter_map(|u| {
                    match self
                        .indexer()
                        .get_tx_confirmations(&u.outpoint.txid.to_string())
                    {
                        Ok(confirmations) => {
                            if let Some(confirmations) = confirmations {
                                if confirmations >= min_confirmations as u64 {
                                    return Some(Ok(u));
                                }
                            }
                            None
                        }
                        Err(e) => Some(Err(e)),
                    }
                })
                .collect::<Result<Vec<LocalOutput>, Error>>()
        } else {
            Ok(unspents.collect())
        };

        info!(self.logger, "List unspents vanilla completed");
        res
    }

    /// Return the transfer dir path for the provided transfer ID (e.g. the TXID).
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn get_transfer_dir(&self, transfer_id: &str) -> PathBuf {
        self.get_transfers_dir().join(transfer_id)
    }

    /// Return the asset transfer dir path for the provided transfer dir and asset ID.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn get_asset_transfer_dir<P: AsRef<Path>>(
        &self,
        transfer_dir: P,
        asset_id: &str,
    ) -> PathBuf {
        let asset_id_no_prefix = asset_id.replace(ASSET_ID_PREFIX, "");
        transfer_dir.as_ref().join(&asset_id_no_prefix)
    }

    /// Return the consignment file path for a send transfer of an asset.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn get_send_consignment_path<P: AsRef<Path>>(&self, asset_transfer_dir: P) -> PathBuf {
        asset_transfer_dir.as_ref().join(CONSIGNMENT_FILE)
    }
}
