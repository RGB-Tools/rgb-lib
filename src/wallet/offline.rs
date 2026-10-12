//! Offline functionality.
//!
//! This module defines the offline wallet methods.

use super::*;

pub(crate) const TRANSFERS_DIR: &str = "transfers";

const CONSIGNMENT_RCV_META_FILE: &str = "rcv_compose.meta.json";
/// File holding the details of a received mint, saved once the wallet has checked it
pub(crate) const MINT_RCV_FILE: &str = "rcv_mint.json";
pub(crate) const CONSIGNMENT_RCV_FILE: &str = "rcv_compose.rgbc";

const ASSET_ID_PREFIX: &str = "rgb:";

pub(crate) const UDA_FIXED_INDEX: u32 = 0;

pub(crate) const MAX_ATTACHMENTS: usize = 20;

pub(crate) const MAX_TRANSPORT_ENDPOINTS: usize = 3;

pub(crate) const TRANSFER_DATA_FILE: &str = "transfer_data.txt";

pub trait WalletOffline: WalletBackup {
    fn bitcoin_network(&self) -> BitcoinNetwork {
        self.wallet_data().bitcoin_network
    }

    fn chain_net(&self) -> ChainNet {
        self.bitcoin_network().into()
    }

    fn rgb_stock<'txn>(&self, db_txn: &'txn DbTxn) -> Result<RgbStock<'txn>, Error> {
        Ok(RgbStock::new(db_txn))
    }

    fn media_dir(&self) -> PathBuf {
        self.wallet_dir().join(MEDIA_DIR)
    }

    fn get_transfers_dir(&self) -> PathBuf {
        self.wallet_dir().join(TRANSFERS_DIR)
    }

    fn get_transfer_dir(&self, transfer_id: &str) -> PathBuf {
        self.get_transfers_dir().join(transfer_id)
    }

    fn get_asset_transfer_dir<P: AsRef<Path>>(&self, transfer_dir: P, asset_id: &str) -> PathBuf {
        let asset_id_no_prefix = asset_id.replace(ASSET_ID_PREFIX, "");
        transfer_dir.as_ref().join(&asset_id_no_prefix)
    }

    fn max_allocations_per_utxo(&self) -> u32 {
        self.wallet_data().max_allocations_per_utxo
    }

    fn supports_schema(&self, asset_schema: &AssetSchema) -> bool {
        self.wallet_data().supported_schemas.contains(asset_schema)
    }

    fn check_schema_support(&self, asset_schema: &AssetSchema) -> Result<(), Error> {
        if !self.supports_schema(asset_schema) {
            return Err(Error::UnsupportedSchema {
                asset_schema: *asset_schema,
            });
        }
        Ok(())
    }

    fn check_transport_endpoints(&self, transport_endpoints: &[String]) -> Result<(), Error> {
        if transport_endpoints.iter().any(String::is_empty) {
            return Err(Error::InvalidTransportEndpoints {
                details: s!("transport endpoints cannot be empty strings"),
            });
        }
        if transport_endpoints.len() > MAX_TRANSPORT_ENDPOINTS {
            return Err(Error::InvalidTransportEndpoints {
                details: format!(
                    "library supports at max {MAX_TRANSPORT_ENDPOINTS} transport endpoints"
                ),
            });
        }

        Ok(())
    }

    fn filter_unspents(&self, keychain: KeychainKind) -> impl Iterator<Item = LocalOutput> + '_ {
        self.bdk_wallet()
            .list_unspent()
            .filter(move |u| u.keychain == keychain)
    }

    fn internal_unspents(&self) -> impl Iterator<Item = LocalOutput> + '_ {
        self.filter_unspents(KeychainKind::Internal)
    }

    fn filter_outputs(&self, keychain: KeychainKind) -> impl Iterator<Item = LocalOutput> + '_ {
        self.bdk_wallet()
            .list_output()
            .filter(move |u| u.keychain == keychain)
    }

    fn get_available_allocations<T>(
        &self,
        unspents: T,
        exclude_utxos: &[Outpoint],
        max_allocations: Option<u32>,
    ) -> Result<Vec<LocalUnspent>, Error>
    where
        T: Into<Vec<LocalUnspent>>,
    {
        let mut mut_unspents = unspents.into();
        mut_unspents
            .iter_mut()
            .for_each(|u| u.rgb_allocations.retain(|a| !a.status.failed()));
        let max_allocs = max_allocations.unwrap_or_else(|| {
            // guaranteed > 0; guard here too to avoid future regression
            self.max_allocations_per_utxo()
                .checked_sub(1)
                .expect("max allocations per UTXO must be greater than 0")
        });
        Ok(mut_unspents
            .iter()
            .filter(|u| u.utxo.exists)
            .filter(|u| !u.utxo.pending_witness)
            .filter(|u| !exclude_utxos.contains(&u.utxo.outpoint()))
            .filter(|u| {
                (u.rgb_allocations.len() as u32) + u.pending_blinded <= max_allocs
                    && !u.rgb_allocations.iter().any(|a| {
                        !a.incoming && (a.status.initiated() || a.status.waiting_counterparty())
                    })
            })
            .cloned()
            .collect())
    }

    fn get_utxo(
        &self,
        txn: &DbTxn,
        exclude_utxos: &[Outpoint],
        unspents: Option<&[LocalUnspent]>,
        pending_operation: bool,
        max_allocations: Option<u32>,
    ) -> Result<DbTxo, Error> {
        let rgb_allocations = if unspents.is_none() {
            let unspent_txos = txn.get_unspent_txos(vec![])?;
            Some(txn.get_rgb_allocations(unspent_txos, None, None, None, None)?)
        } else {
            None
        };
        let unspents: &[LocalUnspent] = match unspents {
            Some(u) => u,
            None => rgb_allocations.as_deref().unwrap(),
        };

        // exclude UTXOs reserved to be spent by a pending operation
        let mut exclude_utxos = exclude_utxos.to_vec();
        exclude_utxos.extend(self.get_reserved_colored_outpoints(txn)?);
        let mut allocatable =
            self.get_available_allocations(unspents, &exclude_utxos, max_allocations)?;
        allocatable.sort_by_key(|t| t.rgb_allocations.len() + t.pending_blinded as usize);
        match allocatable.first() {
            Some(mut selected) => {
                if allocatable.len() > 1 && !selected.rgb_allocations.is_empty() {
                    let filtered_allocatable: Vec<&LocalUnspent> = if pending_operation {
                        allocatable
                            .iter()
                            .filter(|t| t.rgb_allocations.iter().any(|a| a.future()))
                            .collect()
                    } else {
                        allocatable
                            .iter()
                            .filter(|t| t.rgb_allocations.iter().all(|a| !a.future()))
                            .collect()
                    };
                    if let Some(other) = filtered_allocatable.first() {
                        selected = other;
                    }
                }
                Ok(selected.clone().utxo)
            }
            None => Err(Error::InsufficientAllocationSlots),
        }
    }

    fn save_transfer_transport_endpoint(
        &self,
        txn: &DbTxn,
        transfer_idx: i32,
        transport_endpoint: &LocalTransportEndpoint,
    ) -> Result<(), Error> {
        let transport_endpoint_idx =
            match txn.get_transport_endpoint(transport_endpoint.endpoint.clone())? {
                Some(ce) => ce.idx,
                None => txn.set_transport_endpoint(DbTransportEndpointActMod {
                    transport_type: ActiveValue::Set(transport_endpoint.transport_type),
                    endpoint: ActiveValue::Set(transport_endpoint.endpoint.clone()),
                    ..Default::default()
                })?,
            };

        txn.set_transfer_transport_endpoint(DbTransferTransportEndpointActMod {
            transfer_idx: ActiveValue::Set(transfer_idx),
            transport_endpoint_idx: ActiveValue::Set(transport_endpoint_idx),
            used: ActiveValue::Set(transport_endpoint.used),
            ..Default::default()
        })?;

        Ok(())
    }

    /// Persist the sender's own proxy endpoint for a transfer (the target for the recipient's ACK and
    /// the signed-TX release), tagged with the [`TransportEndpointKind::SenderProxy`] role.
    fn save_sender_transport_endpoint(
        &self,
        txn: &DbTxn,
        transfer_idx: i32,
        endpoint: &str,
    ) -> Result<(), Error> {
        let transport_endpoint_idx = match txn.get_transport_endpoint(endpoint.to_string())? {
            Some(ce) => ce.idx,
            None => txn.set_transport_endpoint(DbTransportEndpointActMod {
                transport_type: ActiveValue::Set(TransportType::JsonRpc),
                endpoint: ActiveValue::Set(endpoint.to_string()),
                ..Default::default()
            })?,
        };
        // `used` hard-coded to false, so it never pops up in lookups not filtering for kind
        txn.set_transfer_transport_endpoint(DbTransferTransportEndpointActMod {
            transfer_idx: ActiveValue::Set(transfer_idx),
            transport_endpoint_idx: ActiveValue::Set(transport_endpoint_idx),
            used: ActiveValue::Set(false),
            kind: ActiveValue::Set(TransportEndpointKind::SenderProxy),
            ..Default::default()
        })?;
        Ok(())
    }

    fn check_burn_reason(&self, burn_reason: String) -> Result<BurnReason, Error> {
        let bytes = hex::decode(burn_reason).map_err(|e| Error::InvalidBurnReason {
            details: format!("not a hex string: {e}"),
        })?;
        let bytes: [u8; 32] =
            bytes
                .try_into()
                .map_err(|bytes: Vec<u8>| Error::InvalidBurnReason {
                    details: format!("expected 32 bytes, got {}", bytes.len()),
                })?;
        Ok(BurnReason::from(Bytes32::from(bytes)))
    }

    fn check_contract_address(&self, contract_address: String) -> Result<EvmAddress, Error> {
        let hex_part = contract_address.strip_prefix("0x").ok_or_else(|| {
            Error::InvalidEvmContractAddress {
                details: s!("address must start with 0x"),
            }
        })?;
        if hex_part.len() != 40 || !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::InvalidEvmContractAddress {
                details: s!("address must be 20 hex-encoded bytes"),
            });
        }
        EvmAddress::from_str(hex_part).map_err(|e| Error::InvalidEvmContractAddress {
            details: e.to_string(),
        })
    }

    fn check_details(&self, details: String) -> Result<Details, Error> {
        if details.is_empty() {
            return Err(Error::InvalidDetails {
                details: s!("ident must contain at least one character"),
            });
        }
        Details::from_str(&details).map_err(|e| Error::InvalidDetails {
            details: e.to_string(),
        })
    }

    fn check_name(&self, name: String) -> Result<Name, Error> {
        Name::try_from(name).map_err(|e| Error::InvalidName {
            details: e.to_string(),
        })
    }

    fn check_precision(&self, precision: u8) -> Result<Precision, Error> {
        Precision::try_from(precision).map_err(|_| Error::InvalidPrecision {
            details: s!("precision is too high"),
        })
    }

    fn check_reject_list_url(&self, opid_reject_url: String) -> Result<RejectListUrl, Error> {
        RejectListUrl::try_from(opid_reject_url).map_err(|e| Error::InvalidRejectListUrl {
            details: e.to_string(),
        })
    }

    fn check_ticker(&self, ticker: String) -> Result<Ticker, Error> {
        if ticker.to_ascii_uppercase() != *ticker {
            return Err(Error::InvalidTicker {
                details: s!("ticker needs to be all uppercase"),
            });
        }
        Ticker::try_from(ticker).map_err(|e| Error::InvalidTicker {
            details: e.to_string(),
        })
    }

    fn get_total_issue_amount(&self, amounts: &[u64], allow_empty: bool) -> Result<u64, Error> {
        if amounts.is_empty() && !allow_empty {
            return Err(Error::NoIssuanceAmounts);
        }
        amounts.iter().try_fold(0u64, |acc, x| {
            if *x == 0 {
                return Err(Error::InvalidAmountZero);
            }
            acc.checked_add(*x).ok_or(Error::TooHighIssuanceAmounts)
        })
    }

    fn get_total_inflation_amount(
        &self,
        inflation_amounts: &[u64],
        issued_supply: u64,
    ) -> Result<u64, Error> {
        let total_inflation = inflation_amounts.iter().try_fold(0u64, |acc, x| {
            if *x == 0 {
                return Err(Error::InvalidAmountZero);
            }
            acc.checked_add(*x).ok_or(Error::TooHighInflationAmounts)
        })?;

        if inflation_amounts.is_empty() {
            return Ok(0);
        }

        issued_supply
            .checked_add(total_inflation)
            .ok_or(Error::TooHighInflationAmounts)?;

        Ok(total_inflation)
    }

    fn file_details<P: AsRef<Path>>(
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
        let file_hash = hash_bytes(&file_bytes);
        let mime = FileFormat::from_file(original_file_path.as_ref())?
            .media_type()
            .to_string();
        let media_ty: &'static str = Box::leak(mime.clone().into_boxed_str());
        let media_type = MediaType::with(media_ty);
        let digest = hex::encode(&file_hash);
        let file_path = self.media_dir().join(&digest).to_string_lossy().to_string();
        Ok((
            Attachment {
                ty: media_type,
                digest: Bytes32::try_from(file_hash.as_slice()).unwrap(),
            },
            Media {
                digest,
                mime,
                file_path,
            },
        ))
    }

    fn copy_media_file<P: AsRef<Path>>(
        &self,
        original_file_path: P,
        media: &Media,
    ) -> Result<(), Error> {
        let src = original_file_path.as_ref().to_string_lossy().to_string();
        let dst = media.clone().file_path;
        if src != dst {
            atomic_write_with(Path::new(&dst), |tmp| {
                fs::copy(&src, tmp)?;
                Ok(())
            })?;
        }
        Ok(())
    }

    fn new_asset_terms(&self, text: RicardianContract, media: Option<Attachment>) -> ContractTerms {
        ContractTerms { text, media }
    }

    fn get_blind_seal(&self, outpoint: impl Into<OutPoint>) -> BlindSeal<RgbTxid> {
        let outpoint = outpoint.into();
        BlindSeal::new_random(outpoint.txid, outpoint.vout)
    }

    fn get_builder_seal(&self, outpoint: impl Into<OutPoint>) -> BuilderSeal<BlindSeal<RgbTxid>> {
        BuilderSeal::from(self.get_blind_seal(outpoint))
    }

    fn get_issue_consignment_path(&self, asset_id: &str) -> PathBuf {
        self.wallet_dir().join(ASSETS_DIR).join(asset_id)
    }

    fn import_and_save_contract(
        &self,
        txn: &DbTxn,
        issue_data: &IssueData,
        stock: &mut RgbStock<'_>,
    ) -> Result<DbAsset, Error> {
        stock
            .import_contract(issue_data.valid_contract.clone(), &DumbResolver)
            .expect("failure importing issued contract");

        let asset = self.add_asset_to_db(txn, &issue_data.asset_data)?;
        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::Settled),
            created_at: ActiveValue::Set(issue_data.asset_data.added_at),
            min_confirmations: ActiveValue::Set(0),
            incoming: ActiveValue::Set(true),
            ..Default::default()
        };
        let batch_transfer_idx = txn.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_id: ActiveValue::Set(Some(issue_data.asset_data.asset_id.clone())),
            ..Default::default()
        };
        let asset_transfer_idx = txn.set_asset_transfer(asset_transfer)?;
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            ..Default::default()
        };
        txn.set_transfer(transfer)?;
        for (utxo_idx, assignments) in &issue_data.issue_utxos {
            for assignment in assignments {
                let db_coloring = DbColoringActMod {
                    txo_idx: ActiveValue::Set(*utxo_idx),
                    asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
                    r#type: ActiveValue::Set(ColoringType::Issue),
                    assignment: ActiveValue::Set(assignment.clone()),
                    ..Default::default()
                };
                txn.set_coloring(db_coloring)?;
            }
        }

        Ok(asset)
    }

    fn issue_contract(
        &self,
        builder: ContractBuilder,
    ) -> Result<(String, PathBuf, ValidContract), Error> {
        let valid_contract = builder.issue_contract().expect("issuance should succeed");
        let asset_id = valid_contract.contract_id().to_string();
        let contract_path = self.get_issue_consignment_path(&asset_id);
        atomic_write_with(&contract_path, |tmp| Ok(valid_contract.save_file(tmp)?))?;
        Ok((asset_id, contract_path, valid_contract))
    }

    fn create_nia_contract(
        &self,
        txn: &DbTxn,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<IssueData, Error> {
        let asset_schema = AssetSchema::Nia;

        self.check_schema_support(&asset_schema)?;

        let settled = self.get_total_issue_amount(&amounts, false)?;

        let mut unspents: Vec<LocalUnspent> = txn.get_rgb_allocations(
            txn.get_unspent_txos(txn.iter_txos()?)?,
            None,
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
            ticker: self.check_ticker(ticker.clone())?,
            name: self.check_name(name.clone())?,
            details,
            precision: self.check_precision(precision)?,
        };

        let mut builder = ContractBuilder::with(
            Identity::default(),
            NonInflatableAsset::schema_rules(),
            self.chain_net(),
        )
        .add_global_state("spec", spec.clone())
        .expect("invalid spec")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_global_state(RGB_GLOBAL_ISSUED_SUPPLY, Amount::from(settled))
        .expect("invalid issuedSupply");

        let mut issue_utxos: HashMap<i32, Vec<Assignment>> = HashMap::new();
        let mut exclude_outpoints = vec![];
        for amount in &amounts {
            let utxo = self.get_utxo(txn, &exclude_outpoints, Some(&unspents), false, None)?;
            exclude_outpoints.push(utxo.outpoint());
            issue_utxos
                .entry(utxo.idx)
                .or_default()
                .push(Assignment::Fungible(*amount));

            builder = builder
                .add_fungible_state(RGB_STATE_ASSET_OWNER, self.get_builder_seal(utxo), *amount)
                .expect("invalid global state data");
        }

        debug!(self.logger(), "Issuing: {issue_utxos:?}");

        let (asset_id, _contract_path, valid_contract) = self.issue_contract(builder)?;

        let asset_data = LocalAssetData {
            asset_id,
            name,
            asset_schema,
            precision,
            ticker: Some(ticker),
            details: spec.details().map(|d| d.to_string()),
            media: None,
            initial_supply: settled,
            max_supply: None,
            known_circulating_supply: None,
            reject_list_url: None,
            token: None,
            timestamp: valid_contract.genesis.timestamp,
            added_at: created_at,
        };

        Ok(IssueData {
            asset_data,
            valid_contract,
            #[cfg(feature = "_online")]
            contract_path: _contract_path,
            issue_utxos,
        })
    }

    fn new_token_data(
        &self,
        index: TokenIndex,
        attachment: &Option<Attachment>,
        attachments: BTreeMap<u8, Attachment>,
    ) -> TokenData {
        TokenData {
            index,
            media: attachment.clone(),
            attachments: Confined::try_from(attachments.clone()).unwrap(),
            ..Default::default()
        }
    }

    fn create_uda_contract(
        &self,
        txn: &DbTxn,
        ticker: String,
        name: String,
        details: Option<String>,
        precision: u8,
        media_file_path: Option<String>,
        attachments_file_paths: Vec<String>,
    ) -> Result<IssueData, Error> {
        let asset_schema = &AssetSchema::Uda;

        self.check_schema_support(asset_schema)?;

        if attachments_file_paths.len() > MAX_ATTACHMENTS {
            return Err(Error::InvalidAttachments {
                details: format!("no more than {MAX_ATTACHMENTS} attachments are supported"),
            });
        }

        let mut unspents: Vec<LocalUnspent> = txn.get_rgb_allocations(
            txn.get_unspent_txos(txn.iter_txos()?)?,
            None,
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

        let details_obj = if let Some(details) = &details {
            Some(self.check_details(details.clone())?)
        } else {
            None
        };
        let ticker_obj = self.check_ticker(ticker.clone())?;
        let spec = AssetSpec {
            ticker: ticker_obj.clone(),
            name: self.check_name(name.clone())?,
            details: details_obj,
            precision: self.check_precision(precision)?,
        };

        let issue_utxo = self.get_utxo(txn, &[], Some(&unspents), false, None)?;
        let issue_utxos: HashMap<i32, Vec<Assignment>> =
            HashMap::from([(issue_utxo.idx, vec![Assignment::NonFungible])]);

        let index = TokenIndex::from_inner(UDA_FIXED_INDEX);

        let token_attachment = if let Some(media_file_path) = &media_file_path {
            let (attach, media) = self.file_details(media_file_path)?;
            self.copy_media_file(media_file_path, &media)?;
            Some(attach)
        } else {
            None
        };

        let mut attachments = BTreeMap::new();
        let mut media_attachments = HashMap::new();
        for (idx, attachment_file_path) in attachments_file_paths.iter().enumerate() {
            let (attach, media) = self.file_details(attachment_file_path)?;
            self.copy_media_file(attachment_file_path, &media)?;
            attachments.insert(idx as u8, attach);
            media_attachments.insert(idx as u8, media);
        }

        let token_data = self.new_token_data(index, &token_attachment, attachments);
        #[cfg(test)]
        let token_data = mock_token_data(token_data);

        let fraction = OwnedFraction::from_inner(1);
        let allocation = Allocation::with(token_data.index, fraction);

        let builder = ContractBuilder::with(
            Identity::default(),
            UniqueDigitalAsset::schema_rules(),
            self.chain_net(),
        )
        .add_global_state("spec", spec)
        .expect("invalid spec")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_data(
            RGB_STATE_ASSET_OWNER,
            self.get_builder_seal(issue_utxo.clone()),
            allocation,
        )
        .expect("invalid global state data")
        .add_global_state("tokens", token_data.clone())
        .expect("invalid tokens");

        debug!(self.logger(), "Issuing: {issue_utxos:?}");

        let (asset_id, _contract_path, valid_contract) = self.issue_contract(builder)?;

        let asset_data = LocalAssetData {
            asset_id: asset_id.clone(),
            name,
            asset_schema: *asset_schema,
            precision,
            ticker: Some(ticker),
            details,
            media: None,
            initial_supply: 1,
            max_supply: None,
            known_circulating_supply: None,
            reject_list_url: None,
            token: Some(Token::from_token_data(&token_data, self.media_dir())),
            timestamp: valid_contract.genesis.timestamp,
            added_at: created_at,
        };

        Ok(IssueData {
            asset_data,
            valid_contract,
            #[cfg(feature = "_online")]
            contract_path: _contract_path,
            issue_utxos,
        })
    }

    fn create_cfa_contract(
        &self,
        txn: &DbTxn,
        name: String,
        details: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<IssueData, Error> {
        let asset_schema = &AssetSchema::Cfa;

        self.check_schema_support(asset_schema)?;

        let settled = self.get_total_issue_amount(&amounts, false)?;

        let mut unspents: Vec<LocalUnspent> = txn.get_rgb_allocations(
            txn.get_unspent_txos(txn.iter_txos()?)?,
            None,
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
        let (attachment, media) = if let Some(file_path) = &file_path {
            let (attachment, media) = self.file_details(file_path)?;
            self.copy_media_file(file_path, &media)?;
            (Some(attachment), Some(media))
        } else {
            (None, None)
        };
        let terms = ContractTerms {
            text,
            media: attachment.clone(),
        };
        let precision_state = self.check_precision(precision)?;
        let name_state = self.check_name(name.clone())?;

        let mut builder = ContractBuilder::with(
            Identity::default(),
            CollectibleFungibleAsset::schema_rules(),
            self.chain_net(),
        )
        .add_global_state("name", name_state)
        .expect("invalid name")
        .add_global_state("precision", precision_state)
        .expect("invalid precision")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_global_state(RGB_GLOBAL_ISSUED_SUPPLY, Amount::from(settled))
        .expect("invalid issuedSupply");

        if let Some(details) = &details {
            builder = builder
                .add_global_state("details", self.check_details(details.clone())?)
                .expect("invalid details");
        };

        let mut issue_utxos: HashMap<i32, Vec<Assignment>> = HashMap::new();
        let mut exclude_outpoints = vec![];
        for amount in &amounts {
            let utxo = self.get_utxo(txn, &exclude_outpoints, Some(&unspents), false, None)?;
            exclude_outpoints.push(utxo.outpoint());
            issue_utxos
                .entry(utxo.idx)
                .or_default()
                .push(Assignment::Fungible(*amount));

            builder = builder
                .add_fungible_state(RGB_STATE_ASSET_OWNER, self.get_builder_seal(utxo), *amount)
                .expect("invalid global state data");
        }

        debug!(self.logger(), "Issuing: {issue_utxos:?}");

        let (asset_id, _contract_path, valid_contract) = self.issue_contract(builder)?;

        let asset_data = LocalAssetData {
            asset_id: asset_id.clone(),
            name,
            asset_schema: *asset_schema,
            precision,
            ticker: None,
            details,
            media,
            initial_supply: settled,
            max_supply: None,
            known_circulating_supply: None,
            reject_list_url: None,
            token: None,
            timestamp: valid_contract.genesis.timestamp,
            added_at: created_at,
        };

        Ok(IssueData {
            asset_data,
            valid_contract,
            #[cfg(feature = "_online")]
            contract_path: _contract_path,
            issue_utxos,
        })
    }

    fn create_ifa_contract(
        &self,
        txn: &DbTxn,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
        inflation_amounts: Vec<u64>,
        reject_list_url: Option<String>,
    ) -> Result<IssueData, Error> {
        let asset_schema = &AssetSchema::Ifa;

        self.check_schema_support(asset_schema)?;

        let settled = self.get_total_issue_amount(&amounts, true)?;
        let inflation_amt = self.get_total_inflation_amount(&inflation_amounts, settled)?;
        if settled == 0 && inflation_amt == 0 {
            return Err(Error::NoIssuanceAmounts);
        }
        let max_supply = settled + inflation_amt;

        let mut unspents: Vec<LocalUnspent> = txn.get_rgb_allocations(
            txn.get_unspent_txos(txn.iter_txos()?)?,
            None,
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
            ticker: self.check_ticker(ticker.clone())?,
            name: self.check_name(name.clone())?,
            details,
            precision: self.check_precision(precision)?,
        };

        let mut builder = ContractBuilder::with(
            Identity::default(),
            InflatableFungibleAsset::schema_rules(),
            self.chain_net(),
        )
        .add_global_state("spec", spec.clone())
        .expect("invalid spec")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_global_state(RGB_GLOBAL_ISSUED_SUPPLY, Amount::from(settled))
        .expect("invalid issuedSupply")
        .add_global_state("maxSupply", Amount::from(max_supply))
        .expect("invalid maxSupply");
        if let Some(reject_list_url) = &reject_list_url {
            builder = builder
                .add_global_state(
                    RGB_GLOBAL_REJECT_LIST_URL,
                    self.check_reject_list_url(reject_list_url.clone())?,
                )
                .expect("invalid rejectListUrl");
        }

        let mut issue_utxos: HashMap<i32, Vec<Assignment>> = HashMap::new();
        let mut exclude_outpoints: Vec<Outpoint> = vec![];
        for amount in &amounts {
            let utxo = self.get_utxo(txn, &exclude_outpoints, Some(&unspents), false, None)?;
            exclude_outpoints.push(utxo.outpoint());
            issue_utxos
                .entry(utxo.idx)
                .or_default()
                .push(Assignment::Fungible(*amount));

            builder = builder
                .add_fungible_state(RGB_STATE_ASSET_OWNER, self.get_builder_seal(utxo), *amount)
                .expect("invalid global state data");
        }

        for amount in &inflation_amounts {
            let utxo = self.get_utxo(txn, &exclude_outpoints, Some(&unspents), false, Some(0))?;
            exclude_outpoints.push(utxo.outpoint());
            issue_utxos
                .entry(utxo.idx)
                .or_default()
                .push(Assignment::InflationRight(*amount));

            builder = builder
                .add_fungible_state(
                    RGB_STATE_INFLATION_ALLOWANCE,
                    self.get_builder_seal(utxo),
                    *amount,
                )
                .expect("invalid global state data");
        }

        debug!(self.logger(), "Issuing: {issue_utxos:?}");

        let (asset_id, _contract_path, valid_contract) = self.issue_contract(builder)?;

        let asset_data = LocalAssetData {
            asset_id: asset_id.clone(),
            name,
            asset_schema: *asset_schema,
            precision,
            ticker: Some(ticker),
            details: spec.details().map(|d| d.to_string()),
            media: None,
            initial_supply: settled,
            max_supply: Some(max_supply),
            known_circulating_supply: Some(settled),
            reject_list_url,
            token: None,
            timestamp: valid_contract.genesis.timestamp,
            added_at: created_at,
        };

        Ok(IssueData {
            asset_data,
            valid_contract,
            #[cfg(feature = "_online")]
            contract_path: _contract_path,
            issue_utxos,
        })
    }

    fn create_bfa_contract(
        &self,
        txn: &DbTxn,
        ticker: String,
        name: String,
        precision: u8,
        mint_rights: u8,
        chain_id: u64,
        contract_address: String,
        reject_list_address: Option<String>,
    ) -> Result<IssueData, Error> {
        let asset_schema = &AssetSchema::Bfa;

        self.check_schema_support(asset_schema)?;

        if mint_rights == 0 {
            return Err(Error::NoMintRights);
        }

        let db_data = txn.get_db_data(false)?;

        let mut unspents: Vec<LocalUnspent> = txn.get_rgb_allocations(
            txn.get_unspent_txos(db_data.txos.clone())?,
            None,
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
            ticker: self.check_ticker(ticker.clone())?,
            name: self.check_name(name.clone())?,
            details,
            precision: self.check_precision(precision)?,
        };

        let contract_address = self.check_contract_address(contract_address)?;

        let mut builder = ContractBuilder::with(
            Identity::default(),
            BridgedFungibleAsset::schema_rules(),
            self.chain_net(),
        )
        .add_global_state("spec", spec.clone())
        .expect("invalid spec")
        .add_global_state("terms", terms)
        .expect("invalid terms")
        .add_global_state(
            RGB_GLOBAL_BRIDGE_LOCATION,
            BridgeLocation::Evm(EvmContract {
                chain_id,
                address: contract_address,
            }),
        )
        .expect("invalid bridgeLocation");
        if let Some(reject_list_address) = reject_list_address {
            builder = builder
                .add_global_state(
                    RGB_GLOBAL_REJECT_LIST,
                    RejectListLocation::Evm(EvmContract {
                        chain_id,
                        address: self.check_contract_address(reject_list_address)?,
                    }),
                )
                .expect("invalid rejectList");
        }

        let mut issue_utxos: HashMap<i32, Vec<Assignment>> = HashMap::new();
        let mut exclude_outpoints: Vec<Outpoint> = vec![];
        for _ in 0..mint_rights {
            let utxo = self.get_utxo(txn, &exclude_outpoints, Some(&unspents), false, Some(0))?;
            exclude_outpoints.push(utxo.outpoint());
            issue_utxos
                .entry(utxo.idx)
                .or_default()
                .push(Assignment::MintRight);

            builder = builder
                .add_rights(RGB_STATE_MINT_RIGHT, self.get_builder_seal(utxo))
                .expect("invalid global state data");
        }

        debug!(self.logger(), "Issuing: {issue_utxos:?}");

        let (asset_id, _contract_path, valid_contract) = self.issue_contract(builder)?;

        let asset_data = LocalAssetData {
            asset_id: asset_id.clone(),
            name,
            asset_schema: *asset_schema,
            precision,
            ticker: Some(ticker),
            details: spec.details().map(|d| d.to_string()),
            media: None,
            initial_supply: 0,
            max_supply: None,
            known_circulating_supply: None,
            reject_list_url: None,
            token: None,
            timestamp: valid_contract.genesis.timestamp,
            added_at: created_at,
        };

        Ok(IssueData {
            asset_data,
            valid_contract,
            #[cfg(feature = "_online")]
            contract_path: _contract_path,
            issue_utxos,
        })
    }

    /// Convert a single `RgbTransport`-format endpoint (e.g. `rpc://host`) to its HTTP(S) URL, as
    /// needed to build a [`ProxyClient`].
    fn convert_transport_endpoint_url(&self, endpoint: &str) -> Result<String, Error> {
        self.convert_transport_endpoints(std::slice::from_ref(&endpoint.to_string()))?
            .pop()
            .ok_or_else(|| Error::InvalidTransportEndpoint {
                details: s!("empty transport endpoint"),
            })
    }

    // convert from RgbTransport format to TransportEndpoint format
    fn convert_transport_endpoints(
        &self,
        transport_endpoints: &[String],
    ) -> Result<Vec<String>, Error> {
        let mut endpoints: Vec<String> = vec![];
        for endpoint_str in transport_endpoints {
            let rgb_transport = RgbTransport::from_str(endpoint_str)?;
            match &rgb_transport {
                RgbTransport::JsonRpc { .. } => {
                    endpoints.push(
                        TransportEndpoint::try_from(rgb_transport)
                            .map_err(|e| Error::InvalidTransportEndpoint {
                                details: e.to_string(),
                            })?
                            .endpoint
                            .clone(),
                    );
                }
                _ => {
                    return Err(Error::UnsupportedTransportType);
                }
            }
        }
        Ok(endpoints)
    }

    #[allow(clippy::too_many_arguments)]
    fn create_receive_data(
        &mut self,
        txn: &DbTxn,
        #[cfg_attr(not(feature = "_online"), allow(unused_variables))] online: Option<Online>,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: i64,
        transport_endpoints: Vec<String>,
        recipient_type: RecipientType,
        encryption: Option<Encryption>,
    ) -> Result<ReceiveDataInternal, Error> {
        let encryption = encryption.unwrap_or(Encryption::Required);

        // no need to be online if an out-of-band exchange is requested
        let out_of_band = transport_endpoints.is_empty();
        #[cfg(not(feature = "_online"))]
        if !out_of_band {
            return Err(Error::Offline);
        }
        #[cfg(feature = "_online")]
        if let Some(online) = online {
            self.check_online(online)?;
        } else if !out_of_band {
            return Err(Error::OnlineNeeded);
        }

        let (beneficiary, recipient_type_full, blind_seal, script_pubkey) = match recipient_type {
            RecipientType::Blind => {
                let mut unspents: Vec<LocalUnspent> =
                    txn.get_rgb_allocations(txn.get_unspent_txos(vec![])?, None, None, None, None)?;
                unspents.retain(|u| {
                    !(u.rgb_allocations
                        .iter()
                        .any(|a| !a.incoming && a.status.waiting_counterparty()))
                });
                let utxo = self.get_utxo(txn, &[], Some(&unspents), true, None)?;
                let unblinded_utxo = utxo.outpoint();
                debug!(
                    self.logger(),
                    "Blinding outpoint '{}'",
                    unblinded_utxo.to_string()
                );
                let blind_seal = self.get_blind_seal(utxo.clone()).transmutate();
                let beneficiary = Beneficiary::BlindedSeal(blind_seal.conceal());
                let recipient_type_full = RecipientTypeFull::Blind { unblinded_utxo };
                (beneficiary, recipient_type_full, Some(blind_seal), None)
            }
            RecipientType::Witness => {
                let script_pubkey = self.get_new_address()?.script_pubkey();
                let beneficiary = beneficiary_from_script_buf(script_pubkey.clone());
                let recipient_type_full = RecipientTypeFull::Witness { vout: None };
                (beneficiary, recipient_type_full, None, Some(script_pubkey))
            }
        };

        #[cfg(test)]
        let network = mock_chain_net(self);
        #[cfg(not(test))]
        let network: ChainNet = self.bitcoin_network().into();

        let beneficiary = XChainNet::with(network, beneficiary);
        let recipient_id = beneficiary.to_string();
        debug!(self.logger(), "Recipient ID: {recipient_id}");
        let (schema, contract_id) = if let Some(aid) = asset_id.clone() {
            let asset = txn.check_asset_exists(aid.clone())?;
            let contract_id = ContractId::from_str(&aid).expect("invalid contract ID");
            (Some(asset.schema), Some(contract_id))
        } else {
            (None, None)
        };

        let endpoints = if out_of_band {
            vec![]
        } else {
            self.check_transport_endpoints(&transport_endpoints)?;
            let mut transport_endpoints_dedup = transport_endpoints.clone();
            transport_endpoints_dedup.sort();
            transport_endpoints_dedup.dedup();
            if transport_endpoints_dedup.len() != transport_endpoints.len() {
                return Err(Error::InvalidTransportEndpoints {
                    details: s!("no duplicate transport endpoints allowed"),
                });
            }
            self.convert_transport_endpoints(&transport_endpoints)?
        };

        let mut invoice_builder = RgbInvoiceBuilder::new(beneficiary);
        if let Some(schema) = schema {
            invoice_builder = invoice_builder.set_schema(schema.into());
        }
        if let Some(contract_id) = contract_id {
            invoice_builder = invoice_builder.set_contract(contract_id);
        }
        if !out_of_band {
            let transports: Vec<&str> = transport_endpoints.iter().map(AsRef::as_ref).collect();
            invoice_builder = invoice_builder.add_transports(transports).unwrap();
        }
        let detected_assignment = match (&assignment, schema) {
            (Assignment::Any, Some(schema)) => schema.any_assignment(),
            (Assignment::Any, None) => Assignment::Any,
            (_, Some(schema)) if !schema.supports_assignment(&assignment) => {
                return Err(Error::InvalidAssignment);
            }
            (_, Some(_)) | (Assignment::Fungible(_), None) => assignment.clone(),
            // without a schema only an amount can be requested
            (_, None) => return Err(Error::InvalidAssignment),
        };
        // the amount comes from the request, so it's left open when narrowing down "any"
        match &assignment {
            Assignment::Fungible(amt) | Assignment::InflationRight(amt) => {
                invoice_builder = invoice_builder.set_amount_raw(*amt);
            }
            Assignment::MintRight => invoice_builder = invoice_builder.set_void(),
            Assignment::NonFungible | Assignment::Any => {}
        }
        if let Some(state_name) = detected_assignment.state_name() {
            invoice_builder = invoice_builder.set_assignment_name(state_name);
        }
        let created_at = now().unix_timestamp();
        if expiration_timestamp < created_at {
            return Err(Error::InvalidExpiration);
        }
        invoice_builder = invoice_builder.set_expiry_timestamp(expiration_timestamp);

        // generate the per-operation Ed25519 key and embed the public key + encryption preference
        let recipient_sk = gen_ed25519();
        let recipient_vk = recipient_sk.verifying_key();
        // the invoice carries the recipient's public key as a 64-char hex string
        invoice_builder = invoice_builder.set_public_key(recipient_vk.to_bytes());
        invoice_builder = invoice_builder.set_encryption(encryption.into());

        let invoice = invoice_builder.finish();
        let invoice_string = invoice.to_string();

        // for a blind receive reserved on proxies, the invoice expiration plus the longest retention
        // grace among them, until which a transfer to the invoice can still be downloaded
        #[allow(unused_mut)]
        let mut retention_deadline = None;

        // reserve the invoice on the proxy/proxies; skipped for the out-of-band exchange
        #[cfg(feature = "_online")]
        if !out_of_band {
            let recipient_pk = hex::encode(recipient_vk.to_bytes());
            let invoice_id = invoice_id(&recipient_id, &recipient_pk);
            let reservation_expiration = expiration_timestamp.to_string();
            let mut reserved = false;
            // whether a proxy refused the reservation as it is draining
            let mut draining = false;
            for endpoint in &endpoints {
                // skip endpoints that are unreachable or run an unsupported protocol version
                let Ok(info) = check_proxy(endpoint) else {
                    continue;
                };
                let proxy_client = ProxyClient::new(endpoint)?;
                // only needed by proxies with proxy-level authentication enabled
                let signature = self.online_data().as_ref().and_then(|online_data| {
                    online_data.proxy_auth_signature(
                        endpoint,
                        &prepare_invoice_auth_message(
                            &invoice_id,
                            &reservation_expiration,
                            &recipient_pk,
                        ),
                    )
                });
                match proxy_client.prepare_invoice(
                    &invoice_id,
                    &reservation_expiration,
                    &recipient_pk,
                    signature,
                ) {
                    Ok(res) if res.error.is_none() => {
                        reserved = true;
                        if matches!(recipient_type_full, RecipientTypeFull::Blind { .. }) {
                            let deadline =
                                expiration_timestamp + i64::from(proxy_retention_grace(&info));
                            retention_deadline =
                                Some(max(retention_deadline.unwrap_or(deadline), deadline));
                        }
                    }
                    Ok(res) => match map_proxy_error(res.error.unwrap()) {
                        // a draining proxy, the invoice can be reserved on the other ones
                        Error::ProxyDraining => draining = true,
                        // a definite proxy-side rejection (e.g. invoice already reserved) is fatal
                        e => return Err(e),
                    },
                    Err(_) => continue,
                }
            }
            if !reserved {
                return Err(if draining {
                    Error::ProxyDraining
                } else {
                    Error::NoValidTransportEndpoint
                });
            }
        }

        Ok(ReceiveDataInternal {
            asset_id,
            detected_assignment,
            invoice_string,
            recipient_id,
            endpoints,
            created_at,
            expiration_timestamp,
            recipient_type_full,
            blind_seal,
            script_pubkey,
            operation_secret_key: recipient_sk.to_bytes().to_vec(),
            recipient_pubkey: recipient_vk.to_bytes().to_vec(),
            encryption,
            retention_deadline,
        })
    }

    fn store_receive_transfer(
        &self,
        txn: &DbTxn,
        receive_data_internal: &ReceiveDataInternal,
        min_confirmations: u8,
    ) -> Result<i32, Error> {
        let batch_transfer = DbBatchTransferActMod {
            status: ActiveValue::Set(TransferStatus::WaitingCounterparty),
            expiration: ActiveValue::Set(Some(receive_data_internal.expiration_timestamp)),
            created_at: ActiveValue::Set(receive_data_internal.created_at),
            min_confirmations: ActiveValue::Set(min_confirmations),
            incoming: ActiveValue::Set(true),
            operation_secret_key: ActiveValue::Set(Some(
                receive_data_internal.operation_secret_key.clone(),
            )),
            retention_deadline: ActiveValue::Set(receive_data_internal.retention_deadline),
            ..Default::default()
        };
        let batch_transfer_idx = txn.set_batch_transfer(batch_transfer)?;
        let asset_transfer = DbAssetTransferActMod {
            user_driven: ActiveValue::Set(true),
            batch_transfer_idx: ActiveValue::Set(batch_transfer_idx),
            asset_id: ActiveValue::Set(receive_data_internal.asset_id.clone()),
            ..Default::default()
        };
        let asset_transfer_idx = txn.set_asset_transfer(asset_transfer)?;
        let transfer = DbTransferActMod {
            asset_transfer_idx: ActiveValue::Set(asset_transfer_idx),
            requested_assignment: ActiveValue::Set(Some(
                receive_data_internal.detected_assignment.clone(),
            )),
            recipient_id: ActiveValue::Set(Some(receive_data_internal.recipient_id.clone())),
            recipient_type: ActiveValue::Set(Some(
                receive_data_internal.recipient_type_full.clone(),
            )),
            invoice_string: ActiveValue::Set(Some(receive_data_internal.invoice_string.clone())),
            recipient_pubkey: ActiveValue::Set(Some(
                receive_data_internal.recipient_pubkey.clone(),
            )),
            recipient_encryption: ActiveValue::Set(receive_data_internal.encryption),
            ..Default::default()
        };
        let transfer_idx = txn.set_transfer(transfer)?;
        for endpoint in &receive_data_internal.endpoints {
            self.save_transfer_transport_endpoint(
                txn,
                transfer_idx,
                &LocalTransportEndpoint {
                    endpoint: endpoint.clone(),
                    transport_type: TransportType::JsonRpc,
                    used: false,
                    usable: true,
                },
            )?;
        }

        if let Some(secret_seal) = receive_data_internal.blind_seal {
            self.rgb_stock(txn)?.store_secret_seal(secret_seal)?;
        }

        if let Some(script_pubkey) = &receive_data_internal.script_pubkey {
            txn.set_pending_witness_script(DbPendingWitnessScriptActMod {
                script: ActiveValue::Set(script_pubkey.to_hex_string()),
                ..Default::default()
            })?;
        }

        Ok(batch_transfer_idx)
    }

    fn finalize_psbt_impl(
        &self,
        signed_psbt: &mut Psbt,
        sign_options: Option<SignOptions>,
    ) -> Result<(), Error> {
        let sign_options = sign_options.unwrap_or_default();
        if !self
            .bdk_wallet()
            .finalize_psbt(signed_psbt, sign_options)
            .map_err(InternalError::from)?
        {
            return Err(Error::CannotFinalizePsbt);
        }
        Ok(())
    }

    fn delete_batch_transfer(
        &self,
        txn: &DbTxn,
        batch_transfer: &DbBatchTransfer,
        asset_transfers: &Vec<DbAssetTransfer>,
        colorings: &[DbColoring],
        txos: &[DbTxo],
    ) -> Result<(), Error> {
        let mut txos_to_delete = HashSet::new();
        for asset_transfer in asset_transfers {
            txn.del_coloring(asset_transfer.idx)?;
            colorings
                .iter()
                .filter(|c| c.asset_transfer_idx == asset_transfer.idx)
                .for_each(|c| {
                    if let Some(txo) = txos.iter().find(|t| !t.exists && t.idx == c.txo_idx) {
                        txos_to_delete.insert(txo.idx);
                    }
                });
        }
        for txo in txos_to_delete {
            txn.del_txo(txo)?;
        }
        txn.del_batch_transfer(batch_transfer)
    }

    fn delete_transfers_impl(
        &self,
        txn: &DbTxn,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        let db_data = txn.get_db_data(true)?;
        let mut transfers_changed = false;

        if let Some(batch_transfer_idx) = batch_transfer_idx {
            let batch_transfer =
                txn.get_batch_transfer_or_fail(batch_transfer_idx, &db_data.batch_transfers)?;

            if !batch_transfer.failed() {
                return Err(Error::CannotDeleteBatchTransfer);
            }

            let asset_transfers = batch_transfer.get_asset_transfers(&db_data.asset_transfers);

            if no_asset_only {
                let connected_assets = asset_transfers.iter().any(|t| t.asset_id.is_some());
                if connected_assets {
                    return Err(Error::CannotDeleteBatchTransfer);
                }
            }

            transfers_changed = true;
            self.delete_batch_transfer(
                txn,
                &batch_transfer,
                &asset_transfers,
                &db_data.colorings,
                &db_data.txos,
            )?
        } else {
            // delete all failed transfers
            let mut batch_transfers: Vec<DbBatchTransfer> = db_data
                .batch_transfers
                .clone()
                .into_iter()
                .filter(|t| t.failed())
                .collect();
            for batch_transfer in batch_transfers.iter_mut() {
                let asset_transfers = batch_transfer.get_asset_transfers(&db_data.asset_transfers);
                if no_asset_only {
                    let connected_assets = asset_transfers.iter().any(|t| t.asset_id.is_some());
                    if connected_assets {
                        continue;
                    }
                }
                transfers_changed = true;
                self.delete_batch_transfer(
                    txn,
                    batch_transfer,
                    &asset_transfers,
                    &db_data.colorings,
                    &db_data.txos,
                )?
            }
        }

        Ok(transfers_changed)
    }

    fn get_new_addresses(
        &mut self,
        keychain: KeychainKind,
        _count: u32,
    ) -> Result<BdkAddress, Error> {
        Ok(self.bdk_wallet_mut().reveal_next_address(keychain).address)
    }

    fn get_new_address(&mut self) -> Result<BdkAddress, Error> {
        self.get_new_addresses(KeychainKind::External, 1)
    }

    fn get_asset_balance_impl(&self, txn: &DbTxn, asset_id: String) -> Result<Balance, Error> {
        txn.check_asset_exists(asset_id.clone())?;
        txn.get_asset_balance(asset_id, None, None, None, None, None)
    }

    fn get_asset_metadata_impl(&self, txn: &DbTxn, asset_id: String) -> Result<Metadata, Error> {
        let asset = txn.check_asset_exists(asset_id.clone())?;

        let initial_supply = asset.initial_supply.parse::<u64>().unwrap();
        let max_supply = if let Some(max_supply) = asset.max_supply {
            max_supply.parse::<u64>().unwrap()
        } else {
            initial_supply
        };
        let known_circulating_supply =
            if let Some(known_circulating_supply) = asset.known_circulating_supply {
                known_circulating_supply.parse::<u64>().unwrap()
            } else {
                initial_supply
            };
        let token = match asset.schema {
            AssetSchema::Uda => {
                let medias = txn.iter_media()?;
                let tokens = txn.iter_tokens()?;
                let token_medias = txn.iter_token_medias()?;
                if let Some(token_light) =
                    self.get_asset_token(asset.idx, &medias, &tokens, &token_medias)
                {
                    let mut token = Token {
                        index: token_light.index,
                        ticker: token_light.ticker,
                        name: token_light.name,
                        details: token_light.details,
                        embedded_media: None,
                        media: token_light.media,
                        attachments: token_light.attachments,
                        reserves: None,
                    };
                    if token_light.embedded_media || token_light.reserves {
                        let stock = self.rgb_stock(txn)?;
                        let contract_id =
                            ContractId::from_str(&asset_id).expect("invalid contract ID");
                        let contract = stock.contract_wrapper::<UniqueDigitalAsset>(contract_id)?;
                        let uda_token =
                            Token::from_token_data(&contract.token_data(), self.media_dir());
                        token.embedded_media = uda_token.embedded_media;
                        token.reserves = uda_token.reserves;
                    }
                    Some(token)
                } else {
                    None
                }
            }
            AssetSchema::Nia | AssetSchema::Cfa | AssetSchema::Ifa | AssetSchema::Bfa => None,
        };

        Ok(Metadata {
            asset_schema: asset.schema,
            initial_supply,
            max_supply,
            known_circulating_supply,
            timestamp: asset.timestamp,
            name: asset.name,
            precision: asset.precision,
            ticker: asset.ticker,
            details: asset.details,
            token,
            reject_list_url: asset.reject_list_url,
        })
    }

    fn save_token_media(
        &self,
        txn: &DbTxn,
        token_idx: i32,
        digest: String,
        mime: String,
        attachment_id: Option<u8>,
    ) -> Result<(), Error> {
        let media_idx = txn.get_or_insert_media(digest, mime)?;

        txn.set_token_media(DbTokenMediaActMod {
            token_idx: ActiveValue::Set(token_idx),
            media_idx: ActiveValue::Set(media_idx),
            attachment_id: ActiveValue::Set(attachment_id),
            ..Default::default()
        })?;

        Ok(())
    }

    fn add_asset_to_db(&self, txn: &DbTxn, asset_data: &LocalAssetData) -> Result<DbAsset, Error> {
        let media_idx = if let Some(media) = &asset_data.media {
            Some(txn.get_or_insert_media(media.digest.clone(), media.mime.clone())?)
        } else {
            None
        };
        let mut db_asset = DbAssetActMod {
            idx: ActiveValue::NotSet,
            media_idx: ActiveValue::Set(media_idx),
            id: ActiveValue::Set(asset_data.asset_id.clone()),
            schema: ActiveValue::Set(asset_data.asset_schema),
            added_at: ActiveValue::Set(asset_data.added_at),
            details: ActiveValue::Set(asset_data.details.clone()),
            initial_supply: ActiveValue::Set(asset_data.initial_supply.to_string()),
            max_supply: ActiveValue::Set(asset_data.max_supply.map(|s| s.to_string())),
            known_circulating_supply: ActiveValue::Set(
                asset_data.known_circulating_supply.map(|s| s.to_string()),
            ),
            name: ActiveValue::Set(asset_data.name.clone()),
            precision: ActiveValue::Set(asset_data.precision),
            ticker: ActiveValue::Set(asset_data.ticker.clone()),
            timestamp: ActiveValue::Set(asset_data.timestamp),
            reject_list_url: ActiveValue::Set(asset_data.reject_list_url.clone()),
        };
        let idx = txn.set_asset(db_asset.clone())?;
        db_asset.idx = ActiveValue::Set(idx);

        if let Some(ref token) = asset_data.token {
            let db_token = DbTokenActMod {
                asset_idx: ActiveValue::Set(idx),
                index: ActiveValue::Set(token.index),
                ticker: ActiveValue::Set(token.ticker.clone()),
                name: ActiveValue::Set(token.name.clone()),
                details: ActiveValue::Set(token.details.clone()),
                embedded_media: ActiveValue::Set(token.embedded_media.is_some()),
                reserves: ActiveValue::Set(token.reserves.is_some()),
                ..Default::default()
            };
            let token_idx = txn.set_token(db_token)?;

            if let Some(media) = &token.media {
                self.save_token_media(
                    txn,
                    token_idx,
                    media.get_digest(),
                    media.mime.clone(),
                    None,
                )?;
            }
            for (attachment_id, media) in token.attachments.clone() {
                self.save_token_media(
                    txn,
                    token_idx,
                    media.get_digest(),
                    media.mime.clone(),
                    Some(attachment_id),
                )?;
            }
        }

        Ok(db_asset.try_into_model().expect("valid model"))
    }

    fn get_asset_token(
        &self,
        asset_idx: i32,
        medias: &[DbMedia],
        tokens: &[DbToken],
        token_medias: &[DbTokenMedia],
    ) -> Option<TokenLight> {
        if let Some(db_token) = tokens.iter().find(|t| t.asset_idx == asset_idx) {
            let mut media = None;
            let mut attachments = HashMap::new();
            let media_dir = self.media_dir();
            token_medias
                .iter()
                .filter(|tm| tm.token_idx == db_token.idx)
                .for_each(|tm| {
                    let db_media = medias.iter().find(|m| m.idx == tm.media_idx).unwrap();
                    let media_tkn = Media::from_db_media(db_media, &media_dir);
                    if let Some(attachment_id) = tm.attachment_id {
                        attachments.insert(attachment_id, media_tkn);
                    } else {
                        media = Some(media_tkn);
                    }
                });

            Some(TokenLight {
                index: db_token.index,
                ticker: db_token.ticker.clone(),
                name: db_token.name.clone(),
                details: db_token.details.clone(),
                embedded_media: db_token.embedded_media,
                media,
                attachments,
                reserves: db_token.reserves,
            })
        } else {
            None
        }
    }

    fn get_asset_medias(
        &self,
        txn: &DbTxn,
        media_idx: Option<i32>,
        token: Option<TokenLight>,
    ) -> Result<HashSet<Media>, Error> {
        let mut asset_medias = HashSet::new();
        if let Some(media_idx) = media_idx {
            let db_media = txn.get_media(media_idx)?.unwrap();
            asset_medias.insert(Media::from_db_media(&db_media, self.media_dir()));
        }
        if let Some(token) = token {
            if let Some(token_media) = token.media {
                asset_medias.insert(token_media);
            }
            for (_, attachment_media) in token.attachments {
                asset_medias.insert(attachment_media);
            }
        }
        Ok(asset_medias)
    }

    fn get_btc_balance_for_keychain(&self, keychain: KeychainKind) -> Result<Balance, Error> {
        let chain = self.bdk_wallet().local_chain();
        let chain_tip = self.bdk_wallet().latest_checkpoint().block_id();
        let outpoints = self.filter_unspents(keychain).map(|lo| ((), lo.outpoint));
        let balance = self.bdk_wallet().as_ref().balance(
            chain,
            chain_tip,
            CanonicalizationParams::default(),
            outpoints,
            |_, _| false,
        );

        let future = balance.total();
        Ok(Balance {
            settled: balance.confirmed.to_sat(),
            future: future.to_sat(),
            spendable: future
                .to_sat()
                .checked_sub(balance.immature.to_sat())
                .expect("immature balance cannot exceed the total balance"),
        })
    }

    fn get_btc_balance_impl(
        &mut self,
        txn: &DbTxn,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<BtcBalance, Error> {
        self.sync_if_requested(txn, online, skip_sync, KeychainKind::External)?;
        self.sync_if_requested(txn, online, skip_sync, KeychainKind::Internal)?;
        let mut vanilla = self.get_btc_balance_for_keychain(KeychainKind::Internal)?;
        let colored = self.get_btc_balance_for_keychain(KeychainKind::External)?;

        let reserved: HashSet<BdkOutPoint> = self
            .get_reserved_vanilla_outpoints(txn)?
            .into_iter()
            .collect();
        if !reserved.is_empty() {
            let reserved_sum: u64 = self
                .internal_unspents()
                .filter(|u| reserved.contains(&u.outpoint))
                .map(|u| u.txout.value.to_sat())
                .sum();
            vanilla.spendable = vanilla.spendable.saturating_sub(reserved_sum);
        }

        Ok(BtcBalance { vanilla, colored })
    }

    fn extract_asset_data(
        &self,
        stock: &RgbStock<'_>,
        contract_id: ContractId,
        asset_schema: AssetSchema,
        valid_contract: ValidContract,
        valid_transfer: Option<ValidTransfer>,
    ) -> Result<LocalAssetData, Error> {
        let timestamp = valid_contract.genesis.timestamp;
        let added_at = now().unix_timestamp();
        let media_dir = self.media_dir();
        Ok(match &asset_schema {
            AssetSchema::Nia => {
                let contract = stock.contract_wrapper::<NonInflatableAsset>(contract_id)?;
                let spec = contract.spec();
                let ticker = spec.ticker().to_string();
                let name = spec.name().to_string();
                let details = spec.details().map(|d| d.to_string());
                let precision = spec.precision.into();
                let initial_supply = contract.total_issued_supply().into();
                let media = contract
                    .contract_terms()
                    .media
                    .map(|a| Media::from_attachment(&a, media_dir));
                LocalAssetData {
                    asset_id: contract_id.to_string(),
                    name,
                    asset_schema,
                    precision,
                    ticker: Some(ticker),
                    details,
                    media,
                    initial_supply,
                    max_supply: None,
                    known_circulating_supply: None,
                    reject_list_url: None,
                    token: None,
                    timestamp,
                    added_at,
                }
            }
            AssetSchema::Uda => {
                let contract = stock.contract_wrapper::<UniqueDigitalAsset>(contract_id)?;
                let spec = contract.spec();
                let ticker = spec.ticker().to_string();
                let name = spec.name().to_string();
                let details = spec.details().map(|d| d.to_string());
                let precision = spec.precision.into();
                let initial_supply = 1;
                let media = contract
                    .contract_terms()
                    .media
                    .map(|a| Media::from_attachment(&a, media_dir));
                let token_full = Token::from_token_data(&contract.token_data(), self.media_dir());
                LocalAssetData {
                    asset_id: contract_id.to_string(),
                    name,
                    asset_schema,
                    precision,
                    ticker: Some(ticker),
                    details,
                    media,
                    initial_supply,
                    max_supply: None,
                    known_circulating_supply: None,
                    reject_list_url: None,
                    token: Some(token_full),
                    timestamp,
                    added_at,
                }
            }
            AssetSchema::Cfa => {
                let contract = stock.contract_wrapper::<CollectibleFungibleAsset>(contract_id)?;
                let name = contract.name().to_string();
                let details = contract.details().map(|d| d.to_string());
                let precision = contract.precision().into();
                let initial_supply = contract.total_issued_supply().into();
                let media = contract
                    .contract_terms()
                    .media
                    .map(|a| Media::from_attachment(&a, media_dir));
                LocalAssetData {
                    asset_id: contract_id.to_string(),
                    name,
                    asset_schema,
                    precision,
                    ticker: None,
                    details,
                    media,
                    initial_supply,
                    max_supply: None,
                    known_circulating_supply: None,
                    reject_list_url: None,
                    token: None,
                    timestamp,
                    added_at,
                }
            }
            AssetSchema::Ifa => {
                let contract = stock.contract_wrapper::<InflatableFungibleAsset>(contract_id)?;
                let spec = contract.spec();
                let ticker = spec.ticker().to_string();
                let name = spec.name().to_string();
                let details = spec.details().map(|d| d.to_string());
                let precision = spec.precision.into();
                let initial_supply = contract.total_issued_supply().into();
                let media = contract
                    .contract_terms()
                    .media
                    .map(|a| Media::from_attachment(&a, media_dir));
                let max_supply = contract.max_supply().into();
                let known_circulating_supply = if let Some(valid_transfer) = valid_transfer {
                    IfaWrapper::with(stock.consignment_data(&valid_transfer)?)
                        .total_issued_supply()
                        .into()
                } else {
                    initial_supply
                };
                let reject_list_url = contract.reject_list_url().map(|u| u.to_string());
                LocalAssetData {
                    asset_id: contract_id.to_string(),
                    name,
                    asset_schema,
                    precision,
                    ticker: Some(ticker),
                    details,
                    media,
                    initial_supply,
                    max_supply: Some(max_supply),
                    known_circulating_supply: Some(known_circulating_supply),
                    reject_list_url,
                    token: None,
                    timestamp,
                    added_at,
                }
            }
            AssetSchema::Bfa => {
                let contract = stock.contract_wrapper::<BridgedFungibleAsset>(contract_id)?;
                let spec = contract.spec();
                let ticker = spec.ticker().to_string();
                let name = spec.name().to_string();
                let details = spec.details().map(|d| d.to_string());
                let precision = spec.precision.into();
                let media = contract
                    .contract_terms()
                    .media
                    .map(|a| Media::from_attachment(&a, media_dir));
                let reject_list_url = bfa_reject_list_url(contract.reject_list());
                LocalAssetData {
                    asset_id: contract_id.to_string(),
                    name,
                    asset_schema,
                    precision,
                    ticker: Some(ticker),
                    details,
                    media,
                    initial_supply: 0,
                    max_supply: None,
                    known_circulating_supply: None,
                    reject_list_url,
                    token: None,
                    timestamp,
                    added_at,
                }
            }
        })
    }

    fn save_new_asset_internal(
        &self,
        txn: &DbTxn,
        stock: &RgbStock<'_>,
        contract_id: ContractId,
        asset_schema: AssetSchema,
        valid_contract: ValidContract,
        valid_transfer: Option<ValidTransfer>,
    ) -> Result<LocalAssetData, Error> {
        let local_asset_data = self.extract_asset_data(
            stock,
            contract_id,
            asset_schema,
            valid_contract,
            valid_transfer,
        )?;

        let _ = self.add_asset_to_db(txn, &local_asset_data)?;

        Ok(local_asset_data)
    }

    fn get_reserved_vanilla_outpoints(&self, txn: &DbTxn) -> Result<Vec<BdkOutPoint>, Error> {
        Ok(txn
            .iter_reserved_txos()?
            .into_iter()
            .filter(|r| r.reserved_for_vanilla.is_some())
            .map(BdkOutPoint::from)
            .collect())
    }

    fn get_reserved_colored_outpoints(&self, txn: &DbTxn) -> Result<Vec<Outpoint>, Error> {
        Ok(txn
            .iter_reserved_txos()?
            .into_iter()
            .filter(|r| r.reserved_for_colored.is_some())
            .map(Outpoint::from)
            .collect())
    }

    fn get_unspendable_bdk_outpoints(&self, txn: &DbTxn) -> Result<Vec<BdkOutPoint>, Error> {
        let mut outpoints: Vec<BdkOutPoint> = txn
            .iter_txos()?
            .into_iter()
            .map(BdkOutPoint::from)
            .collect();
        outpoints.extend(self.get_reserved_vanilla_outpoints(txn)?);
        Ok(outpoints)
    }

    fn get_script_pubkey(&self, address: &str) -> Result<ScriptBuf, Error> {
        Ok(parse_address_str(address, self.bitcoin_network())?.script_pubkey())
    }

    fn list_assets_impl(
        &self,
        txn: &DbTxn,
        mut filter_asset_schemas: Vec<AssetSchema>,
    ) -> Result<Assets, Error> {
        if filter_asset_schemas.is_empty() {
            filter_asset_schemas = AssetSchema::VALUES.to_vec()
        }

        let batch_transfers = Some(txn.iter_batch_transfers()?);
        let colorings = Some(txn.iter_colorings()?);
        let txos = Some(txn.iter_txos()?);
        let asset_transfers = Some(txn.iter_asset_transfers()?);
        let transfers = Some(txn.iter_transfers()?);
        let medias = Some(txn.iter_media()?);

        let assets = txn.iter_assets()?;
        let mut nia = None;
        let mut uda = None;
        let mut cfa = None;
        let mut ifa = None;
        let mut bfa = None;
        for schema in filter_asset_schemas {
            match schema {
                AssetSchema::Nia => {
                    nia = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetNIA::get_asset_details(
                                    txn,
                                    self,
                                    a,
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                    medias.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetNIA>, Error>>()?,
                    );
                }
                AssetSchema::Uda => {
                    let tokens = txn.iter_tokens()?;
                    let token_medias = txn.iter_token_medias()?;
                    uda = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetUDA::get_asset_details(
                                    txn,
                                    self,
                                    a,
                                    self.get_asset_token(
                                        a.idx,
                                        &medias.clone().unwrap(),
                                        &tokens,
                                        &token_medias,
                                    ),
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                    medias.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetUDA>, Error>>()?,
                    );
                }
                AssetSchema::Cfa => {
                    cfa = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetCFA::get_asset_details(
                                    txn,
                                    self,
                                    a,
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                    medias.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetCFA>, Error>>()?,
                    );
                }
                AssetSchema::Ifa => {
                    ifa = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetIFA::get_asset_details(
                                    txn,
                                    self,
                                    a,
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                    medias.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetIFA>, Error>>()?,
                    );
                }
                AssetSchema::Bfa => {
                    bfa = Some(
                        assets
                            .iter()
                            .filter(|a| a.schema == schema)
                            .map(|a| {
                                AssetBFA::get_asset_details(
                                    txn,
                                    self,
                                    a,
                                    transfers.clone(),
                                    asset_transfers.clone(),
                                    batch_transfers.clone(),
                                    colorings.clone(),
                                    txos.clone(),
                                    medias.clone(),
                                )
                            })
                            .collect::<Result<Vec<AssetBFA>, Error>>()?,
                    );
                }
            }
        }

        Ok(Assets {
            nia,
            uda,
            cfa,
            ifa,
            bfa,
        })
    }

    fn sync_if_requested(
        &mut self,
        #[cfg_attr(not(feature = "_online"), allow(unused_variables))] txn: &DbTxn,
        #[cfg_attr(not(feature = "_online"), allow(unused_variables))] online: Option<Online>,
        skip_sync: bool,
        #[cfg_attr(not(feature = "_online"), allow(unused_variables))] keychain: KeychainKind,
    ) -> Result<(), Error> {
        if !skip_sync {
            #[cfg(not(feature = "_online"))]
            return Err(Error::Offline);
            #[cfg(feature = "_online")]
            {
                if let Some(online) = online {
                    self.check_online(online)?;
                } else {
                    return Err(Error::OnlineNeeded);
                }
                let keychain = match keychain {
                    KeychainKind::Internal => SyncKeychain::Vanilla {
                        lookback: self.vanilla_sync_lookback(),
                    },
                    KeychainKind::External => SyncKeychain::Colored,
                };
                self.sync_wallet(
                    txn,
                    SyncOptions {
                        keychain,
                        strategy: SyncStrategy::FastSync,
                    },
                    false,
                )?;
            }
        }
        Ok(())
    }

    fn list_transactions_impl(
        &mut self,
        txn: &DbTxn,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<Vec<Transaction>, Error> {
        self.sync_if_requested(txn, online, skip_sync, KeychainKind::External)?;
        self.sync_if_requested(txn, online, skip_sync, KeychainKind::Internal)?;

        let mut create_utxos_txids = vec![];
        let mut drain_txids = vec![];
        let mut send_btc_txids = vec![];
        let wallet_transactions = txn.iter_wallet_transactions()?;
        for tx in wallet_transactions {
            match tx.r#type {
                WalletTransactionType::CreateUtxos => create_utxos_txids.push(tx.txid),
                WalletTransactionType::Drain => drain_txids.push(tx.txid),
                WalletTransactionType::SendBtc => send_btc_txids.push(tx.txid),
            }
        }
        let rgb_send_txids: Vec<String> = txn
            .iter_batch_transfers()?
            .into_iter()
            .filter_map(|t| t.txid)
            .collect();
        Ok(self
            .bdk_wallet()
            .transactions_sort_by(|tx1, tx2| tx2.chain_position.cmp(&tx1.chain_position))
            .into_iter()
            .map(|t| {
                let txid = t.tx_node.txid.to_string();
                let transaction_type = if drain_txids.contains(&txid) {
                    TransactionType::Drain
                } else if create_utxos_txids.contains(&txid) {
                    TransactionType::CreateUtxos
                } else if rgb_send_txids.contains(&txid) {
                    TransactionType::RgbSend
                } else if send_btc_txids.contains(&txid) {
                    TransactionType::SendBtc
                } else {
                    TransactionType::Incoming
                };
                let confirmation_time = match t.chain_position {
                    ChainPosition::Confirmed { anchor, .. } => Some(BlockTime {
                        height: anchor.block_id.height,
                        timestamp: anchor.confirmation_time,
                    }),
                    _ => None,
                };
                let (sent, received) = self.bdk_wallet().sent_and_received(&t.tx_node);
                let fee = self.bdk_wallet().calculate_fee(&t.tx_node).unwrap();
                Transaction {
                    transaction_type,
                    txid,
                    received: received.to_sat(),
                    sent: sent.to_sat(),
                    fee: fee.to_sat(),
                    confirmation_time,
                }
            })
            .collect())
    }

    fn normalize_recipient_id(&self, recipient_id: &str) -> String {
        recipient_id.replace(":", "_")
    }

    fn get_receive_consignment_path(&self, recipient_id: &str) -> PathBuf {
        self.get_transfers_dir()
            .join(self.normalize_recipient_id(recipient_id))
            .join(CONSIGNMENT_RCV_FILE)
    }

    fn get_received_mint_path(&self, recipient_id: &str) -> PathBuf {
        self.get_transfers_dir()
            .join(self.normalize_recipient_id(recipient_id))
            .join(MINT_RCV_FILE)
    }

    /// The details of the mint received on `recipient_id`, saved once the wallet has checked it.
    fn read_received_mint(&self, recipient_id: &str) -> Result<Option<ReceivedMint>, Error> {
        let path = self.get_received_mint_path(recipient_id);
        if !path.exists() {
            return Ok(None);
        }
        let received_mint =
            serde_json::from_str(&fs::read_to_string(path)?).map_err(InternalError::from)?;
        Ok(Some(received_mint))
    }

    fn get_received_mint_impl(
        &self,
        txn: &DbTxn,
        recipient_id: &str,
    ) -> Result<Option<ReceivedMint>, Error> {
        // a failed receive has nothing left to lock for
        let db_data = txn.get_db_data(false)?;
        let failed = db_data.transfers.iter().any(|t| {
            t.recipient_id.as_deref() == Some(recipient_id)
                && db_data
                    .asset_transfers
                    .iter()
                    .find(|at| at.idx == t.asset_transfer_idx)
                    .and_then(|at| {
                        db_data
                            .batch_transfers
                            .iter()
                            .find(|bt| bt.idx == at.batch_transfer_idx)
                    })
                    .is_some_and(|bt| bt.incoming && bt.status == TransferStatus::Failed)
        });
        if failed {
            return Ok(None);
        }
        self.read_received_mint(recipient_id)
    }

    fn get_receive_consignment_meta_path(&self, recipient_id: &str) -> PathBuf {
        self.get_transfers_dir()
            .join(self.normalize_recipient_id(recipient_id))
            .join(CONSIGNMENT_RCV_META_FILE)
    }

    /// Path of the validated copy of a received consignment, kept only while the transfer is
    /// pending
    fn get_receive_valid_consignment_path(&self, consignment_path: &Path) -> PathBuf {
        consignment_path.with_extension("valid.rgbc")
    }

    fn send_consignment_path(&self, asset_id: &str, transfer_id: &str) -> PathBuf {
        let transfer_dir = self.get_transfer_dir(transfer_id);
        let asset_transfer_dir = self.get_asset_transfer_dir(transfer_dir, asset_id);
        asset_transfer_dir.join(CONSIGNMENT_FILE)
    }

    fn get_transfer_data(
        &self,
        transfer: &DbTransfer,
        asset_transfer: &DbAssetTransfer,
        batch_transfer: &DbBatchTransfer,
        txos: &[DbTxo],
        colorings: &[DbColoring],
    ) -> Result<TransferData, Error> {
        let filtered_coloring = colorings
            .iter()
            .filter(|&c| c.asset_transfer_idx == asset_transfer.idx)
            .cloned();

        let assignments = filtered_coloring
            .clone()
            .filter(|c| c.r#type != ColoringType::Input)
            .map(|c| c.assignment)
            .collect();

        let kind = if batch_transfer.incoming {
            if filtered_coloring.clone().count() > 0
                && filtered_coloring
                    .clone()
                    .all(|c| c.r#type == ColoringType::Issue)
            {
                TransferKind::Issuance
            } else {
                match transfer.recipient_type.as_ref().unwrap() {
                    RecipientTypeFull::Blind { .. } => TransferKind::ReceiveBlind,
                    RecipientTypeFull::Witness { .. } => TransferKind::ReceiveWitness,
                }
            }
        } else {
            match asset_transfer
                .transition_type
                .expect("outgoing asset transfers record their transition")
            {
                TypeOfTransition::Transfer => TransferKind::Send,
                TypeOfTransition::Inflate => TransferKind::Inflation,
                TypeOfTransition::Burn => TransferKind::Burn,
                TypeOfTransition::Mint => TransferKind::Mint,
            }
        };

        let txo_ids: Vec<i32> = filtered_coloring.clone().map(|c| c.txo_idx).collect();
        let transfer_txos: Vec<DbTxo> = txos
            .iter()
            .filter(|&t| txo_ids.contains(&t.idx))
            .cloned()
            .collect();
        let receive_utxo = match &transfer.recipient_type {
            Some(RecipientTypeFull::Blind { unblinded_utxo }) => Some(unblinded_utxo.clone()),
            Some(RecipientTypeFull::Witness { vout }) => {
                let received_txo_idx: Vec<i32> = filtered_coloring
                    .clone()
                    // issue coloring from inflation is considered as received
                    .filter(|c| [ColoringType::Receive, ColoringType::Issue].contains(&c.r#type))
                    .map(|c| c.txo_idx)
                    .collect();
                transfer_txos
                    .clone()
                    .into_iter()
                    .filter(|t| received_txo_idx.contains(&t.idx) && t.vout == vout.unwrap())
                    .map(|t| t.outpoint())
                    .collect::<Vec<Outpoint>>()
                    .first()
                    .cloned()
            }
            _ => None,
        };
        let change_utxo = match kind {
            TransferKind::ReceiveBlind | TransferKind::ReceiveWitness => None,
            TransferKind::Send
            | TransferKind::Inflation
            | TransferKind::Burn
            | TransferKind::Mint => {
                let change_txo_idx: Vec<i32> = filtered_coloring
                    .filter(|c| c.r#type == ColoringType::Change)
                    .map(|c| c.txo_idx)
                    .collect();
                transfer_txos
                    .into_iter()
                    .filter(|t| change_txo_idx.contains(&t.idx))
                    .map(|t| t.outpoint())
                    .collect::<Vec<Outpoint>>()
                    .first()
                    .cloned()
            }
            TransferKind::Issuance => None,
        };

        let consignment_path = match (&kind, batch_transfer.status) {
            (TransferKind::Send | TransferKind::Burn | TransferKind::Mint, _) => {
                Some(self.send_consignment_path(
                    &asset_transfer.asset_id.clone().unwrap(),
                    &batch_transfer.txid.clone().unwrap(),
                ))
            }
            // no consignment is produced for an inflation, which has no counterparty
            (TransferKind::Inflation, _) => None,
            (
                TransferKind::ReceiveBlind | TransferKind::ReceiveWitness,
                TransferStatus::WaitingCounterparty,
            ) => None,
            (TransferKind::ReceiveBlind | TransferKind::ReceiveWitness, _) => {
                Some(self.get_receive_consignment_path(&transfer.recipient_id.clone().unwrap()))
            }
            (TransferKind::Issuance, _) => {
                Some(self.get_issue_consignment_path(&asset_transfer.asset_id.clone().unwrap()))
            }
        }
        .map(|p| p.to_string_lossy().to_string());

        let psbt_path = match &kind {
            TransferKind::Send
            | TransferKind::Inflation
            | TransferKind::Burn
            | TransferKind::Mint => batch_transfer
                .txid
                .as_ref()
                .map(|txid| self.get_transfer_dir(txid).join(UNSIGNED_PSBT_FILE))
                .filter(|p| p.exists())
                .map(|p| p.to_string_lossy().to_string()),
            TransferKind::ReceiveBlind | TransferKind::ReceiveWitness | TransferKind::Issuance => {
                None
            }
        };

        Ok(TransferData {
            kind,
            status: batch_transfer.status,
            batch_transfer_idx: batch_transfer.idx,
            assignments,
            txid: batch_transfer.txid.clone(),
            receive_utxo,
            change_utxo,
            created_at: batch_transfer.created_at,
            updated_at: batch_transfer.updated_at,
            expiration_timestamp: batch_transfer.expiration,
            consignment_path,
            psbt_path,
        })
    }

    fn list_transfers_impl(
        &self,
        txn: &DbTxn,
        asset_filter: AssetFilter,
        txid: Option<String>,
    ) -> Result<Vec<Transfer>, Error> {
        let db_data = txn.get_db_data(false)?;
        let batch_transfer_idxs: Option<HashSet<i32>> = txid.map(|txid| {
            db_data
                .batch_transfers
                .iter()
                .filter(|b| b.txid.as_deref() == Some(txid.as_str()))
                .map(|b| b.idx)
                .collect()
        });
        let asset_transfer_ids: Vec<i32> = db_data
            .asset_transfers
            .iter()
            .filter(|t| match &asset_filter {
                AssetFilter::AnyOrNone => true,
                AssetFilter::None => t.asset_id.is_none(),
                AssetFilter::Id(asset_id) => t.asset_id.as_ref() == Some(asset_id),
            })
            .filter(|t| {
                batch_transfer_idxs
                    .as_ref()
                    .is_none_or(|idxs| idxs.contains(&t.batch_transfer_idx))
            })
            .filter(|t| t.user_driven)
            .map(|t| t.idx)
            .collect();
        db_data
            .transfers
            .into_iter()
            .filter(|t| asset_transfer_ids.contains(&t.asset_transfer_idx))
            .map(|t| {
                let (asset_transfer, batch_transfer) =
                    t.related_transfers(&db_data.asset_transfers, &db_data.batch_transfers);
                let td = self.get_transfer_data(
                    &t,
                    &asset_transfer,
                    &batch_transfer,
                    &db_data.txos,
                    &db_data.colorings,
                )?;
                let tte_data = txn.get_transfer_transport_endpoints_data(t.idx)?;
                // recipient transport endpoints
                let transport_endpoints = tte_data
                    .iter()
                    .filter(|(tte, _)| tte.kind == TransportEndpointKind::RecipientProxy)
                    .map(|(tte, ce)| ce.to_transfer_transport_endpoint(tte))
                    .collect();
                // sender transport endpoint
                let sender_transport_endpoint = tte_data
                    .iter()
                    .find(|(tte, _)| tte.kind == TransportEndpointKind::SenderProxy)
                    .map(|(_, ce)| ce.endpoint.clone());
                Ok(t.to_transfer(td, transport_endpoints, sender_transport_endpoint))
            })
            .collect()
    }

    fn list_unspents_impl(
        &mut self,
        txn: &DbTxn,
        online: Option<Online>,
        settled_only: bool,
        skip_sync: bool,
    ) -> Result<Vec<Unspent>, Error> {
        self.sync_if_requested(txn, online, skip_sync, KeychainKind::External)?;
        self.sync_if_requested(txn, online, skip_sync, KeychainKind::Internal)?;

        let db_data = txn.get_db_data(false)?;

        let spent_txos_ids: Vec<i32> = db_data
            .txos
            .iter()
            .filter(|t| t.spent)
            .map(|t| t.idx)
            .collect();
        let waiting_confs_batch_transfer_ids: Vec<i32> = db_data
            .batch_transfers
            .iter()
            .filter(|t| t.waiting_confirmations())
            .map(|t| t.idx)
            .collect();
        let waiting_confs_transfer_ids: Vec<i32> = db_data
            .asset_transfers
            .iter()
            .filter(|t| waiting_confs_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let almost_spent_txos_ids: Vec<i32> = db_data
            .colorings
            .iter()
            .filter(|c| {
                waiting_confs_transfer_ids.contains(&c.asset_transfer_idx)
                    && spent_txos_ids.contains(&c.txo_idx)
            })
            .map(|c| c.txo_idx)
            .collect();
        let allocation_txos: Vec<DbTxo> = db_data
            .txos
            .into_iter()
            .filter(|t| !t.spent || almost_spent_txos_ids.contains(&t.idx))
            .collect();

        let mut txos_allocations = txn.get_rgb_allocations(
            allocation_txos,
            Some(db_data.colorings),
            Some(db_data.batch_transfers),
            Some(db_data.asset_transfers),
            Some(db_data.transfers),
        )?;

        txos_allocations
            .iter_mut()
            .for_each(|t| t.rgb_allocations.retain(|a| a.settled() || a.future()));

        txos_allocations.retain(|t| !(t.rgb_allocations.is_empty() && t.utxo.spent));

        let mut unspents: Vec<Unspent> = txos_allocations.into_iter().map(Unspent::from).collect();

        if settled_only {
            unspents
                .iter_mut()
                .for_each(|u| u.rgb_allocations.retain(|a| a.settled));
        }

        let spk_index = self.bdk_wallet().spk_index();
        for unspent in unspents.iter_mut() {
            if let Some(((KeychainKind::External, derivation_index), _)) =
                spk_index.txout(BdkOutPoint::from(unspent.utxo.outpoint.clone()))
            {
                unspent.utxo.derivation_index = Some(derivation_index);
            }
        }
        let mut internal_unspents: Vec<Unspent> =
            self.internal_unspents().map(Unspent::from).collect();

        unspents.append(&mut internal_unspents);

        Ok(unspents)
    }

    fn psbt_signature_count(&self, psbt: &Psbt) -> Result<u16, Error> {
        let mut signature_count = 0u16;
        for input in psbt.inputs.iter() {
            let partial_sigs_count = u16::try_from(input.partial_sigs.len())
                .map_err(|_| Error::TooManySignaturesInPsbt)?;
            let tap_script_sigs_count = u16::try_from(input.tap_script_sigs.len())
                .map_err(|_| Error::TooManySignaturesInPsbt)?;

            signature_count = signature_count
                .checked_add(partial_sigs_count)
                .and_then(|s| s.checked_add(tap_script_sigs_count))
                .ok_or(Error::TooManySignaturesInPsbt)?;
        }
        Ok(signature_count)
    }

    fn inspect_psbt_impl(&self, psbt: &str) -> Result<PsbtInspection, Error> {
        // check request data validity
        let psbt = Psbt::from_str(psbt)?;

        // collect PSBT inputs
        let mut inputs = Vec::new();
        let mut total_input_sat = 0u64;
        for (psbt_input, input) in psbt.inputs.iter().zip(psbt.unsigned_tx.input.iter()) {
            let witness_utxo =
                psbt_input
                    .witness_utxo
                    .as_ref()
                    .ok_or_else(|| Error::PsbtInspection {
                        details: s!("cannot inspect non-segwit PSBT"),
                    })?;
            let amount_sat = witness_utxo.value.to_sat();
            let is_mine = self
                .bdk_wallet()
                .is_mine(witness_utxo.script_pubkey.clone());
            total_input_sat = total_input_sat
                .checked_add(amount_sat)
                .expect("should never overflow");
            inputs.push(PsbtInputInfo {
                outpoint: Outpoint {
                    txid: input.previous_output.txid.to_string(),
                    vout: input.previous_output.vout,
                },
                amount_sat,
                is_mine,
            });
        }

        // collect PSBT outputs
        let mut outputs = Vec::new();
        let mut total_output_sat = 0u64;
        let network = BdkNetwork::from(self.bitcoin_network());
        for output in psbt.unsigned_tx.output.iter() {
            let amount_sat = output.value.to_sat();
            total_output_sat = total_output_sat
                .checked_add(amount_sat)
                .expect("should never overflow");
            let is_op_return = output.script_pubkey.is_op_return();
            let address = if !is_op_return {
                BdkAddress::from_script(&output.script_pubkey, network)
                    .ok()
                    .map(|addr| addr.to_string())
            } else {
                None
            };
            let is_mine = self.bdk_wallet().is_mine(output.script_pubkey.clone());
            outputs.push(PsbtOutputInfo {
                address,
                script_pubkey_hex: output.script_pubkey.to_hex_string(),
                amount_sat,
                is_op_return,
                is_mine,
            });
        }

        Ok(PsbtInspection {
            txid: psbt.unsigned_tx.compute_txid().to_string(),
            inputs,
            outputs,
            total_input_sat,
            total_output_sat,
            fee_sat: total_input_sat.saturating_sub(total_output_sat),
            signature_count: self.psbt_signature_count(&psbt)?,
            size_vbytes: psbt.unsigned_tx.vsize() as u64,
        })
    }

    fn inspect_rgb_transfer_impl(
        &self,
        txn: &DbTxn,
        psbt: String,
        fascia_path: String,
        entropy: u64,
    ) -> Result<RgbInspection, Error> {
        // check request data validity
        let psbt = Psbt::from_str(&psbt)?;
        check_file_exists(&fascia_path)?;
        let fascia =
            read_fascia(&PathBuf::from(&fascia_path)).map_err(|_| Error::InvalidFilePath {
                file_path: fascia_path,
            })?;

        // verify the fascia's witness ID matches our PSBT's transaction ID
        let fascia_witness_id = fascia.witness_id();
        let witness_id = psbt.unsigned_tx.compute_txid();
        if fascia_witness_id != witness_id {
            return Err(Error::RgbInspection {
                details: format!(
                    "fascia witness ID {} does not match PSBT TXID {}",
                    fascia_witness_id, witness_id
                ),
            });
        }

        // verify all PSBT inputs belong to this wallet
        for psbt_input in psbt.inputs.iter() {
            let script_pubkey = psbt_input
                .witness_utxo
                .as_ref()
                .ok_or_else(|| Error::RgbInspection {
                    details: s!("RGB-related PSBTs require segwit"),
                })?
                .script_pubkey
                .clone();
            if !self.bdk_wallet().is_mine(script_pubkey) {
                return Err(Error::RgbInspection {
                    details: s!("found a PSBT input that does not belong to this wallet"),
                });
            }
        }

        // inspect fascia bundles and collect operations and commitment messages
        let mut messages = BTreeMap::new();
        let mut operations = Vec::new();
        let mut stock = self.rgb_stock(txn)?;
        let prev_outputs: Vec<_> = psbt
            .unsigned_tx
            .input
            .iter()
            .map(|input| BdkOutPoint::new(input.previous_output.txid, input.previous_output.vout))
            .collect();
        let is_revealed_seal_ours = |txid: TxPtr, vout: u32| -> bool {
            if txid == TxPtr::WitnessTx {
                psbt.unsigned_tx
                    .output
                    .get(vout as usize)
                    .map(|output| self.bdk_wallet().is_mine(output.script_pubkey.clone()))
                    .unwrap_or(false)
            } else {
                let outpoint = Outpoint {
                    txid: txid.to_string(),
                    vout,
                };
                txn.get_txo(&outpoint).unwrap_or(None).is_some()
            }
        };
        for (contract_id, bundle) in fascia.bundles() {
            // collect the RGB inputs allocated in stock to the spent outputs
            let mut stock_inputs = HashMap::new();
            if let Ok(ass_map) =
                stock.contract_assignments_for(*contract_id, prev_outputs.iter().copied())
            {
                for (explicit_seal, opout_state_map) in ass_map {
                    let outpoint = explicit_seal.to_outpoint();
                    for (opout, state) in opout_state_map {
                        let assignment = Assignment::from_opout_and_state(opout, &state);
                        stock_inputs.insert(opout, (outpoint, assignment));
                    }
                }
            }

            // safety check: ensure the bundle spends all the stock inputs, as the ones it doesn't
            // spend would be lost (inputs missing in stock are detected when inspecting the bundle)
            let bundle_input_opouts: HashSet<_> = bundle
                .known_transitions
                .iter()
                .flat_map(|KnownTransition { transition, .. }| transition.inputs())
                .collect();
            let extra_in_stock: Vec<_> = stock_inputs
                .keys()
                .filter(|opout| !bundle_input_opouts.contains(opout))
                .collect();
            if !extra_in_stock.is_empty() {
                return Err(Error::RgbInspection {
                    details: format!(
                        "bundle for contract {contract_id}: stock contains {} input(s) not declared in bundle (malformed transfer?): {extra_in_stock:?}",
                        extra_in_stock.len(),
                    ),
                });
            }

            // inspect bundle transitions
            let chain_net = stock.genesis(*contract_id)?.chain_net;
            let asset_schema = AssetSchema::get_from_contract_id(*contract_id, &stock)?;
            let is_ours = |seal: &BuilderSeal<GraphSeal>| {
                Some(match seal {
                    BuilderSeal::Revealed(seal) => {
                        is_revealed_seal_ours(seal.txid, seal.vout.into_u32())
                    }
                    BuilderSeal::Concealed(seal) => {
                        stock.seal_of_secret(*seal).unwrap_or(None).is_some()
                    }
                })
            };
            let operation = bundle_info(
                *contract_id,
                asset_schema,
                chain_net,
                bundle,
                &psbt.unsigned_tx,
                true,
                |opout| stock_inputs.get(&opout).cloned(),
                is_ours,
                |_| None,
            )?;

            operations.push(operation);

            messages.insert(
                ProtocolId::from(*contract_id),
                Message::from(bundle.bundle_id()),
            );
        }

        // rebuild the MPC commitment of the fascia bundles and verify the PSBT commits to it, which
        // ensures the PSBT commits to these bundles only
        let mut source = MultiSource::with_static_entropy(entropy);
        source.messages = MediumOrdMap::from_checked(messages);
        let merkle_tree = MerkleTree::try_commit(&source).expect("commit should succeed");
        let commitment = merkle_tree.commit_id();
        let dbc_proof = &fascia.seal_witness().dbc_proof;
        verify_witness_commitment(dbc_proof, &commitment, &psbt.unsigned_tx)?;

        Ok(RgbInspection {
            txid: witness_id.to_string(),
            close_method: dbc_proof.method(),
            commitment_hex: hex::encode(commitment.to_byte_array()),
            operations,
        })
    }

    fn get_transfer_end_data(&self, psbt: &Psbt) -> Result<TransferEndData, Error> {
        let txid = psbt
            .clone()
            .extract_tx()
            .map_err(InternalError::from)?
            .compute_txid()
            .to_string();
        let transfer_dir = self.get_transfer_dir(&txid);
        if !transfer_dir.exists() {
            return Err(Error::UnknownTransfer { txid });
        }

        let info_file = transfer_dir.join(TRANSFER_DATA_FILE);
        let serialized_info = fs::read_to_string(info_file)?;
        let info_contents: InfoBatchTransfer =
            serde_json::from_str(&serialized_info).map_err(InternalError::from)?;

        let fascia = read_fascia(&transfer_dir.join(FASCIA_FILE))?;

        Ok((txid, transfer_dir, info_contents, fascia))
    }

    fn get_send_consignment_path_impl<P: AsRef<Path>>(&self, asset_transfer_dir: P) -> PathBuf {
        asset_transfer_dir.as_ref().join(CONSIGNMENT_FILE)
    }

    fn gen_consignments(
        &self,
        txn: &DbTxn,
        fascia: &Fascia,
        transfer_info_map: &BTreeMap<String, InfoAssetTransfer>,
        transfer_dir: &PathBuf,
    ) -> Result<(), Error> {
        let mut stock = self.rgb_stock(txn)?;
        for (asset_id, transfer_info) in transfer_info_map {
            let contract_id = transfer_info.asset_info.contract_id;
            #[allow(unused_mut)]
            let (mut consignment, spv_proofs) = stock.transfer_from_fascia(
                contract_id,
                transfer_info.beneficiaries_witness.clone(),
                transfer_info.beneficiaries_blinded.clone(),
                transfer_info.requested_opids(fascia),
                fascia,
                self.spv_resolver(),
            )?;
            stock.store_spv_proofs(spv_proofs)?;
            #[cfg(all(test, feature = "_online"))]
            if strip_terminals() {
                consignment.terminals = Default::default();
            }
            let asset_transfer_dir = self.get_asset_transfer_dir(transfer_dir, asset_id);
            fs::create_dir_all(&asset_transfer_dir)?;
            let consignment_path = self.get_send_consignment_path_impl(asset_transfer_dir);
            atomic_write_with(&consignment_path, |tmp| Ok(consignment.save_file(tmp)?))?;
        }
        Ok(())
    }
}

/// Offline operations for a wallet.
pub trait RgbWalletOpsOffline: WalletOffline + WalletBackup {
    /// Return the data that defines the wallet.
    fn get_wallet_data(&self) -> WalletData {
        self.wallet_data().clone()
    }

    /// Return the wallet directory.
    fn get_wallet_dir(&self) -> PathBuf {
        self.wallet_dir().clone()
    }

    /// Return the media directory.
    fn get_media_dir(&self) -> PathBuf {
        self.media_dir()
    }

    /// Return the [`Balance`] for the RGB asset with the provided ID.
    fn get_asset_balance(&self, asset_id: String) -> Result<Balance, Error> {
        info!(self.logger(), "Getting balance for asset '{}'...", asset_id);
        let txn = self.database().begin_transaction()?;
        let balance = self.get_asset_balance_impl(&txn, asset_id)?;
        txn.commit()?;
        info!(self.logger(), "Get asset balance completed");
        Ok(balance)
    }

    /// Return the [`Metadata`] for the RGB asset with the provided ID.
    fn get_asset_metadata(&self, asset_id: String) -> Result<Metadata, Error> {
        info!(
            self.logger(),
            "Getting metadata for asset '{}'...", asset_id
        );
        let txn = self.database().begin_transaction()?;
        let metadata = self.get_asset_metadata_impl(&txn, asset_id)?;
        txn.commit()?;
        info!(self.logger(), "Get asset metadata completed");
        Ok(metadata)
    }

    /// Return the [`BtcBalance`] of the internal Bitcoin wallets.
    fn get_btc_balance(
        &mut self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<BtcBalance, Error> {
        info!(self.logger(), "Getting BTC balance...");
        let txn = self.database().begin_transaction()?;
        let balance = self.get_btc_balance_impl(&txn, online, skip_sync)?;
        self.persist_and_commit(txn)?;
        info!(self.logger(), "Get BTC balance completed");
        Ok(balance)
    }

    /// List the known RGB assets.
    ///
    /// Providing an empty `filter_asset_schemas` will list assets for all schemas, otherwise only
    /// assets for the provided schemas will be listed.
    ///
    /// The returned `Assets` will have fields set to `None` for schemas that have not been
    /// requested.
    fn list_assets(&self, filter_asset_schemas: Vec<AssetSchema>) -> Result<Assets, Error> {
        info!(self.logger(), "Listing assets...");
        let txn = self.database().begin_transaction()?;
        let assets = self.list_assets_impl(&txn, filter_asset_schemas)?;
        txn.commit()?;
        info!(self.logger(), "List assets completed");
        Ok(assets)
    }

    /// List the Bitcoin [`Transaction`]s known to the wallet, newest first.
    fn list_transactions(
        &mut self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<Vec<Transaction>, Error> {
        info!(self.logger(), "Listing transactions...");
        let txn = self.database().begin_transaction()?;
        let transactions = self.list_transactions_impl(&txn, online, skip_sync)?;
        self.persist_and_commit(txn)?;
        info!(self.logger(), "List transactions completed");
        Ok(transactions)
    }

    /// List the RGB [`Transfer`]s known to the wallet.
    ///
    /// `asset_filter` selects transfers by asset. When a `txid` is provided, restrict the result to the
    /// transfers committed by the on-chain transaction with that ID; an unknown `txid` yields an
    /// empty list.
    fn list_transfers(
        &self,
        asset_filter: AssetFilter,
        txid: Option<String>,
    ) -> Result<Vec<Transfer>, Error> {
        info!(
            self.logger(),
            "Listing transfers for filter '{:?}' and txid '{:?}'...", asset_filter, txid
        );
        let txn = self.database().begin_transaction()?;
        if let AssetFilter::Id(asset_id) = &asset_filter {
            txn.check_asset_exists(asset_id.clone())?;
        }
        let transfers = self.list_transfers_impl(&txn, asset_filter, txid)?;
        txn.commit()?;
        info!(self.logger(), "List transfers completed");
        Ok(transfers)
    }

    /// List the [`Unspent`]s known to the wallet.
    ///
    /// If `settled_only` is true only show settled RGB allocations, if false also show pending RGB
    /// allocations.
    fn list_unspents(
        &mut self,
        online: Option<Online>,
        settled_only: bool,
        skip_sync: bool,
    ) -> Result<Vec<Unspent>, Error> {
        info!(self.logger(), "Listing unspents...");
        let txn = self.database().begin_transaction()?;
        let unspents = self.list_unspents_impl(&txn, online, settled_only, skip_sync)?;
        self.persist_and_commit(txn)?;
        info!(self.logger(), "List unspents completed");
        Ok(unspents)
    }

    /// Finalize a PSBT, optionally providing BDK sign options to tweak the behavior of the
    /// finalizer.
    fn finalize_psbt(
        &self,
        signed_psbt: String,
        sign_options: Option<SignOptions>,
    ) -> Result<String, Error> {
        info!(self.logger(), "Finalizing PSBT...");
        let mut psbt = Psbt::from_str(&signed_psbt)?;
        self.finalize_psbt_impl(&mut psbt, sign_options)?;
        info!(self.logger(), "Finalize PSBT completed");
        Ok(psbt.to_string())
    }

    /// Delete eligible transfers from the database and return true if any transfer has been
    /// deleted.
    ///
    /// An optional `batch_transfer_idx` can be provided to operate on a single batch transfer.
    ///
    /// If a `batch_transfer_idx` is provided and `no_asset_only` is true, transfers with an
    /// associated asset ID will not be deleted and instead return an error.
    ///
    /// If no `batch_transfer_idx` is provided, all failed transfers will be deleted, and if
    /// `no_asset_only` is true transfers with an associated asset ID will be skipped.
    ///
    /// Eligible transfers are the ones in status [`TransferStatus::Failed`].
    fn delete_transfers(
        &self,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        info!(
            self.logger(),
            "Deleting batch transfer with idx {:?}...", batch_transfer_idx
        );
        let txn = self.database().begin_transaction()?;
        let changed = self.delete_transfers_impl(&txn, batch_transfer_idx, no_asset_only)?;
        if changed {
            self.update_backup_info(&txn, false)?;
        }
        txn.commit()?;
        info!(self.logger(), "Delete transfer completed");
        Ok(changed)
    }

    /// Create a backup of the wallet as a file with the provided name and encrypted with the
    /// provided password.
    ///
    /// Scrypt is used for hashing and xchacha20poly1305 is used for encryption. A random salt for
    /// hashing and a random nonce for encrypting are randomly generated and included in the final
    /// backup file, along with the backup version.
    fn backup(&self, backup_path: &str, password: &str) -> Result<(), Error> {
        info!(self.logger(), "Backing up...");
        self.backup_customize(backup_path, password, None)?;
        info!(self.logger(), "Backup completed");
        Ok(())
    }

    /// Return whether the wallet requires to perform a backup.
    fn backup_info(&self) -> Result<bool, Error> {
        info!(self.logger(), "Getting backup info...");
        let txn = self.database().begin_transaction()?;
        let backup_required = self.get_backup_info(&txn)?;
        txn.commit()?;
        info!(self.logger(), "Get backup info completed");
        Ok(backup_required)
    }

    /// Get the details of the mint received on the provided `recipient_id`, once the wallet has
    /// checked it.
    ///
    /// The consignment of a mint is posted before its sender locks the corresponding tokens on the
    /// external chain, so that its recipient can check it first: when refreshing, the wallet checks
    /// it, assuming the lock will be made, and refuses it unless it's valid, pays the invoice (the
    /// requested amount, if the invoice requests one) and none of its history is rejected. The
    /// returned details are what the lock has to match, for the mint to complete: its operation
    /// ID and amount, the external chain block it has to follow and the bridge contract where to
    /// make it. The lock has to be included on the external chain by the expiration of the invoice:
    /// if it is, the transfer is waited for past the expiration, until the lock is finalized, for
    /// up to 24 hours.
    ///
    /// Returns `None` if no mint checked by the wallet pays `recipient_id` (e.g. it hasn't been
    /// received or checked yet, or it has been refused or failed).
    fn get_received_mint(&self, recipient_id: String) -> Result<Option<ReceivedMint>, Error> {
        info!(self.logger(), "Getting received mint...");
        let txn = self.database().begin_transaction()?;
        let received_mint = self.get_received_mint_impl(&txn, &recipient_id)?;
        txn.commit()?;
        info!(self.logger(), "Get received mint completed");
        Ok(received_mint)
    }

    /// Inspect a PSBT to return its information.
    fn inspect_psbt(&self, psbt: String) -> Result<PsbtInspection, Error> {
        info!(self.logger(), "Inspecting PSBT...");
        let inspection = self.inspect_psbt_impl(&psbt)?;
        info!(self.logger(), "PSBT inspection completed");
        Ok(inspection)
    }

    /// Inspect a PSBT and an RGB fascia to verify the commitment and show information about the
    /// RGB transfer.
    fn inspect_rgb_transfer(
        &self,
        psbt: String,
        fascia_path: String,
        entropy: u64,
    ) -> Result<RgbInspection, Error> {
        info!(self.logger(), "Inspecting RGB transfer...");
        let txn = self.database().begin_transaction()?;
        let inspection = self.inspect_rgb_transfer_impl(&txn, psbt, fascia_path, entropy)?;
        txn.commit()?;
        info!(self.logger(), "RGB transfer inspection completed");
        Ok(inspection)
    }
}
