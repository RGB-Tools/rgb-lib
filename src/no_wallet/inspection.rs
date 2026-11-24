//! Inspection.
//!
//! This module defines the consignment and fascia inspections, which don't require a wallet, and
//! the objects they return, some of which are shared with wallet methods (e.g. the PSBT inspection
//! ones).

use super::*;

/// An operation output, which RGB identifies as `<operation ID>/<assignment type>/<index>`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Opout {
    /// ID of the operation the output belongs to
    pub opid: String,
    /// Type of the assignment the output belongs to
    pub assignment_type: TypeOfAssignment,
    /// Index of the output among the ones of its assignment type
    pub index: u16,
}

impl TryFrom<RgbOpout> for Opout {
    /// The assignment type of the operation output, when it's not one of the supported schemas'
    type Error = AssignmentType;

    fn try_from(opout: RgbOpout) -> Result<Self, Self::Error> {
        Ok(Self {
            opid: opout.op.to_string(),
            assignment_type: TypeOfAssignment::from_assignment_type(opout.ty).ok_or(opout.ty)?,
            index: opout.no,
        })
    }
}

/// Information about an RGB input being spent.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbInputInfo {
    /// The operation output the input spends, which the bundle of the spending transition commits
    /// to in its input map
    pub opout: Opout,
    /// The index of the transaction input spending the allocation (`None` when the allocation
    /// can't be resolved, as inspecting a fascia without a wallet)
    pub vin: Option<u32>,
    /// The assignment in input, the state it carries included (`None` when the spent allocation
    /// isn't resolved, as inspecting a fascia without a wallet: the opout gives the type of the
    /// assignment, not the state the input spends)
    pub assignment: Option<Assignment>,
}

/// Information about an RGB output destination.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbOutputInfo {
    /// The output TXID (if the seal is revealed)
    pub txid: Option<String>,
    /// The output vout (if the seal is revealed)
    pub vout: Option<u32>,
    /// The blinding of the output seal (if the seal is revealed), telling apart allocations on the
    /// same vout
    pub blinding: Option<u64>,
    /// The assignment in output
    pub assignment: Assignment,
    /// Whether this output is allocated to a concealed seal
    pub is_concealed: bool,
    /// The recipient ID of the blinded receive the output is allocated to (if the seal is
    /// concealed)
    pub recipient_id: Option<String>,
    /// Whether this output belongs to the wallet (`None` when inspecting without a wallet)
    pub is_ours: Option<bool>,
    /// Whether this output belongs to a terminal operation of the consignment, one nothing in the
    /// consignment builds on, so to what the consignment proves rather than to its history (`None`
    /// when not inspecting a consignment)
    pub is_terminal: Option<bool>,
}

/// A global state entry committed in an RGB transition.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum TransitionGlobal {
    /// Amount of newly issued supply, committed in inflate transitions
    IssuedSupply(u64),
    /// Amount of burned asset, committed in burn transitions
    BurnedAsset(u64),
    /// Amount of burned inflation rights, committed in burn transitions
    BurnedInflation(u64),
    /// Hex-encoded 32-byte burn reason, committed in burn transitions
    BurnReason(String),
    /// ID of the contract the asset is linked to, committed in link transitions
    LinkedToContract(String),
}

/// A metadata entry committed in an RGB transition, read by its validation but not part of the
/// contract state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum TransitionMetadata {
    /// Amount of inflation rights allowed in output, committed in IFA inflate transitions
    AllowedInflation(u64),
    /// External chain block before which the lock backing the mint has to be included, committed
    /// in BFA mint transitions
    BeforeBlock(u64),
}

/// Information about an RGB transition.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbTransitionInfo {
    /// The transition type
    pub r#type: TypeOfTransition,
    /// ID of the operation
    pub opid: String,
    /// Details of the transition inputs
    pub inputs: Vec<RgbInputInfo>,
    /// Details of the transition outputs
    pub outputs: Vec<RgbOutputInfo>,
    /// Global state entries committed in the transition
    pub globals: Vec<TransitionGlobal>,
    /// Metadata entries committed in the transition
    pub metadata: Vec<TransitionMetadata>,
}

/// Information about an RGB operation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbOperationInfo {
    /// ID of the asset being operated on
    pub asset_id: String,
    /// Number of asset transitions committed by the witness transaction, which can be more than
    /// the inspected ones
    pub committed_transition_count: u32,
    /// Number of asset inputs spent by the transitions committed by the witness transaction
    pub committed_input_count: u32,
    /// Details of the inspected asset transitions committed by the witness transaction
    pub transitions: Vec<RgbTransitionInfo>,
}

impl RgbOperationInfo {
    /// Return whether the witness transaction commits asset transitions that are not part of the
    /// inspected ones, meaning the operation contains concealed transitions.
    pub fn has_concealed_transitions(&self) -> bool {
        self.committed_transition_count as usize != self.transitions.len()
    }
}

/// Result of inspecting the RGB operations committed by a witness transaction.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbInspection {
    /// ID of the witness transaction
    pub txid: String,
    /// The close method used for the operation
    pub close_method: CloseMethod,
    /// The RGB commitment in hex encoded format
    pub commitment_hex: String,
    /// Details of each operation
    pub operations: Vec<RgbOperationInfo>,
}

impl RgbInspection {
    /// Return whether any of the inspected operations contains concealed transitions.
    pub fn has_concealed_transitions(&self) -> bool {
        self.operations
            .iter()
            .any(RgbOperationInfo::has_concealed_transitions)
    }
}

/// Information about a media file attached to an RGB asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbMediaInfo {
    /// Hex-encoded digest of the media file
    pub digest: String,
    /// MIME type of the media file
    pub mime: String,
}

impl From<&Attachment> for RgbMediaInfo {
    fn from(attachment: &Attachment) -> Self {
        Self {
            digest: hex::encode(attachment.digest),
            mime: attachment.ty.to_string(),
        }
    }
}

/// A smart contract deployed on an EVM-compatible chain, as referenced by RGB contract state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbEvmContractInfo {
    /// EIP-155 chain ID of the chain the contract is deployed on
    pub chain_id: u64,
    /// Address of the contract, hex-encoded with the `0x` prefix
    pub address: String,
}

impl From<&EvmContract> for RgbEvmContractInfo {
    fn from(contract: &EvmContract) -> Self {
        Self {
            chain_id: contract.chain_id,
            address: contract.address.to_string(),
        }
    }
}

