//! Consignment inspection.
//!
//! This module defines the consignment inspection, which doesn't require a wallet, and the objects
//! it returns, some of which are shared with wallet methods (e.g. the PSBT inspection ones).

use super::*;

/// Information about an RGB input being spent.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbInputInfo {
    /// The input index in the transaction
    pub vin: u32,
    /// The assignment in input
    pub assignment: Assignment,
}

/// Information about an RGB output destination.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RgbOutputInfo {
    /// The output TXID (if the seal is revealed)
    pub txid: Option<String>,
    /// The output vout (if the seal is revealed)
    pub vout: Option<u32>,
    /// The assignment in output
    pub assignment: Assignment,
    /// Whether this output is allocated to a concealed seal
    pub is_concealed: bool,
    /// The recipient ID of the blinded receive the output is allocated to (if the seal is
    /// concealed)
    pub recipient_id: Option<String>,
    /// Whether this output belongs to the wallet (`None` when inspecting without a wallet)
    pub is_ours: Option<bool>,
    /// Whether this output is a terminal of the consignment, meaning it's allocated to the
    /// consignment recipient (`None` when not inspecting a consignment)
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
}

/// A metadata entry committed in an RGB transition, read by its validation but not part of the
/// contract state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum TransitionMetadata {
    /// Amount of inflation rights allowed in output, committed in IFA inflate transitions
    AllowedInflation(u64),
    /// External chain block after which the lock event backing the mint must be searched,
    /// committed in BFA mint transitions
    AfterBlock(u64),
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
    /// Details of the inspected witness transactions (the ones of the terminals or all of them), in
    /// consignment order (none for a contract consignment)
    pub witnesses: Vec<RgbInspection>,
}

/// The bundles of a consignment to inspect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum BundleFilter {
    /// All the consignment bundles
    All,
    /// The bundles containing the consignment terminals
    Terminals,
    /// The bundle committed by the witness transaction with the given TXID
    Witness(String),
}

