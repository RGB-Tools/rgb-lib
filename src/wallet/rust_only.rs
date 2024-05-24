//! RGB Rust-only methods module
//!
//! This module defines additional utility methods that are not exposed via FFI

use super::*;

/// RGB information to color a transaction
#[derive(Clone, Debug)]
pub struct ColoringInfo {
    /// Contract ID of the transaction to be colored
    pub contract_id: ContractId,
    /// Contract iface
    pub iface: AssetIface,
    /// Input outpoint of the transaction to be colored
    pub input_outpoint: Outpoint,
    /// Map of vouts and asset amounts to color the transaction outputs
    pub output_map: HashMap<u32, u64>,
    /// Static blinding to keep the transaction construction deterministic
    pub static_blinding: u64,
}

impl Wallet {
    /// Color a PSBT.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn color_psbt(
        &self,
        psbt_to_color: &mut PartiallySignedTransaction,
        coloring_info: ColoringInfo,
    ) -> Result<(Fascia, Vec<BuilderSeal<GraphSeal>>), Error> {
        info!(self.logger, "Coloring PSBT...");
        let mut transaction = psbt_to_color.clone().extract_tx();
        let mut psbt = if !transaction
            .output
            .iter()
            .any(|o| o.script_pubkey.is_op_return())
        {
            transaction.output.push(TxOut {
                value: 0,
                script_pubkey: ScriptBuf::new_op_return(&[]),
            });
            PartiallySignedTransaction::from_unsigned_tx(transaction.clone()).unwrap()
        } else {
            psbt_to_color.clone()
        };

        let runtime = self.rgb_runtime()?;

        let mut static_blinding_32_bytes: [u8; 32] = [0; 32];
        static_blinding_32_bytes[0..8]
            .copy_from_slice(&coloring_info.static_blinding.to_le_bytes());
        let static_blinding_factor = BlindingFactor::try_from(static_blinding_32_bytes).unwrap();

        let mut asset_transition_builder = runtime.transition_builder(
            coloring_info.contract_id,
            coloring_info.iface.to_typename(),
            None::<&str>,
        )?;
        let assignment_id = asset_transition_builder
            .assignments_type(&FieldName::from("assetOwner"))
            .ok_or(InternalError::Unexpected)?;

        let mut beneficiaries = vec![];
        for (vout, amount) in coloring_info.output_map {
            if amount == 0 {
                continue;
            }
            let graph_seal = GraphSeal::with_blinded_vout(
                CloseMethod::OpretFirst,
                vout,
                coloring_info.static_blinding,
            );
            let seal = BuilderSeal::Revealed(XChain::with(Layer1::Bitcoin, graph_seal));
            beneficiaries.push(seal);
            asset_transition_builder = asset_transition_builder.add_fungible_state_raw(
                assignment_id,
                seal,
                amount,
                static_blinding_factor,
            )?;
        }

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
        for ((opout, _), state) in
            runtime.state_for_outpoints(coloring_info.contract_id, prev_outputs.iter().copied())?
        {
            asset_transition_builder = asset_transition_builder.add_input(opout, state)?;
        }
        let transition = asset_transition_builder.complete_transition()?;

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
        opreturn_output.set_mpc_entropy(coloring_info.static_blinding);

        let inputs: &[XOutputSeal; 1] = &[XChain::with(
            Layer1::Bitcoin,
            ExplicitSeal::new(
                CloseMethod::OpretFirst,
                RgbOutpoint::new(
                    RgbTxid::from_str(&coloring_info.input_outpoint.txid).unwrap(),
                    coloring_info.input_outpoint.vout,
                ),
            ),
        )];
        for (input, txin) in psbt.inputs.iter_mut().zip(&psbt.unsigned_tx.input) {
            let prevout = txin.previous_output;
            let outpoint = RgbOutpoint::new(prevout.txid.to_byte_array().into(), prevout.vout);
            let output = XChain::with(
                Layer1::Bitcoin,
                ExplicitSeal::new(CloseMethod::OpretFirst, outpoint),
            );
            if inputs.contains(&output) {
                input.set_rgb_consumer(coloring_info.contract_id, transition.id())?;
            }
        }
        psbt.push_rgb_transition(transition, CloseMethodSet::OpretFirst)?;

        let mut rgb_psbt = RgbPsbt::from_str(&psbt.to_string()).unwrap();
        rgb_psbt.complete_construction();
        let fascia = rgb_psbt
            .rgb_commit()
            .map_err(|_| InternalError::Unexpected)?;

        *psbt_to_color = PartiallySignedTransaction::from_str(&rgb_psbt.to_string()).unwrap();

        info!(self.logger, "Color PSBT completed");
        Ok((fascia, beneficiaries))
    }

    /// Color a PSBT, consume the RGB fascia and return the related consignment.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn color_psbt_and_consume(
        &self,
        psbt_to_color: &mut PartiallySignedTransaction,
        coloring_info: ColoringInfo,
    ) -> Result<RgbTransfer, Error> {
        info!(self.logger, "Coloring PSBT and consuming...");
        let (fascia, beneficiaries) = self.color_psbt(psbt_to_color, coloring_info.clone())?;

        let mut runtime = self.rgb_runtime()?;
        runtime.consume(fascia)?;

        let rgb_psbt = RgbPsbt::from_str(&psbt_to_color.to_string()).unwrap();
        let witness_txid = rgb_psbt.txid();
        let mut beneficiaries_outputs = vec![];
        let mut beneficiaries_secret_seals = vec![];
        for beneficiary in beneficiaries {
            match beneficiary {
                BuilderSeal::Revealed(seal) => {
                    beneficiaries_outputs.push(XChain::Bitcoin(ExplicitSeal::new(
                        CloseMethod::OpretFirst,
                        RgbOutpoint::new(witness_txid, seal.as_reduced_unsafe().vout),
                    )))
                }
                BuilderSeal::Concealed(seal) => beneficiaries_secret_seals.push(seal),
            };
        }
        let mut transfer = runtime.transfer(
            coloring_info.contract_id,
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

        *psbt_to_color = PartiallySignedTransaction::from_str(&rgb_psbt.to_string()).unwrap();

        info!(self.logger, "Color PSBT and consume completed");
        Ok(transfer)
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
        static_blinding: u64,
        force: bool,
    ) -> Result<(RgbTransfer, u64), Error> {
        info!(self.logger, "Accepting transfer...");
        let proxy_url = TransportEndpoint::try_from(consignment_endpoint)?.endpoint;

        let consignment_res = self.get_consignment(&proxy_url, txid.clone())?;
        let consignment_bytes = general_purpose::STANDARD
            .decode(consignment_res.consignment)
            .map_err(InternalError::from)?;
        let consignment = RgbTransfer::load(&consignment_bytes[..]).map_err(InternalError::from)?;

        let schema_id = consignment.schema_id().to_string();
        match AssetSchema::from_schema_id(schema_id.clone()) {
            Ok(AssetSchema::Nia) => {}
            _ => return Err(Error::UnknownRgbSchema { schema_id }),
        }

        let mut runtime = self.rgb_runtime()?;

        let blind_seal = BlindSeal::with_blinding(
            CloseMethod::OpretFirst,
            TxPtr::WitnessTx,
            vout,
            static_blinding,
        );
        let graph_seal = GraphSeal::from(blind_seal);
        let funding_seal = XChain::with(Layer1::Bitcoin, graph_seal);
        runtime.store_seal_secret(funding_seal)?;

        let validated_transfer = match consignment
            .clone()
            .validate(&mut self.blockchain_resolver()?, self.testnet())
        {
            Ok(consignment) => consignment,
            Err(consignment) => consignment,
        };
        let validation_status = validated_transfer.clone().into_validation_status().unwrap();
        let validity = validation_status.validity();
        if ![
            Validity::Valid,
            Validity::UnminedTerminals,
            Validity::UnresolvedTransactions,
        ]
        .contains(&validity)
        {
            return Err(Error::InvalidConsignment);
        }

        let mut minimal_contract = consignment.clone().into_contract();
        minimal_contract.bundles = none!();
        minimal_contract.terminals = none!();
        let minimal_contract_validated =
            match minimal_contract.validate(&mut self.blockchain_resolver()?, self.testnet()) {
                Ok(consignment) => consignment,
                Err(consignment) => consignment,
            };
        runtime
            .import_contract(minimal_contract_validated, &mut self.blockchain_resolver()?)
            .expect("failure importing validated contract");

        let (remote_rgb_amount, _not_opret) =
            self.extract_received_amount(&validated_transfer, txid, Some(vout), None);

        let _status =
            runtime.accept_transfer(validated_transfer, &mut self.blockchain_resolver()?, force)?;

        info!(self.logger, "Accept transfer completed");
        Ok((consignment, remote_rgb_amount))
    }

    /// Consume an RGB fascia.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing</div>
    pub fn consume_fascia(&self, fascia: Fascia) -> Result<(), Error> {
        info!(self.logger, "Consuming fascia...");
        self.rgb_runtime()?.consume(fascia.clone())?;
        info!(self.logger, "Consume fascia completed");
        Ok(())
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
        asset_schema: &AssetSchema,
        contract_id: ContractId,
        contract: Option<Contract>,
    ) -> Result<(), Error> {
        info!(self.logger, "Saving new asset...");
        let mut runtime = self.rgb_runtime()?;
        let contract = if let Some(contract) = contract {
            contract
        } else {
            runtime.export_contract(contract_id)?
        };
        let contract_iface = self.get_contract_iface(&mut runtime, asset_schema, contract_id)?;

        let timestamp = contract.genesis.timestamp;
        let (name, precision, issued_supply, ticker, details, media_idx, token) =
            match &asset_schema {
                AssetSchema::Nia => {
                    let iface_nia = Rgb20::from(contract_iface.clone());
                    let spec = iface_nia.spec();
                    let ticker = spec.ticker().to_string();
                    let name = spec.name().to_string();
                    let details = spec.details().map(|d| d.to_string());
                    let precision = spec.precision.into();
                    let issued_supply = iface_nia.total_issued_supply().into();
                    let media_idx = if let Some(attachment) = iface_nia.contract_terms().media {
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
                    let iface_uda = Rgb21::from(contract_iface.clone());
                    let spec = iface_uda.spec();
                    let ticker = spec.ticker().to_string();
                    let name = spec.name().to_string();
                    let details = spec.details().map(|d| d.to_string());
                    let precision = spec.precision.into();
                    let issued_supply = 1;
                    let token_full = self.get_uda_token(contract_iface.clone())?;
                    (
                        name,
                        precision,
                        issued_supply,
                        Some(ticker),
                        details,
                        None,
                        token_full,
                    )
                }
                AssetSchema::Cfa => {
                    let iface_cfa = Rgb25::from(contract_iface.clone());
                    let name = iface_cfa.name().to_string();
                    let details = iface_cfa.details().map(|d| d.to_string());
                    let precision = iface_cfa.precision().into();
                    let issued_supply = iface_cfa.total_issued_supply().into();
                    let media_idx = if let Some(attachment) = iface_cfa.contract_terms().media {
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
            };

        let db_asset = self.add_asset_to_db(
            contract_id.to_string(),
            asset_schema,
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

    /// Broadcast the given transaction.
    ///
    /// <div class="warning">This method is meant for special usage and is normally not needed, use
    /// it only if you know what you're doing.</div>
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn broadcast_tx(&self, tx: BdkTransaction) -> Result<BdkTransaction, Error> {
        let txid = tx.txid().to_string();
        self.bdk_blockchain()?.broadcast(&tx).map_err(|e| {
            let mut min_fee_not_met = false;
            match self.indexer() {
                #[cfg(feature = "electrum")]
                Indexer::Electrum(_) => {
                    let err_str = e.to_string();
                    if err_str.contains("min relay fee not met")
                        || err_str.contains("mempool min fee not met")
                    {
                        min_fee_not_met = true;
                    }
                }
                #[cfg(feature = "esplora")]
                Indexer::Esplora(_) => {
                    if let bdk::Error::Esplora(ref box_err) = e {
                        // wait for new esplora_client release to check message
                        if matches!(**box_err, EsploraError::HttpResponse(400)) {
                            min_fee_not_met = true;
                        }
                    }
                }
            }
            if min_fee_not_met {
                Error::MinFeeNotMet { txid: txid.clone() }
            } else {
                Error::FailedBroadcast {
                    details: e.to_string(),
                }
            }
        })?;
        debug!(self.logger, "Broadcasted TX with ID '{}'", txid);

        Ok(tx)
    }

    /// List the Bitcoin unspents of the vanilla wallet, using BDK's objects, filtered by
    /// `min_confirmations`.
    ///
    /// <div class="warning">This method is meant for special usage, for most cases the method
    /// <code>list_unspents</code> is sufficient</div>
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub fn list_unspents_vanilla(
        &self,
        online: Online,
        min_confirmations: u8,
    ) -> Result<Vec<LocalUtxo>, Error> {
        info!(self.logger, "Listing unspents vanilla...");
        self.check_online(online)?;
        self.sync_wallet(&self.bdk_wallet)?;

        let unspents = self.internal_unspents()?;

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
                .collect::<Result<Vec<LocalUtxo>, Error>>()
        } else {
            Ok(unspents.collect())
        };

        info!(self.logger, "List unspents vanilla completed");
        res
    }
}