/// Where the bridge contract backing an asset is deployed, committed in its genesis.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum RgbBridgeLocationInfo {
    /// A bridge contract deployed on an EVM-compatible chain, whose chain ID a claim resolver has
    /// to check against the chain it's connected to
    Evm(RgbEvmContractInfo),
}

impl From<BridgeLocation> for RgbBridgeLocationInfo {
    fn from(location: BridgeLocation) -> Self {
        match location {
            BridgeLocation::Evm(contract) => Self::Evm((&contract).into()),
        }
    }
}

/// Where the reject list of an asset is published, committed in its genesis.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum RgbRejectListInfo {
    /// A list of rejected opouts served at a URL
    Url(String),
    /// A list of rejected opouts kept by an `IRgbRejectList` contract deployed on an
    /// EVM-compatible chain
    Evm(RgbEvmContractInfo),
}

impl From<RejectListSource> for RgbRejectListInfo {
    fn from(source: RejectListSource) -> Self {
        match source {
            RejectListSource::Url(url) => Self::Url(url),
            RejectListSource::Evm(contract) => Self::Evm((&contract).into()),
        }
    }
}

impl From<RejectListLocation> for RgbRejectListInfo {
    fn from(location: RejectListLocation) -> Self {
        match location {
            RejectListLocation::Url(url) => Self::Url(url.to_string()),
            RejectListLocation::Evm(contract) => Self::Evm((&contract).into()),
        }
    }
}

/// Information about the genesis of an RGB asset.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbGenesisInfo {
    /// Chain and network the asset is issued on, as in RGB invoices (e.g. `bc` for Bitcoin mainnet,
    /// `bcrt` for Bitcoin regtest)
    pub chain_net: String,
    /// Issuance timestamp
    pub timestamp: i64,
    /// Issuer identity
    pub issuer: String,
    /// Asset name
    pub name: String,
    /// Asset ticker (not defined by the CFA schema)
    pub ticker: Option<String>,
    /// Asset details
    pub details: Option<String>,
    /// Asset precision
    pub precision: u8,
    /// Supply issued by the genesis itself, committed by the schemas that allow an initial
    /// issuance (`None` for the UDA schema, which issues a single token, and for the BFA one,
    /// whose supply all comes from mints). Later issuances commit their own amount, so this is
    /// the initial supply, not the current one (see [`TransitionGlobal::IssuedSupply`])
    pub issued_supply: Option<u64>,
    /// Maximum supply the asset can be inflated to, committed by the IFA schema (`None` for the
    /// other schemas: a BFA asset has no such cap, its supply being bounded by the tokens locked
    /// on the bridge contract)
    pub max_supply: Option<u64>,
    /// ID of the contract the asset is linked from, if any (only committed by the BFA and IFA
    /// schemas; see [`TransitionGlobal::LinkedToContract`] for the other end of the link)
    pub linked_from_contract: Option<String>,
    /// Where the bridge contract backing the asset is deployed, which the BFA schema requires
    /// (`None` for the other schemas)
    pub bridge_location: Option<RgbBridgeLocationInfo>,
    /// Where the reject list of the asset is published, if it declares one (only committed by the
    /// BFA and IFA schemas, the latter supporting a URL only)
    pub reject_list: Option<RgbRejectListInfo>,
    /// Media files of the asset: the contract terms media, plus, for a UDA, the token media and
    /// its attachments
    pub media: Vec<RgbMediaInfo>,
    /// Details of the issuance outputs
    pub outputs: Vec<RgbOutputInfo>,
}

/// Result of inspecting an RGB consignment.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct ConsignmentInspection {
    /// ID of the consignment
    pub consignment_id: String,
    /// Version of the consignment
    pub version: u8,
    /// Whether this is a transfer consignment (`false` for a contract consignment)
    pub is_transfer: bool,
    /// ID of the asset
    pub asset_id: String,
    /// Schema of the asset
    pub asset_schema: AssetSchema,
    /// Details of the asset genesis
    pub genesis: RgbGenesisInfo,
    /// Details of the inspected witness transactions (the ones of the terminal operations or all of
    /// them), in consignment order (none for a contract consignment)
    pub witnesses: Vec<RgbInspection>,
}

/// The bundles of a consignment to inspect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum BundleFilter {
    /// All the consignment bundles
    All,
    /// The bundles holding a terminal operation of the consignment, one nothing in the consignment
    /// builds on (e.g. the last transfer or a burn)
    Terminals,
    /// The bundle committed by the witness transaction with the given TXID
    Witness(String),
}

/// Inspect the RGB consignment (transfer or contract) at the given path to show information about
/// the asset and the transitions committed by the witness transactions of the bundles selected by
/// the given filter.
///
/// Inputs are resolved from the consignment history, outputs report whether they belong to a
/// terminal operation of the consignment. Which outputs are allocated to the consignment recipient
/// isn't reported, so a received allocation can't be told from a change by this alone: a recipient
/// finds theirs by recipient ID (blinded receives) or by TXID and vout (witness receives). As the
/// owner of a UTXO can spend all the allocations on it, TXID and vout tell which allocations a
/// recipient controls, but not which transfer each pays when several are on the same vout (e.g. two
/// witness receives to the same script paid by the same transaction): the blinding tells them
/// apart, when known (agreed out of band or derived by the proxy exchange).
///
/// The witness transactions are verified to commit to the inspected transitions, and the
/// transitions their inputs are resolved from to have the operation ID they are referred to by,
/// also for a validated consignment file. This doesn't validate the consignment: a successful
/// inspection doesn't mean the consignment is valid nor that its witness transactions have been
/// broadcast.
pub fn inspect_consignment(
    consignment_path: String,
    bundle_filter: BundleFilter,
) -> Result<ConsignmentInspection, Error> {
    check_file_exists(&consignment_path)?;
    let witness_id = match &bundle_filter {
        BundleFilter::Witness(txid) => Some(Txid::from_str(txid).map_err(|_| Error::InvalidTxid)?),
        BundleFilter::All | BundleFilter::Terminals => None,
    };
    let selected = |txid: Txid, has_terminals: bool| match bundle_filter {
        BundleFilter::All => true,
        BundleFilter::Terminals => has_terminals,
        BundleFilter::Witness(_) => Some(txid) == witness_id,
    };

    let inspection = match UniversalFile::load_file(&consignment_path) {
        Ok(UniversalFile::Transfer(consignment)) => consignment_inspection(&consignment, selected),
        Ok(UniversalFile::Contract(consignment)) => consignment_inspection(&consignment, selected),
        Ok(UniversalFile::SchemaDefinition(_)) => Err(Error::InvalidConsignment),
        // validated consignments have their own file type, but they're checked as any other: the
        // file doesn't prove it has been validated
        Err(LoadError::InvalidMagic) => {
            if let Ok(consignment) = ValidTransfer::load_file(&consignment_path) {
                consignment_inspection(&consignment, selected)
            } else if let Ok(consignment) = ValidContract::load_file(&consignment_path) {
                consignment_inspection(&consignment, selected)
            } else {
                Err(Error::InvalidConsignment)
            }
        }
        Err(_) => Err(Error::InvalidConsignment),
    }?;

    if let Some(witness_id) = witness_id
        && inspection.witnesses.is_empty()
    {
        return Err(Error::RgbInspection {
            details: format!("witness {witness_id} not found in the consignment"),
        });
    }
    Ok(inspection)
}