/// Inspect the RGB consignment (transfer or contract) at the given path to show information about
/// the asset and the transitions committed by the witness transactions of the bundles selected by
/// the given filter.
///
/// Inputs are resolved from the consignment history, outputs report whether they're allocated to
/// the consignment recipient. The witness transactions are verified to commit to the inspected
/// transitions, and the transitions their inputs are resolved from to have the operation ID they
/// are referred to by, also for a validated consignment file. This doesn't validate the
/// consignment: a successful inspection doesn't mean the consignment is valid nor that its witness
/// transactions have been broadcast.
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
/// function (called with their witness TXID and whether they contain terminals) and checking the
/// witness transactions commit to the inspected transitions.
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
    let resolve_input = |opout: Opout| -> Option<(OutPoint, Assignment)> {
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
    let mut witnesses = vec![];
    for witness_bundle in consignment.bundles.iter() {
        let bundle_id = witness_bundle.bundle.bundle_id();
        let txid = witness_bundle.witness_id();
        let terminal_seals = consignment.terminals.get(&bundle_id);
        if !selected(txid, terminal_seals.is_some()) {
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

        let terminal_seals: HashSet<SecretSeal> = terminal_seals
            .map(|seals| seals.into_iter().map(|s| s.to_secret_seal()).collect())
            .unwrap_or_default();
        let operation = bundle_info(
            contract_id,
            asset_schema,
            consignment.genesis.chain_net,
            &witness_bundle.bundle,
            tx,
            true,
            resolve_input,
            |_| None,
            |seal| Some(terminal_seals.contains(&seal.to_secret_seal())),
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

/// Verify the witness transaction commits to the given MPC commitment with the given DBC proof.
pub(crate) fn verify_witness_commitment(
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
        AssetSchema::Nia | AssetSchema::Uda | AssetSchema::Ifa | AssetSchema::Bfa => {
            let spec: AssetSpec =
                global(contract_id, genesis, GS_NOMINAL)?.ok_or(missing(GS_NOMINAL))?;
            (spec.name, Some(spec.ticker), spec.details, spec.precision)
        }
    };

    Ok(RgbGenesisInfo {
        chain_net: genesis.chain_net.prefix().to_string(),
        timestamp: genesis.timestamp,
        issuer: genesis.issuer.to_string(),
        name: name.to_string(),
        ticker: ticker.map(|t| t.to_string()),
        details: details.map(|d| d.to_string()),
        precision: precision.into(),
        outputs: outputs_info(
            &format!("genesis of contract {contract_id}"),
            asset_schema,
            &genesis.assignments,
            genesis.chain_net,
            None,
            |_| None,
            |_| Some(false),
        )?,
    })
}

/// Build the inspection details of the operation of a bundle, given its witness transaction and
/// the functions resolving an input to the outpoint it's allocated to and its assignment and telling
/// if an output seal is ours and if it's a terminal.
///
/// The bundle ID only commits to the bundle input map, which binds each input to the ID of the
/// transition spending it, so, if requested, transitions whose ID isn't the one committed for their
/// inputs are rejected, as they're not the ones the bundle commits to.
pub(crate) fn bundle_info(
    contract_id: ContractId,
    asset_schema: AssetSchema,
    chain_net: ChainNet,
    bundle: &TransitionBundle,
    witness_tx: &BdkTransaction,
    check_commitments: bool,
    mut resolve_input: impl FnMut(Opout) -> Option<(OutPoint, Assignment)>,
    mut is_ours: impl FnMut(&BuilderSeal<GraphSeal>) -> Option<bool>,
    mut is_terminal: impl FnMut(&BuilderSeal<GraphSeal>) -> Option<bool>,
) -> Result<RgbOperationInfo, Error> {
    let mut transitions = vec![];
    for KnownTransition { opid, transition } in bundle.known_transitions.iter() {
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
            let (outpoint, assignment) =
                resolve_input(opout).ok_or_else(|| Error::RgbInspection {
                    details: format!("bundle for contract {contract_id}: unknown input {opout}"),
                })?;
            let vin = witness_tx
                .input
                .iter()
                .position(|i| i.previous_output == outpoint)
                .ok_or_else(|| Error::RgbInspection {
                    details: format!(
                        "bundle for contract {contract_id}: input {opout} not spent by witness {}",
                        witness_tx.compute_txid()
                    ),
                })?;
            inputs_info.push(RgbInputInfo {
                vin: vin as u32,
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
            &mut is_terminal,
        )?);
    }
    Ok(RgbOperationInfo {
        asset_id: contract_id.to_string(),
        committed_transition_count: bundle.input_map_opids().len() as u32,
        committed_input_count: bundle.input_map.len() as u32,
        transitions,
    })
}

/// Build the inspection details of a transition, given its witness TXID, its already resolved
/// inputs and the functions telling if an output seal is ours and if it's a terminal.
fn transition_info(
    contract_id: ContractId,
    asset_schema: AssetSchema,
    transition: &Transition,
    chain_net: ChainNet,
    witness_txid: Txid,
    inputs: Vec<RgbInputInfo>,
    mut is_ours: impl FnMut(&BuilderSeal<GraphSeal>) -> Option<bool>,
    mut is_terminal: impl FnMut(&BuilderSeal<GraphSeal>) -> Option<bool>,
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
        &mut is_terminal,
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
            MS_AFTER_BLOCK => TransitionMetadata::AfterBlock(
                BlockNumber::from_strict_serialized(value.to_inner())
                    .map_err(|e| invalid(RGB_METADATA_AFTER_BLOCK, e))?
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
/// errors, the contract schema, chain and network, the TXID its seals pointing to the witness
/// transaction refer to and the functions telling if an output seal is ours and if it's a
/// terminal.
///
/// Assignment types rgb-lib can't represent are rejected, telling apart the ones defined by the
/// schema but not supported yet (e.g. link rights) from the ones unknown to it.
fn outputs_info<Seal: ExposedSeal + TxoSeal>(
    context: &str,
    asset_schema: AssetSchema,
    assignments: &Assignments<Seal>,
    chain_net: ChainNet,
    witness_txid: Option<Txid>,
    mut is_ours: impl FnMut(&BuilderSeal<Seal>) -> Option<bool>,
    mut is_terminal: impl FnMut(&BuilderSeal<Seal>) -> Option<bool>,
) -> Result<Vec<RgbOutputInfo>, Error> {
    let mut outputs = vec![];
    for (ass_type, typed_assigns) in assignments.iter() {
        for no in 0..typed_assigns.len_u16() {
            let Some(assignment) = Assignment::from_typed_assigns(*ass_type, typed_assigns, no)
            else {
                let in_schema = asset_schema.schema().owned_types.contains_key(ass_type);
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
            let (txid, vout, recipient_id) = match seal {
                BuilderSeal::Revealed(seal) => (
                    seal.txid().or(witness_txid).map(|t| t.to_string()),
                    Some(seal.vout().into_u32()),
                    None,
                ),
                BuilderSeal::Concealed(seal) => {
                    let beneficiary = Beneficiary::BlindedSeal(*seal);
                    (
                        None,
                        None,
                        Some(XChainNet::with(chain_net, beneficiary).to_string()),
                    )
                }
            };
            outputs.push(RgbOutputInfo {
                txid,
                vout,
                assignment,
                is_concealed: recipient_id.is_some(),
                recipient_id,
                is_ours: is_ours(seal),
                is_terminal: is_terminal(seal),
            });
        }
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use amplify::confinement::NonEmptyVec;
    use rgbstd::{Assign, VoidState, assignments::AssignVec};
    use schemata::OS_LINK;

    use super::*;

    #[test]
    fn inspect_consignment_invalid_file() {
        // inexistent file
        let result = inspect_consignment("inexistent".to_string(), BundleFilter::Terminals);
        assert!(
            matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == "inexistent")
        );

        // not a consignment
        let file_path = std::env::temp_dir().join("rgb_lib_inspect_consignment_invalid_file");
        std::fs::write(&file_path, b"not a consignment").unwrap();
        let result =
            inspect_consignment(file_path.to_string_lossy().to_string(), BundleFilter::All);
        assert_matches!(result, Err(Error::InvalidConsignment));

        // invalid witness TXID
        let result = inspect_consignment(
            file_path.to_string_lossy().to_string(),
            BundleFilter::Witness("invalid".to_string()),
        );
        assert_matches!(result, Err(Error::InvalidTxid));
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
        let outputs = |asset_schema: AssetSchema, ty: AssignmentType| {
            outputs_info(
                "ctx",
                asset_schema,
                &assignments(ty),
                ChainNet::BitcoinRegtest,
                None,
                |_| None,
                |_| None,
            )
        };

        // supported type
        let result = outputs(AssetSchema::Bfa, OS_MINT).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].assignment, Assignment::MintRight);

        // type defined by the schema but not supported by rgb-lib
        let result = outputs(AssetSchema::Ifa, OS_LINK);
        assert_matches!(
            result,
            Err(Error::RgbInspection { details })
                if details == format!("ctx: currently unsupported assignment type: {OS_LINK}")
        );

        // type unknown to the schema
        let result = outputs(AssetSchema::Nia, OS_LINK);
        assert_matches!(
            result,
            Err(Error::RgbInspection { details })
                if details == format!("ctx: unknown assignment type: {OS_LINK}")
        );
    }
}