/// Build the inspection details of a consignment, inspecting the bundles selected by the given
/// function (called with their witness TXID and whether they hold a terminal operation) and
/// checking the witness transactions commit to the inspected transitions.
fn consignment_inspection<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
    selected: impl Fn(Txid, bool) -> bool,
) -> Result<ConsignmentInspection, Error> {
    let contract_id = consignment.contract_id();
    let asset_schema = AssetSchema::from_schema_id(consignment.schema_id())?;
    let genesis = genesis_info(contract_id, asset_schema, &consignment.genesis)?;

    // index the operations whose assignments can be spent by consignment transitions. Inputs refer
    // to them by operation ID, so only the transitions having the ID they're listed with are
    // indexed (a different one would make inputs show assignments they don't spend), and an
    // operation in more than one bundle is refused (it's not clear which witness spends it)
    let genesis_id = consignment.genesis.id();
    let mut transitions: HashMap<OpId, (&Transition, Txid)> = HashMap::new();
    for witness_bundle in consignment.bundles.iter() {
        let witness_id = witness_bundle.witness_id();
        for KnownTransition { opid, transition } in witness_bundle.bundle.known_transitions.iter() {
            if transition.id() != *opid {
                continue;
            }
            if let Some((_, other_witness_id)) = transitions.insert(*opid, (transition, witness_id))
                && other_witness_id != witness_id
            {
                return Err(Error::RgbInspection {
                    details: format!(
                        "transition {opid} in the bundles of witnesses {other_witness_id} and \
                         {witness_id}"
                    ),
                });
            }
        }
    }
    // get the spent outpoint and assignment of a transition input
    let resolve_input = |opout: RgbOpout| -> Option<(OutPoint, Assignment)> {
        if opout.op == genesis_id {
            let typed_assigns = consignment.genesis.assignments.get(&opout.ty)?;
            let seal = typed_assigns.revealed_seal_at(opout.no).ok()??;
            let assignment = Assignment::from_typed_assigns(opout.ty, typed_assigns, opout.no)?;
            Some((seal.to_outpoint(), assignment))
        } else {
            let (transition, witness_id) = transitions.get(&opout.op)?;
            let typed_assigns = transition.assignments.get(&opout.ty)?;
            let seal = typed_assigns.revealed_seal_at(opout.no).ok()??;
            let assignment = Assignment::from_typed_assigns(opout.ty, typed_assigns, opout.no)?;
            Some((seal.resolve(*witness_id).to_outpoint(), assignment))
        }
    };

    // inspect the selected bundles
    let terminal_ops = consignment.terminal_ops();
    let mut witnesses = vec![];
    for witness_bundle in consignment.bundles.iter() {
        let bundle_id = witness_bundle.bundle.bundle_id();
        let txid = witness_bundle.witness_id();
        let has_terminals = witness_bundle
            .bundle
            .known_transitions
            .iter()
            .any(|kt| terminal_ops.contains(&kt.opid));
        if !selected(txid, has_terminals) {
            continue;
        }
        let tx = &witness_bundle.tx;

        // get the commitment, verifying the witness transaction commits to it if requested
        let anchor = &witness_bundle.anchor;
        let commitment =
            anchor
                .convolve(contract_id, bundle_id)
                .map_err(|e| Error::RgbInspection {
                    details: format!("bundle {bundle_id} not committed by witness {txid}: {e}"),
                })?;
        verify_witness_commitment(&anchor.dbc_proof, &commitment, tx)?;

        let operation = bundle_info(
            contract_id,
            Some(asset_schema),
            consignment.genesis.chain_net,
            &witness_bundle.bundle,
            tx,
            true,
            true,
            resolve_input,
            |_| None,
            |opid| Some(terminal_ops.contains(opid)),
        )?;

        witnesses.push(RgbInspection {
            txid: txid.to_string(),
            close_method: anchor.dbc_proof.method(),
            commitment_hex: hex::encode(commitment.to_byte_array()),
            operations: vec![operation],
        });
    }

    Ok(ConsignmentInspection {
        consignment_id: consignment.consignment_id().to_string(),
        version: consignment.version as u8,
        is_transfer: consignment.transfer,
        asset_id: contract_id.to_string(),
        asset_schema,
        genesis,
        witnesses,
    })
}

/// Inspect the given PSBT and the RGB fascia at the given path, both saved by the wallet that
/// built the operation, to verify the PSBT commits to the bundles of the fascia and show
/// information about the RGB transitions they carry, so that a signer can check what the PSBT it's
/// asked to sign commits to.
///
/// The fascia is checked to be the one of the PSBT transaction and the MPC commitment of its
/// bundles, rebuilt with the entropy the operation was built with, to be the one the PSBT
/// transaction commits to, which proves it commits to these bundles only. The bundles are checked
/// to commit to the inspected transitions.
///
/// A fascia file larger than `max_fascia_size` bytes is refused with [`Error::FileTooLarge`] before
/// being decoded, and a fascia with more than `max_bundles` bundles before inspecting them and
/// rebuilding the commitment, whose cost grows with the number of bundles: an operation commits a
/// bundle per asset it moves, so the limits are the size of the fascia and the most assets the
/// signer expects an operation to move.
///
/// The entropy defaults to the one the MPC commitment of the fascia carries, so it only has to be
/// given for a fascia whose commitment doesn't carry it, as a concealed one, while an inspection
/// without either fails. The wallet that built the operation reports it as well, and a given one
/// the commitment of the fascia wasn't built with is an error, as the two can only disagree if the
/// fascia isn't the one of the inspected operation.
///
/// The given Bitcoin network is only used to build the recipient IDs of the blinded receives the
/// transitions pay (their concealed seals in output), so it has to be the network of the assets.
///
/// Without a wallet the allocations the transitions spend aren't known, so an input only reports
/// the operation output it spends, which its bundle commits to, unless another transition of the
/// same bundle assigns it: [`RgbInputInfo::vin`] and [`RgbInputInfo::assignment`] are `None`.
/// Which outputs are the signer's isn't known either ([`RgbOutputInfo::is_ours`] is `None`), and
/// the assignment types of the transitions aren't checked against the schemas of their assets,
/// which the fascia doesn't carry.
#[cfg_attr(
    feature = "wallet",
    doc = "The wallet inspection ([`crate::wallet::RgbWalletOpsOffline::inspect_fascia`]) reports \
           all of it."
)]
#[cfg_attr(
    not(feature = "wallet"),
    doc = "The wallet inspection (`wallet::RgbWalletOpsOffline::inspect_fascia`, available with \
           the `wallet` feature) reports all of it."
)]
///
/// This doesn't validate the operations: a successful inspection doesn't mean they are valid, nor
/// that the transaction committing to them has been signed or broadcast.
pub fn inspect_fascia(
    psbt: String,
    fascia_path: String,
    entropy: Option<u64>,
    max_fascia_size: u64,
    max_bundles: u32,
    bitcoin_network: BitcoinNetwork,
) -> Result<RgbInspection, Error> {
    // check request data validity
    let psbt = Psbt::from_str(&psbt)?;
    let fascia = fascia_from_file(&fascia_path, max_fascia_size)?;

    fascia_inspection(
        &psbt,
        &fascia,
        entropy,
        max_bundles,
        |contract_id, bundle, witness_tx| {
            bundle_info(
                contract_id,
                None,
                bitcoin_network.into(),
                bundle,
                witness_tx,
                true,
                false,
                |_| None,
                |_| None,
                |_| None,
            )
        },
    )
}

/// Read the fascia the inspection is given the path of, refusing a file larger than the given size
/// before decoding it.
pub(crate) fn fascia_from_file(fascia_path: &str, max_fascia_size: u64) -> Result<Fascia, Error> {
    let invalid_file_path = || Error::InvalidFilePath {
        file_path: fascia_path.to_string(),
    };
    // the size is checked on the opened file, which is then read only up to the max size
    let file = fs::File::open(fascia_path).map_err(|_| invalid_file_path())?;
    let file_metadata = file.metadata()?;
    if !file_metadata.is_file() {
        return Err(invalid_file_path());
    }
    if file_metadata.len() > max_fascia_size {
        return Err(Error::FileTooLarge {
            file_path: fascia_path.to_string(),
            max_size: max_fascia_size,
        });
    }
    let mut reader = StrictReader::with(StreamReader::new::<FASCIA_MAX_LEN>(
        file.take(max_fascia_size),
    ));
    let fascia = Fascia::strict_decode(&mut reader).map_err(|_| invalid_file_path())?;
    // as when reading a fascia the wallet saved, the whole file has to be the fascia
    let mut file = reader.unbox().unconfine().into_inner();
    if file.stream_position()? != file.seek(io::SeekFrom::End(0))? {
        return Err(invalid_file_path());
    }
    Ok(fascia)
}

/// Build the inspection details of a fascia, inspecting each of its bundles with the given
/// function (called with the contract ID of the bundle and the witness transaction).
///
/// The fascia is checked to be the one of the PSBT transaction and to have at most `max_bundles`
/// bundles, before inspecting them. The PSBT transaction is then checked to commit to the MPC
/// commitment rebuilt from the inspected bundles with the given entropy, so to these bundles
/// only. The entropy defaults to the one the MPC commitment of the fascia carries, which only a
/// concealed commitment doesn't, and a given one has to be the carried one.
pub(crate) fn fascia_inspection(
    psbt: &Psbt,
    fascia: &Fascia,
    entropy: Option<u64>,
    max_bundles: u32,
    mut bundle_operation: impl FnMut(
        ContractId,
        &TransitionBundle,
        &BdkTransaction,
    ) -> Result<RgbOperationInfo, Error>,
) -> Result<RgbInspection, Error> {
    // verify the fascia's witness ID matches the PSBT's transaction ID
    let fascia_witness_id = fascia.witness_id();
    let witness_id = psbt.unsigned_tx.compute_txid();
    if fascia_witness_id != witness_id {
        return Err(Error::RgbInspection {
            details: format!(
                "fascia witness ID {fascia_witness_id} does not match PSBT TXID {witness_id}"
            ),
        });
    }

    // refuse a fascia with too many bundles before inspecting them and rebuilding the MPC
    // commitment, which holds a message per bundle
    let bundle_count = fascia.bundles().len();
    if bundle_count > max_bundles as usize {
        return Err(Error::RgbInspection {
            details: format!(
                "fascia bundle count {bundle_count} exceeds the maximum of {max_bundles}"
            ),
        });
    }

    // inspect the fascia bundles and collect the operations and the commitment messages
    let mut messages = BTreeMap::new();
    let mut operations = Vec::new();
    for (contract_id, bundle) in fascia.bundles() {
        operations.push(bundle_operation(*contract_id, bundle, &psbt.unsigned_tx)?);
        messages.insert(
            ProtocolId::from(*contract_id),
            Message::from(bundle.bundle_id()),
        );
    }

    // the entropy of the commitment, which its merkle block carries unless it's concealed: a
    // given one that isn't the carried one means the fascia isn't the one of the operation
    let entropy = match (entropy, fascia.seal_witness().mpc_merkle_block.entropy()) {
        (Some(given), Some(carried)) if given != carried => {
            return Err(Error::RgbInspection {
                details: format!(
                    "the MPC commitment of the fascia was built with entropy {carried}, not the \
                     given {given}"
                ),
            });
        }
        (Some(entropy), _) | (None, Some(entropy)) => entropy,
        (None, None) => {
            return Err(Error::RgbInspection {
                details: "the MPC commitment of the fascia doesn't carry the entropy it was built \
                          with, which then has to be given"
                    .to_string(),
            });
        }
    };

    // rebuild the MPC commitment of the fascia bundles and verify the PSBT commits to it, which
    // ensures the PSBT commits to these bundles only
    let mut source = MultiSource::with_static_entropy(entropy);
    source.messages = MediumOrdMap::from_checked(messages);
    // the protocol IDs are the contract IDs of the inspected fascia, so a fascia whose contracts
    // can't be fit in a tree, which a valid one always can, is reported as invalid
    let merkle_tree = MerkleTree::try_commit(&source).map_err(|e| Error::RgbInspection {
        details: format!("cannot rebuild the MPC commitment of the fascia bundles: {e}"),
    })?;
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

/// Verify the witness transaction commits to the given MPC commitment with the given DBC proof.
fn verify_witness_commitment(
    dbc_proof: &DbcProof,
    commitment: &Commitment,
    witness_tx: &BdkTransaction,
) -> Result<(), Error> {
    dbc_proof
        .verify(commitment, witness_tx)
        .map_err(|e| Error::RgbInspection {
            details: format!(
                "witness {} doesn't commit to {commitment}: {e}",
                witness_tx.compute_txid()
            ),
        })
}

/// Build the inspection details of the genesis of an asset with the given schema.
fn genesis_info(
    contract_id: ContractId,
    asset_schema: AssetSchema,
    genesis: &Genesis,
) -> Result<RgbGenesisInfo, Error> {
    // decode the first value of a global state type
    fn global<T: StrictDeserialize>(
        contract_id: ContractId,
        genesis: &Genesis,
        global_type: GlobalStateType,
    ) -> Result<Option<T>, Error> {
        genesis
            .globals
            .get(&global_type)
            .and_then(|values| values.first())
            .map(|value| {
                T::from_strict_serialized(value.to_inner()).map_err(|e| Error::RgbInspection {
                    details: format!(
                        "genesis of contract {contract_id}: invalid {global_type} global: {e}"
                    ),
                })
            })
            .transpose()
    }
    let missing = |global_type: GlobalStateType| Error::RgbInspection {
        details: format!("genesis of contract {contract_id}: missing {global_type} global"),
    };

    let (name, ticker, details, precision) = match asset_schema {
        AssetSchema::Cfa => {
            let name: Name = global(contract_id, genesis, GS_NAME)?.ok_or(missing(GS_NAME))?;
            let details: Option<Details> = global(contract_id, genesis, GS_DETAILS)?;
            let precision: Precision =
                global(contract_id, genesis, GS_PRECISION)?.ok_or(missing(GS_PRECISION))?;
            (name, None, details, precision)
        }
        AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Ifa => {
            let spec: AssetSpec =
                global(contract_id, genesis, GS_NOMINAL)?.ok_or(missing(GS_NOMINAL))?;
            let ticker = spec.ticker.to_string();
            (spec.name, Some(ticker), spec.details, spec.precision)
        }
        AssetSchema::Bfa => {
            let ticker: UnicodeTicker =
                global(contract_id, genesis, GS_TICKER)?.ok_or(missing(GS_TICKER))?;
            let name: Name = global(contract_id, genesis, GS_NAME)?.ok_or(missing(GS_NAME))?;
            let details: Option<Details> = global(contract_id, genesis, GS_DETAILS)?;
            let precision: Precision =
                global(contract_id, genesis, GS_PRECISION)?.ok_or(missing(GS_PRECISION))?;
            (name, Some(ticker.to_string()), details, precision)
        }
    };

    // the supply, link, bridge and reject list globals of the schemas defining them. The issued
    // supply and the max supply are mandatory where they're defined, the others are optional
    let issued_supply: Option<Amount> = match asset_schema {
        AssetSchema::Nia | AssetSchema::Cfa | AssetSchema::Ifa => {
            Some(global(contract_id, genesis, GS_ISSUED_SUPPLY)?.ok_or(missing(GS_ISSUED_SUPPLY))?)
        }
        AssetSchema::Uda | AssetSchema::Bfa => None,
    };
    let max_supply: Option<Amount> = match asset_schema {
        AssetSchema::Ifa => {
            Some(global(contract_id, genesis, GS_MAX_SUPPLY)?.ok_or(missing(GS_MAX_SUPPLY))?)
        }
        AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa | AssetSchema::Bfa => None,
    };
    let linked_from_contract: Option<ContractId> = match asset_schema {
        AssetSchema::Ifa | AssetSchema::Bfa => {
            global(contract_id, genesis, GS_LINKED_FROM_CONTRACT)?
        }
        AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa => None,
    };
    let bridge_location = match asset_schema {
        AssetSchema::Bfa => Some(
            BridgedFungibleAsset::bridge_location(genesis)
                .map_err(|e| Error::RgbInspection {
                    details: format!(
                        "genesis of contract {contract_id}: invalid {GS_BRIDGE_LOCATION} global: \
                         {e}"
                    ),
                })?
                .into(),
        ),
        AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Cfa | AssetSchema::Ifa => None,
    };
    // read by the same helper the reject list check uses, so that inspection and enforcement
    // always look at the same global
    let reject_list = validation::genesis_reject_list_source(asset_schema, genesis)
        .map_err(|e| Error::RgbInspection {
            details: format!("genesis of contract {contract_id}: invalid reject list global: {e}"),
        })?
        .map(RgbRejectListInfo::from);

    // the media of the asset, read from the same fields the wallet extracts its attachments from
    let terms: ContractTerms = global(contract_id, genesis, GS_TERMS)?.ok_or(missing(GS_TERMS))?;
    let mut media: Vec<RgbMediaInfo> = terms.media.iter().map(RgbMediaInfo::from).collect();
    if matches!(asset_schema, AssetSchema::Uda) {
        let token_data: TokenData =
            global(contract_id, genesis, GS_TOKENS)?.ok_or(missing(GS_TOKENS))?;
        media.extend(token_data.media.iter().map(RgbMediaInfo::from));
        media.extend(token_data.attachments.values().map(RgbMediaInfo::from));
    }

    Ok(RgbGenesisInfo {
        chain_net: genesis.chain_net.prefix().to_string(),
        timestamp: genesis.timestamp,
        issuer: genesis.issuer.to_string(),
        name: name.to_string(),
        ticker,
        details: details.map(|d| d.to_string()),
        precision: precision.into(),
        issued_supply: issued_supply.map(u64::from),
        max_supply: max_supply.map(u64::from),
        linked_from_contract: linked_from_contract.map(|id| id.to_string()),
        bridge_location,
        reject_list,
        media,
        outputs: outputs_info(
            &format!("genesis of contract {contract_id}"),
            Some(asset_schema),
            &genesis.assignments,
            genesis.chain_net,
            None,
            |_| None,
            Some(false),
        )?,
    })
}

/// Build the inspection details of the operation of a bundle, given its witness transaction, the
/// schema of its asset (if known) and the functions resolving an input to the outpoint it's
/// allocated to and its assignment, telling if an output seal is ours and if a transition is a
/// terminal operation.
///
/// The bundle ID only commits to the bundle input map, which binds each input to the ID of the
/// transition spending it, so, if requested, transitions whose ID isn't the one committed for their
/// inputs are rejected, as they're not the ones the bundle commits to. Transitions of a contract
/// other than the one the bundle is filed under are rejected as well, as nothing commits to it,
/// and so are transitions listed twice, as the input map binds each input to a single transition.
///
/// Every input is expected to be resolved, by the given function or by another transition of the
/// same bundle, unless `resolve_inputs` is false, as a fascia inspected without a wallet has
/// nothing to resolve the spent allocations from: such an input only reports its opout. An input
/// spending another transition of the same bundle is expected to be resolved in any case, as the
/// bundle carries the assignment it spends.
pub(crate) fn bundle_info(
    contract_id: ContractId,
    asset_schema: Option<AssetSchema>,
    chain_net: ChainNet,
    bundle: &TransitionBundle,
    witness_tx: &BdkTransaction,
    check_commitments: bool,
    resolve_inputs: bool,
    mut resolve_input: impl FnMut(RgbOpout) -> Option<(OutPoint, Assignment)>,
    mut is_ours: impl FnMut(&BuilderSeal<GraphSeal>) -> Option<bool>,
    mut is_terminal: impl FnMut(&OpId) -> Option<bool>,
) -> Result<RgbOperationInfo, Error> {
    // looked up for each input, as one may be assigned by another transition of the bundle
    let mut bundle_transitions: HashMap<OpId, &Transition> = HashMap::new();
    for KnownTransition { opid, transition } in bundle.known_transitions.iter() {
        // the transitions are a list, so the same one can be listed twice: as the input map binds
        // each input to a single transition, it would be reported (and its state counted) twice
        if bundle_transitions.insert(*opid, transition).is_some() {
            return Err(Error::RgbInspection {
                details: format!(
                    "bundle for contract {contract_id}: transition {opid} is listed twice"
                ),
            });
        }
    }
    let mut transitions = vec![];
    for KnownTransition { opid, transition } in bundle.known_transitions.iter() {
        // nothing commits to the contract a bundle is filed under, so a transition of another
        // contract would be reported as one of this contract, while validating the operation,
        // which checks the contract of each transition, would reject it
        if transition.contract_id != contract_id {
            return Err(Error::RgbInspection {
                details: format!(
                    "bundle for contract {contract_id}: transition {opid} is of contract {}",
                    transition.contract_id
                ),
            });
        }
        let inputs = transition.inputs();
        let committed = !check_commitments
            || transition.id() == *opid
                && !inputs.is_empty()
                && inputs
                    .iter()
                    .all(|opout| bundle.input_map.get(opout) == Some(opid));
        if !committed {
            return Err(Error::RgbInspection {
                details: format!(
                    "bundle for contract {contract_id}: transition {opid} is not committed"
                ),
            });
        }
        let mut inputs_info = vec![];
        for opout in inputs {
            // an input spending another transition of the bundle is resolved by the bundle itself:
            // if it isn't, that transition doesn't assign it (so the spending one doesn't spend
            // what it claims) or assigns it to a seal the witness can't spend (a concealed one or
            // one of the witness transaction itself, which this transaction can't spend)
            let in_bundle = bundle_transitions.contains_key(&opout.op);
            let resolved =
                bundle_output(&bundle_transitions, opout).or_else(|| resolve_input(opout));
            let (vin, assignment) = match resolved {
                Some((outpoint, assignment)) => {
                    let vin = witness_tx
                        .input
                        .iter()
                        .position(|i| i.previous_output == outpoint)
                        .ok_or_else(|| Error::RgbInspection {
                            details: format!(
                                "bundle for contract {contract_id}: input {opout} not spent by \
                                 witness {}",
                                witness_tx.compute_txid()
                            ),
                        })?;
                    (Some(vin as u32), Some(assignment))
                }
                None if in_bundle => {
                    return Err(Error::RgbInspection {
                        details: format!(
                            "bundle for contract {contract_id}: input {opout} isn't assigned by \
                             the bundle to an outpoint the witness spends"
                        ),
                    });
                }
                None if resolve_inputs => {
                    return Err(Error::RgbInspection {
                        details: format!(
                            "bundle for contract {contract_id}: unknown input {opout}"
                        ),
                    });
                }
                None => (None, None),
            };
            inputs_info.push(RgbInputInfo {
                opout: opout.try_into().map_err(|ty| Error::RgbInspection {
                    details: format!(
                        "bundle for contract {contract_id}: unknown assignment type: {ty}"
                    ),
                })?,
                vin,
                assignment,
            });
        }
        transitions.push(transition_info(
            contract_id,
            asset_schema,
            transition,
            chain_net,
            witness_tx.compute_txid(),
            inputs_info,
            &mut is_ours,
            is_terminal(opid),
        )?);
    }
    Ok(RgbOperationInfo {
        asset_id: contract_id.to_string(),
        committed_transition_count: bundle.input_map_opids().len() as u32,
        committed_input_count: bundle.input_map.len() as u32,
        transitions,
    })
}

/// The outpoint and assignment of `opout`, if it's assigned by one of the transitions of a bundle
/// (given by OpId, e.g. the split a burn spends) to an outpoint the witness transaction spends.
fn bundle_output(
    bundle_transitions: &HashMap<OpId, &Transition>,
    opout: RgbOpout,
) -> Option<(OutPoint, Assignment)> {
    let transition = bundle_transitions.get(&opout.op)?;
    let assigns = transition.assignments.get(&opout.ty)?;
    let seal = assigns.revealed_seal_at(opout.no).ok().flatten()?;
    let TxPtr::Txid(txid) = seal.txid else {
        return None;
    };
    let assignment = Assignment::from_typed_assigns(opout.ty, assigns, opout.no)?;
    Some((OutPoint::new(txid, seal.vout.into_u32()), assignment))
}

/// Build the inspection details of a transition, given its witness TXID, its already resolved
/// inputs, the function telling if an output seal is ours and whether it's a terminal operation.
fn transition_info(
    contract_id: ContractId,
    asset_schema: Option<AssetSchema>,
    transition: &Transition,
    chain_net: ChainNet,
    witness_txid: Txid,
    inputs: Vec<RgbInputInfo>,
    mut is_ours: impl FnMut(&BuilderSeal<GraphSeal>) -> Option<bool>,
    is_terminal: Option<bool>,
) -> Result<RgbTransitionInfo, Error> {
    let r#type = TypeOfTransition::try_from(transition.transition_type).map_err(|_| {
        Error::RgbInspection {
            details: format!(
                "bundle for contract {contract_id}: unknown transition type: {}",
                transition.transition_type
            ),
        }
    })?;

    let outputs = outputs_info(
        &format!("bundle for contract {contract_id}"),
        asset_schema,
        &transition.assignments,
        chain_net,
        Some(witness_txid),
        &mut is_ours,
        is_terminal,
    )?;

    // collect transition globals
    let mut globals = vec![];
    for (global_type, values) in transition.globals.iter() {
        for value in values.iter() {
            let decode_amount = |name: &str| {
                Amount::from_strict_serialized(value.to_inner())
                    .map(u64::from)
                    .map_err(|e| Error::RgbInspection {
                        details: format!(
                            "bundle for contract {contract_id}: invalid {name} global: {e}"
                        ),
                    })
            };
            let global = match *global_type {
                GS_ISSUED_SUPPLY => {
                    TransitionGlobal::IssuedSupply(decode_amount(RGB_GLOBAL_ISSUED_SUPPLY)?)
                }
                GS_BURNED_ASSET => {
                    TransitionGlobal::BurnedAsset(decode_amount(RGB_GLOBAL_BURNED_ASSET)?)
                }
                GS_BURNED_INFLATION => {
                    TransitionGlobal::BurnedInflation(decode_amount(RGB_GLOBAL_BURNED_INFLATION)?)
                }
                GS_BURN_REASON => TransitionGlobal::BurnReason(
                    BurnReason::from_strict_serialized(value.to_inner())
                        .map_err(|e| Error::RgbInspection {
                            details: format!(
                                "bundle for contract {contract_id}: invalid {RGB_GLOBAL_BURN_REASON} global: {e}"
                            ),
                        })?
                        .to_string(),
                ),
                GS_LINKED_TO_CONTRACT => TransitionGlobal::LinkedToContract(
                    ContractId::from_strict_serialized(value.to_inner())
                        .map_err(|e| Error::RgbInspection {
                            details: format!(
                                "bundle for contract {contract_id}: invalid {RGB_GLOBAL_LINKED_TO_CONTRACT} global: {e}"
                            ),
                        })?
                        .to_string(),
                ),
                _ => {
                    return Err(Error::RgbInspection {
                        details: format!(
                            "bundle for contract {contract_id}: unknown global state type: {global_type}"
                        ),
                    });
                }
            };
            globals.push(global);
        }
    }

    // collect transition metadata
    let mut metadata = vec![];
    for (meta_type, value) in transition.metadata.iter() {
        let invalid = |name: &str, e: DeserializeError| Error::RgbInspection {
            details: format!("bundle for contract {contract_id}: invalid {name} metadata: {e}"),
        };
        let meta = match *meta_type {
            MS_ALLOWED_INFLATION => TransitionMetadata::AllowedInflation(
                Amount::from_strict_serialized(value.to_inner())
                    .map_err(|e| invalid(RGB_METADATA_ALLOWED_INFLATION, e))?
                    .into(),
            ),
            MS_BEFORE_BLOCK => TransitionMetadata::BeforeBlock(
                BlockNumber::from_strict_serialized(value.to_inner())
                    .map_err(|e| invalid(RGB_METADATA_BEFORE_BLOCK, e))?
                    .into_inner(),
            ),
            _ => {
                return Err(Error::RgbInspection {
                    details: format!(
                        "bundle for contract {contract_id}: unknown metadata type: {meta_type}"
                    ),
                });
            }
        };
        metadata.push(meta);
    }

    Ok(RgbTransitionInfo {
        r#type,
        opid: transition.id().to_string(),
        inputs,
        outputs,
        globals,
        metadata,
    })
}

/// Build the inspection details of the outputs of an operation, given the context to report in
/// errors, the schema of its contract (if known), its chain and network, the TXID its seals
/// pointing to the witness transaction refer to, the function telling if an output seal is ours and
/// whether the operation is a terminal one.
///
/// Assignment types rgb-lib can't represent are rejected, telling apart the ones defined by the
/// schema but not supported yet from the ones unknown to it. Without the schema, which a fascia
/// inspected without a wallet doesn't carry, the assignment types are only checked to be ones
/// rgb-lib can represent, not to be allowed by the schema of the asset.
fn outputs_info<Id: SealTxid>(
    context: &str,
    asset_schema: Option<AssetSchema>,
    assignments: &Assignments<BlindSeal<Id>>,
    chain_net: ChainNet,
    witness_txid: Option<Txid>,
    mut is_ours: impl FnMut(&BuilderSeal<BlindSeal<Id>>) -> Option<bool>,
    is_terminal: Option<bool>,
) -> Result<Vec<RgbOutputInfo>, Error>
where
    BlindSeal<Id>: ExposedSeal,
{
    let mut outputs = vec![];
    for (ass_type, typed_assigns) in assignments.iter() {
        for no in 0..typed_assigns.len_u16() {
            let Some(assignment) = Assignment::from_typed_assigns(*ass_type, typed_assigns, no)
                .filter(|a| asset_schema.is_none_or(|schema| schema.supports_assignment(a)))
            else {
                let in_schema = asset_schema
                    .is_some_and(|schema| schema.schema().owned_types.contains_key(ass_type));
                let kind = if in_schema {
                    "currently unsupported"
                } else {
                    "unknown"
                };
                return Err(Error::RgbInspection {
                    details: format!("{context}: {kind} assignment type: {ass_type}"),
                });
            };
            let seal = typed_assigns.seal_at(no).expect("index in range");
            let (txid, vout, blinding, recipient_id) = match seal {
                BuilderSeal::Revealed(seal) => (
                    seal.txid().or(witness_txid).map(|t| t.to_string()),
                    Some(seal.vout().into_u32()),
                    Some(seal.blinding),
                    None,
                ),
                BuilderSeal::Concealed(seal) => {
                    let beneficiary = Beneficiary::BlindedSeal(*seal);
                    (
                        None,
                        None,
                        None,
                        Some(XChainNet::with(chain_net, beneficiary).to_string()),
                    )
                }
            };
            outputs.push(RgbOutputInfo {
                txid,
                vout,
                blinding,
                assignment,
                is_concealed: recipient_id.is_some(),
                recipient_id,
                is_ours: is_ours(seal),
                is_terminal,
            });
        }
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid PSBT, of a transaction unrelated to any fascia.
    const PSBT: &str = "cHNidP8BAF4CAAAAATQi69UHNVN1H3GVJyjCt2qx9Xsmt56SLSMwGM/GQxgBAQAAAAD9////AfQBAAAAAAAAIlEgoK74YaTaHlE4t4tfisItYxVkOmBakMt96x+kMAS6ArcZCgAAAAEBK+gDAAAAAAAAIlEgjWk7HP1AQKe/fp/RWQmQzRsfIQWHq+fWUteGf/YfEy0hFoJzo2OMevtseGK/uBkC+bautE/IgnmbDCz6eMojamp4GQA39GtzVgAAgB+fDIAAAACAAAAAAAAAAAABFyCCc6NjjHr7bHhiv7gZAvm2rrRPyIJ5mwws+njKI2pqeAAA";

    #[test]
    fn inspect_fascia_invalid_file() {
        let inspect = |psbt: &str, fascia_path: &str| {
            inspect_fascia(
                psbt.to_string(),
                fascia_path.to_string(),
                None,
                u64::MAX,
                1,
                BitcoinNetwork::Regtest,
            )
        };

        // invalid PSBT
        let result = inspect("not a PSBT", "inexistent");
        assert_matches!(result, Err(Error::InvalidPsbt { details: _ }));

        // inexistent file
        let result = inspect(PSBT, "inexistent");
        assert!(
            matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == "inexistent")
        );

        // not a fascia
        let file = tempfile::NamedTempFile::with_prefix("inspect_fascia::").unwrap();
        std::fs::write(file.path(), b"not a fascia").unwrap();
        let file_path = file.path().to_string_lossy().to_string();
        let result = inspect(PSBT, &file_path);
        assert!(matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == file_path));
    }

    #[test]
    fn inspect_consignment_invalid_file() {
        // inexistent file
        let result = inspect_consignment("inexistent".to_string(), BundleFilter::Terminals);
        assert!(
            matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == "inexistent")
        );

        // not a consignment
        let file = tempfile::NamedTempFile::with_prefix("inspect_consignment::").unwrap();
        std::fs::write(file.path(), b"not a consignment").unwrap();
        let file_path = file.path().to_string_lossy().to_string();
        let result = inspect_consignment(file_path.clone(), BundleFilter::All);
        assert_matches!(result, Err(Error::InvalidConsignment));

        // invalid witness TXID
        let result = inspect_consignment(file_path, BundleFilter::Witness("invalid".to_string()));
        assert_matches!(result, Err(Error::InvalidTxid));
    }

    #[test]
    fn opout_assignment_types() {
        let opid = OpId::from([1; 32]);
        let opout = |ty| Opout::try_from(RgbOpout::new(opid, ty, 3));

        assert_eq!(
            opout(OS_ASSET).unwrap(),
            Opout {
                opid: opid.to_string(),
                assignment_type: TypeOfAssignment::Asset,
                index: 3,
            }
        );
        assert_eq!(
            opout(OS_INFLATION).unwrap().assignment_type,
            TypeOfAssignment::InflationRight
        );
        assert_eq!(
            opout(OS_LINK).unwrap().assignment_type,
            TypeOfAssignment::LinkRight
        );
        assert_eq!(
            opout(OS_MINT).unwrap().assignment_type,
            TypeOfAssignment::MintRight
        );

        // an assignment type no supported schema defines
        let unknown = AssignmentType::with(u16::MAX);
        assert_eq!(opout(unknown), Err(unknown));
    }

    #[test]
    fn outputs_info_unsupported_assignment_type() {
        // build assignments with a single declarative output of the given type
        let assignments = |ty: AssignmentType| {
            let assign = Assign::with(
                BuilderSeal::Revealed(GraphSeal::new_random_vout(0)),
                VoidState::default(),
            );
            let mut assignments = Assignments::<GraphSeal>::default();
            assignments
                .insert(
                    ty,
                    TypedAssigns::Declarative(AssignVec::with(NonEmptyVec::with(assign))),
                )
                .unwrap();
            assignments
        };
        let outputs = |asset_schema: Option<AssetSchema>, ty: AssignmentType| {
            outputs_info(
                "ctx",
                asset_schema,
                &assignments(ty),
                ChainNet::BitcoinRegtest,
                None,
                |_| None,
                None,
            )
        };

        // supported types
        let result = outputs(Some(AssetSchema::Ifa), OS_LINK).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].assignment, Assignment::LinkRight);
        let result = outputs(Some(AssetSchema::Bfa), OS_MINT).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].assignment, Assignment::MintRight);

        // type unknown to the schema
        let result = outputs(Some(AssetSchema::Nia), OS_LINK);
        assert_matches!(
            result,
            Err(Error::RgbInspection { details })
                if details == format!("ctx: unknown assignment type: {OS_LINK}")
        );

        // without a schema the type is only checked to be one rgb-lib can represent
        let result = outputs(None, OS_LINK).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].assignment, Assignment::LinkRight);
        let result = outputs(None, AssignmentType::with(u16::MAX));
        assert_matches!(
            result,
            Err(Error::RgbInspection { details })
                if details == format!("ctx: unknown assignment type: {}", u16::MAX)
        );
    }
}
