//! Persistence of the RGB data ([`Stock`]) inside rgb-lib's own database.
//!
//! The rgb-ops [`RgbStore`] trait is implemented directly on [`&DbTxn`](DbTxn), so that the RGB
//! data is read and written through the very same transaction as the rest of the wallet data and
//! committed with it. Transaction control is left entirely to the host transaction.
//!
//! The tables mirror the reference rgb-ops SQLite schema (names prefixed with `rgb_`, and the
//! natural keys kept as unique indexes next to rgb-lib's auto-increment `idx` primary keys). All
//! RGB ids and values are stored as strict-encoded blobs.
//!
//! The module also holds the one-time migration of the legacy filesystem RGB store,
//! into the database, and the legacy consignment files, on disk.

use super::*;

use crate::database::entities::{
    rgb_aluvm_lib, rgb_assignment, rgb_bundle, rgb_bundle_contract, rgb_bundle_witness,
    rgb_genesis, rgb_global_state, rgb_invalid_op, rgb_op_bundle, rgb_op_input, rgb_outpoint_opout,
    rgb_schema, rgb_schema_type_lib, rgb_secret_seal, rgb_secret_seal_opout, rgb_type_lib,
    rgb_witness, rgb_witness_ord,
};

/// Max number of values bound in a single query.
const QUERY_CHUNK: usize = 400;

/// Max number of outpoints matched in a single query, as each one binds 2 values.
const OUTPOINT_CHUNK: usize = QUERY_CHUNK / 2;

// `witness_ords.status` values, mirroring the `WitnessOrd` strict-encoding tag order
const ORD_MINED: u8 = 0;
const ORD_TENTATIVE: u8 = 1;
const ORD_IGNORED: u8 = 2;
const ORD_ARCHIVED: u8 = 3;

type OrdColumns = (u8, Option<u32>, Option<i64>, Option<u8>);

fn enc(val: &impl StrictEncode) -> Result<Vec<u8>, Error> {
    encode(val).map_err(|e| Error::Internal {
        details: format!("cannot serialize RGB data: {e}"),
    })
}

fn dec<T: StrictDecode>(blob: &[u8]) -> Result<T, Error> {
    decode(blob).map_err(|e| Error::Internal {
        details: format!("corrupted RGB data: {e}"),
    })
}

fn collect_decoded<M, T>(
    rows: Result<Vec<M>, Error>,
    decode: impl Fn(M) -> Result<T, Error>,
) -> std::vec::IntoIter<Result<T, Error>> {
    match rows {
        Ok(rows) => rows.into_iter().map(decode).collect::<Vec<_>>().into_iter(),
        Err(e) => vec![Err(e)].into_iter(),
    }
}

fn alloc_kind_str(kind: AllocKind) -> &'static str {
    match kind {
        AllocKind::Fungible => "F",
        AllocKind::Structured => "S",
        AllocKind::Declarative => "R",
    }
}

fn alloc_kind_from_str(s: &str) -> Option<AllocKind> {
    match s {
        "F" => Some(AllocKind::Fungible),
        "S" => Some(AllocKind::Structured),
        "R" => Some(AllocKind::Declarative),
        _ => None,
    }
}

fn assignment_to_row(m: &DbAssignment) -> Result<AllocationRow, Error> {
    let kind = alloc_kind_from_str(&m.state_type).ok_or_else(|| Error::Internal {
        details: format!("corrupted RGB data: unknown state type {}", m.state_type),
    })?;
    let opid = dec::<OpId>(&m.opid)?;
    let opout = Opout::new(opid, AssignmentType::with(m.type_id), m.output_no);
    let seal = AllocSeal {
        txid: m.seal_txid.as_deref().map(dec::<RgbTxid>).transpose()?,
        vout: Vout::from_u32(m.seal_vout),
    };
    let bundle_id = m.bundle_id.as_deref().map(dec::<BundleId>).transpose()?;
    Ok(AllocationRow {
        kind,
        opout,
        seal,
        bundle_id,
        value: m.value_blob.clone(),
    })
}

fn global_state_to_row(m: &DbGlobalState) -> Result<GlobalStateRow, Error> {
    Ok(GlobalStateRow {
        opid: dec::<OpId>(&m.opid)?,
        index: m.out_index,
        nonce: m.nonce.parse().expect("DB should contain a valid nonce"),
        bundle_id: m.bundle_id.as_deref().map(dec::<BundleId>).transpose()?,
        transition_type: m.transition_type.map(TransitionType::with),
        value: dec::<RevealedData>(&m.value_blob)?,
    })
}

fn ord_columns(ord: WitnessOrd) -> OrdColumns {
    match ord {
        WitnessOrd::Mined(pos) => (
            ORD_MINED,
            Some(pos.height().get()),
            Some(pos.timestamp()),
            Some(match pos.layer1() {
                Layer1::Bitcoin => 0,
                Layer1::Liquid => 1,
            }),
        ),
        WitnessOrd::Tentative => (ORD_TENTATIVE, None, None, None),
        WitnessOrd::Ignored => (ORD_IGNORED, None, None, None),
        WitnessOrd::Archived => (ORD_ARCHIVED, None, None, None),
    }
}

fn ord_from_row(m: &DbWitnessOrd) -> Result<WitnessOrd, Error> {
    let corrupted = |what: &str| Error::Internal {
        details: format!("corrupted RGB data: {what}"),
    };
    match m.status {
        ORD_MINED => {
            let height = m
                .height
                .and_then(std::num::NonZeroU32::new)
                .ok_or_else(|| corrupted("mined WitnessOrd without a valid height"))?;
            let timestamp = m
                .timestamp
                .ok_or_else(|| corrupted("mined WitnessOrd without a timestamp"))?;
            let layer1 = match m.layer1 {
                Some(0) => Layer1::Bitcoin,
                Some(1) => Layer1::Liquid,
                _ => return Err(corrupted("mined WitnessOrd without a valid layer1")),
            };
            WitnessPos::with(layer1, height, timestamp)
                .map(WitnessOrd::Mined)
                .ok_or_else(|| corrupted("mined WitnessOrd with an invalid position"))
        }
        ORD_TENTATIVE => Ok(WitnessOrd::Tentative),
        ORD_IGNORED => Ok(WitnessOrd::Ignored),
        ORD_ARCHIVED => Ok(WitnessOrd::Archived),
        other => Err(corrupted(&format!("unknown WitnessOrd status {other}"))),
    }
}

fn outpoint_keys(outpoints: &BTreeSet<OutPoint>) -> Result<Vec<(Vec<u8>, u32)>, Error> {
    outpoints
        .iter()
        .map(|o| Ok((enc(&o.txid)?, o.vout)))
        .collect()
}

// match the wanted outpoints as `(txid = ? AND vout = ?) OR ...`, so that assignments sharing a
// TXID on other vouts are not returned by the query at all
fn outpoint_condition(
    txid_col: impl ColumnTrait,
    vout_col: impl ColumnTrait,
    keys: &[(Vec<u8>, u32)],
) -> Condition {
    keys.iter().fold(Condition::any(), |cond, (txid, vout)| {
        cond.add(
            Condition::all()
                .add(txid_col.eq(txid.clone()))
                .add(vout_col.eq(*vout)),
        )
    })
}

// private helpers for the `RgbStore` implementation below
impl DbTxn {
    fn get_genesis(&self, contract_id: &[u8]) -> Result<Option<DbGenesis>, Error> {
        Ok(block_on(
            RgbGenesis::find()
                .filter(rgb_genesis::Column::ContractId.eq(contract_id.to_vec()))
                .one(self.inner()),
        )?)
    }

    fn get_spend_edges_of(&self, opid: &[u8]) -> Result<Vec<DbOpInput>, Error> {
        Ok(block_on(
            RgbOpInput::find()
                .filter(rgb_op_input::Column::Opid.eq(opid.to_vec()))
                .all(self.inner()),
        )?)
    }

    fn get_schema(&self, schema_id: &[u8]) -> Result<Option<DbSchema>, Error> {
        Ok(block_on(
            RgbSchema::find()
                .filter(rgb_schema::Column::SchemaId.eq(schema_id.to_vec()))
                .one(self.inner()),
        )?)
    }

    fn get_witness(&self, txid: &[u8]) -> Result<Option<DbWitness>, Error> {
        Ok(block_on(
            RgbWitness::find()
                .filter(rgb_witness::Column::Txid.eq(txid.to_vec()))
                .one(self.inner()),
        )?)
    }

    fn assignments_at(
        &self,
        contract_id: Option<&[u8]>,
        outpoints: &BTreeSet<OutPoint>,
        visibility: Visibility,
    ) -> Result<Vec<DbAssignment>, Error> {
        let wanted = outpoint_keys(outpoints)?;
        let mut explicit = Vec::new();
        let mut on_witness = Vec::new();
        for chunk in wanted.chunks(OUTPOINT_CHUNK) {
            let mut query = RgbAssignment::find().filter(outpoint_condition(
                rgb_assignment::Column::SealTxid,
                rgb_assignment::Column::SealVout,
                chunk,
            ));
            if let Some(contract_id) = contract_id {
                query = query.filter(rgb_assignment::Column::ContractId.eq(contract_id.to_vec()));
            }
            explicit.extend(block_on(query.all(self.inner()))?);

            let mut query = RgbAssignment::find()
                .select_only()
                .columns(
                    rgb_assignment::Column::iter()
                        .filter(|c| !matches!(c, rgb_assignment::Column::SealTxid)),
                )
                .column_as(rgb_bundle_witness::Column::Txid, "seal_txid")
                .join(
                    JoinType::InnerJoin,
                    rgb_assignment::Entity::belongs_to(rgb_bundle_witness::Entity)
                        .from(rgb_assignment::Column::BundleId)
                        .to(rgb_bundle_witness::Column::BundleId)
                        .into(),
                )
                .filter(rgb_assignment::Column::SealTxid.is_null())
                .filter(outpoint_condition(
                    rgb_bundle_witness::Column::Txid,
                    rgb_assignment::Column::SealVout,
                    chunk,
                ));
            if let Some(contract_id) = contract_id {
                query = query.filter(rgb_assignment::Column::ContractId.eq(contract_id.to_vec()));
            }
            if visibility == Visibility::Valid {
                query = query
                    .join(
                        JoinType::InnerJoin,
                        rgb_bundle_witness::Entity::belongs_to(rgb_witness_ord::Entity)
                            .from(rgb_bundle_witness::Column::Txid)
                            .to(rgb_witness_ord::Column::Txid)
                            .into(),
                    )
                    .filter(rgb_witness_ord::Column::Status.ne(ORD_ARCHIVED));
            }
            on_witness.extend(block_on(query.all(self.inner()))?);
        }
        let mut out = self.visible_assignments(explicit, visibility)?;
        out.extend(self.assignments_of_valid_ops(on_witness, visibility)?);
        Ok(out)
    }

    /// The bundles, among the provided ones, having at least one witness that is not archived.
    ///
    /// Joins to `rgb_witness_ord` so that the archived witnesses are dropped by the query: only
    /// the surviving bundle ids are returned, so no witness is ever decoded.
    fn valid_bundles(&self, bundle_ids: &BTreeSet<BundleId>) -> Result<BTreeSet<BundleId>, Error> {
        let keys: Vec<Vec<u8>> = bundle_ids.iter().map(enc).collect::<Result<_, Error>>()?;
        let mut out = BTreeSet::new();
        for chunk in keys.chunks(QUERY_CHUNK) {
            for bundle_id in block_on(
                RgbBundleWitness::find()
                    .join(
                        JoinType::InnerJoin,
                        rgb_bundle_witness::Entity::belongs_to(rgb_witness_ord::Entity)
                            .from(rgb_bundle_witness::Column::Txid)
                            .to(rgb_witness_ord::Column::Txid)
                            .into(),
                    )
                    .filter(rgb_bundle_witness::Column::BundleId.is_in(chunk.to_vec()))
                    .filter(rgb_witness_ord::Column::Status.ne(ORD_ARCHIVED))
                    .select_only()
                    .column(rgb_bundle_witness::Column::BundleId)
                    .distinct()
                    .into_tuple::<Vec<u8>>()
                    .all(self.inner()),
            )? {
                out.insert(dec::<BundleId>(&bundle_id)?);
            }
        }
        Ok(out)
    }

    fn invalid_ops_among(&self, opids: &BTreeSet<OpId>) -> Result<BTreeSet<OpId>, Error> {
        let keys: Vec<Vec<u8>> = opids.iter().map(enc).collect::<Result<_, Error>>()?;
        let mut out = BTreeSet::new();
        for chunk in keys.chunks(QUERY_CHUNK) {
            for row in block_on(
                RgbInvalidOp::find()
                    .filter(rgb_invalid_op::Column::Opid.is_in(chunk.to_vec()))
                    .all(self.inner()),
            )? {
                out.insert(dec::<OpId>(&row.opid)?);
            }
        }
        Ok(out)
    }

    fn visible_assignments(
        &self,
        rows: Vec<DbAssignment>,
        visibility: Visibility,
    ) -> Result<Vec<DbAssignment>, Error> {
        if visibility == Visibility::All {
            return Ok(rows);
        }
        let mut bundles = BTreeSet::new();
        let mut opids = BTreeSet::new();
        for m in &rows {
            if let Some(b) = &m.bundle_id {
                bundles.insert(dec::<BundleId>(b)?);
            }
            opids.insert(dec::<OpId>(&m.opid)?);
        }
        let valid_bundles = self.valid_bundles(&bundles)?;
        let invalid_ops = self.invalid_ops_among(&opids)?;
        let mut out = Vec::with_capacity(rows.len());
        for m in rows {
            let bundle_ok = match &m.bundle_id {
                None => true,
                Some(b) => valid_bundles.contains(&dec::<BundleId>(b)?),
            };
            if bundle_ok && !invalid_ops.contains(&dec::<OpId>(&m.opid)?) {
                out.push(m);
            }
        }
        Ok(out)
    }

    /// [`Self::visible_assignments`] for the rows whose witness has already been checked by the
    /// query returning them, leaving only their operations to be.
    fn assignments_of_valid_ops(
        &self,
        rows: Vec<DbAssignment>,
        visibility: Visibility,
    ) -> Result<Vec<DbAssignment>, Error> {
        if visibility == Visibility::All {
            return Ok(rows);
        }
        let opids = rows
            .iter()
            .map(|m| dec::<OpId>(&m.opid))
            .collect::<Result<BTreeSet<_>, Error>>()?;
        let invalid_ops = self.invalid_ops_among(&opids)?;
        let mut out = Vec::with_capacity(rows.len());
        for m in rows {
            if !invalid_ops.contains(&dec::<OpId>(&m.opid)?) {
                out.push(m);
            }
        }
        Ok(out)
    }
}

impl RgbStore for &DbTxn {
    type Error = Error;

    // ----- transaction control ------------------------------------------------
    // transaction control is left to the host rgb-lib transaction (`DbTxn`), which commits the
    // whole unit of work atomically; RgbStore's `begin` just claims to have opened a transaction,
    // and `commit`/`rollback` are no-ops

    fn begin(&self, _mode: TxMode) -> Result<TxBegin, Error> {
        Ok(TxBegin::Opened)
    }
    fn commit(&self) -> Result<(), Error> {
        Ok(())
    }
    fn rollback(&self) {}

    // ----- schema definitions --------------------------------------------------

    fn schema(&self, schema_id: SchemaId) -> Result<Option<Schema>, Error> {
        match self.get_schema(&enc(&schema_id)?)? {
            Some(row) => Ok(Some(dec(&row.schema_blob)?)),
            None => Ok(None),
        }
    }

    fn put_schema(&mut self, schema: &Schema) -> Result<(), Error> {
        block_on(
            RgbSchema::insert(DbSchemaActMod {
                schema_id: ActiveValue::Set(enc(&schema.schema_id())?),
                schema_blob: ActiveValue::Set(enc(schema)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_schema::Column::SchemaId)
                    .do_nothing()
                    .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn schemata(&self) -> impl Iterator<Item = Result<Schema, Error>> + '_ {
        collect_decoded(
            block_on(RgbSchema::find().all(self.inner())).map_err(Error::from),
            |m| dec(&m.schema_blob),
        )
    }

    // ----- type libraries / AluVM libraries -----------------------------------

    fn type_libs(&self, schema_id: SchemaId) -> Result<BTreeMap<TypeLibId, TypeLib>, Error> {
        let mut libs = BTreeMap::new();
        let schema_id = enc(&schema_id)?;
        let type_lib_ids: Vec<Vec<u8>> = block_on(
            RgbSchemaTypeLib::find()
                .filter(rgb_schema_type_lib::Column::SchemaId.eq(schema_id))
                .all(self.inner()),
        )?
        .into_iter()
        .map(|l| l.type_lib_id)
        .collect();
        if !type_lib_ids.is_empty() {
            for row in block_on(
                RgbTypeLib::find()
                    .filter(rgb_type_lib::Column::TypeLibId.is_in(type_lib_ids))
                    .all(self.inner()),
            )? {
                libs.insert(
                    dec::<TypeLibId>(&row.type_lib_id)?,
                    dec::<TypeLib>(&row.type_lib_blob)?,
                );
            }
        }
        Ok(libs)
    }

    fn put_type_lib(&mut self, schema_id: SchemaId, type_lib: &TypeLib) -> Result<(), Error> {
        let type_lib_id = enc(&type_lib.id())?;
        block_on(
            RgbTypeLib::insert(DbTypeLibActMod {
                type_lib_id: ActiveValue::Set(type_lib_id.clone()),
                type_lib_blob: ActiveValue::Set(enc(type_lib)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_type_lib::Column::TypeLibId)
                    .do_nothing()
                    .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        block_on(
            RgbSchemaTypeLib::insert(DbSchemaTypeLibActMod {
                schema_id: ActiveValue::Set(enc(&schema_id)?),
                type_lib_id: ActiveValue::Set(type_lib_id),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::columns([
                    rgb_schema_type_lib::Column::SchemaId,
                    rgb_schema_type_lib::Column::TypeLibId,
                ])
                .do_nothing()
                .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn aluvm_lib(&self, aluvm_lib_id: LibId) -> Result<Option<Lib>, Error> {
        match block_on(
            RgbAluvmLib::find()
                .filter(rgb_aluvm_lib::Column::AluvmLibId.eq(enc(&aluvm_lib_id)?))
                .one(self.inner()),
        )? {
            Some(row) => Ok(Some(dec(&row.aluvm_lib_blob)?)),
            None => Ok(None),
        }
    }

    fn put_aluvm_lib(&mut self, aluvm_lib: &Lib) -> Result<(), Error> {
        block_on(
            RgbAluvmLib::insert(DbAluvmLibActMod {
                aluvm_lib_id: ActiveValue::Set(enc(&aluvm_lib.id())?),
                aluvm_lib_blob: ActiveValue::Set(enc(aluvm_lib)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_aluvm_lib::Column::AluvmLibId)
                    .do_nothing()
                    .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    // ----- contracts (genesis) ------------------------------------------------

    fn genesis(&self, contract_id: ContractId) -> Result<Option<Genesis>, Error> {
        match self.get_genesis(&enc(&contract_id)?)? {
            Some(row) => Ok(Some(dec(&row.genesis_blob)?)),
            None => Ok(None),
        }
    }

    fn put_genesis(&mut self, genesis: &Genesis) -> Result<(), Error> {
        block_on(
            RgbGenesis::insert(DbGenesisActMod {
                contract_id: ActiveValue::Set(enc(&genesis.contract_id())?),
                schema_id: ActiveValue::Set(enc(&genesis.schema_id)?),
                genesis_blob: ActiveValue::Set(enc(genesis)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_genesis::Column::ContractId)
                    .update_columns([
                        rgb_genesis::Column::SchemaId,
                        rgb_genesis::Column::GenesisBlob,
                    ])
                    .to_owned(),
            )
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn geneses(&self) -> impl Iterator<Item = Result<Genesis, Error>> + '_ {
        collect_decoded(
            block_on(RgbGenesis::find().all(self.inner())).map_err(Error::from),
            |m| dec(&m.genesis_blob),
        )
    }

    fn contract_schema(&self, contract_id: ContractId) -> Result<Option<Schema>, Error> {
        let Some(genesis) = self.get_genesis(&enc(&contract_id)?)? else {
            return Ok(None);
        };
        match self.get_schema(&genesis.schema_id)? {
            Some(row) => Ok(Some(dec(&row.schema_blob)?)),
            None => Ok(None),
        }
    }

    // ----- bundles / witnesses / secret seals ---------------------------------

    fn bundle(&self, bundle_id: BundleId) -> Result<Option<TransitionBundle>, Error> {
        match block_on(
            RgbBundle::find()
                .filter(rgb_bundle::Column::BundleId.eq(enc(&bundle_id)?))
                .one(self.inner()),
        )? {
            Some(row) => Ok(Some(dec(&row.bundle_blob)?)),
            None => Ok(None),
        }
    }

    fn put_bundle(&mut self, bundle: &TransitionBundle) -> Result<(), Error> {
        block_on(
            RgbBundle::insert(DbBundleActMod {
                bundle_id: ActiveValue::Set(enc(&bundle.bundle_id())?),
                bundle_blob: ActiveValue::Set(enc(bundle)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_bundle::Column::BundleId)
                    .update_column(rgb_bundle::Column::BundleBlob)
                    .to_owned(),
            )
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn witness(&self, txid: RgbTxid) -> Result<Option<SealWitness>, Error> {
        match self.get_witness(&enc(&txid)?)? {
            Some(row) => Ok(Some(SealWitness {
                tx: dec(&row.tx_blob)?,
                mpc_merkle_block: dec(&row.mpc_merkle_block_blob)?,
                dbc_proof: dec(&row.dbc_proof_blob)?,
                spv_proof: row.spv_proof_blob.as_deref().map(dec).transpose()?,
            })),
            None => Ok(None),
        }
    }

    fn put_witness(&mut self, witness: &SealWitness) -> Result<(), Error> {
        block_on(
            RgbWitness::insert(DbWitnessActMod {
                txid: ActiveValue::Set(enc(&witness.witness_id())?),
                tx_blob: ActiveValue::Set(enc(&witness.tx)?),
                mpc_merkle_block_blob: ActiveValue::Set(enc(&witness.mpc_merkle_block)?),
                dbc_proof_blob: ActiveValue::Set(enc(&witness.dbc_proof)?),
                spv_proof_blob: ActiveValue::Set(witness.spv_proof.as_ref().map(enc).transpose()?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_witness::Column::Txid)
                    .update_columns([
                        rgb_witness::Column::TxBlob,
                        rgb_witness::Column::MpcMerkleBlockBlob,
                        rgb_witness::Column::DbcProofBlob,
                        rgb_witness::Column::SpvProofBlob,
                    ])
                    .to_owned(),
            )
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn witness_tx(&self, txid: RgbTxid) -> Result<Option<bdk_wallet::bitcoin::Transaction>, Error> {
        match self.get_witness(&enc(&txid)?)? {
            Some(row) => Ok(Some(dec(&row.tx_blob)?)),
            None => Ok(None),
        }
    }

    fn witness_spv_proof(&self, txid: RgbTxid) -> Result<Option<SpvProof>, Error> {
        match self.get_witness(&enc(&txid)?)? {
            Some(row) => row.spv_proof_blob.as_deref().map(dec).transpose(),
            None => Ok(None),
        }
    }

    fn set_witness_spv_proof(
        &mut self,
        txid: RgbTxid,
        spv_proof: Option<&SpvProof>,
    ) -> Result<bool, Error> {
        let txid = enc(&txid)?;
        let Some(row) = self.get_witness(&txid)? else {
            return Ok(false);
        };
        let spv_proof: Option<Vec<u8>> = spv_proof.map(enc).transpose()?;
        if row.spv_proof_blob == spv_proof {
            return Ok(false);
        }
        Ok(block_on(
            RgbWitness::update_many()
                .col_expr(rgb_witness::Column::SpvProofBlob, Expr::value(spv_proof))
                .filter(rgb_witness::Column::Txid.eq(txid))
                .exec(self.inner()),
        )?
        .rows_affected
            > 0)
    }

    fn seal_of_secret(&self, secret_seal: SecretSeal) -> Result<Option<GraphSeal>, Error> {
        match block_on(
            RgbSecretSeal::find()
                .filter(rgb_secret_seal::Column::SecretSeal.eq(enc(&secret_seal)?))
                .one(self.inner()),
        )? {
            Some(row) => Ok(Some(dec(&row.graph_seal_blob)?)),
            None => Ok(None),
        }
    }

    fn put_secret_seal(
        &mut self,
        graph_seal: &GraphSeal,
        secret_seal: SecretSeal,
    ) -> Result<(), Error> {
        block_on(
            RgbSecretSeal::insert(DbSecretSealActMod {
                secret_seal: ActiveValue::Set(enc(&secret_seal)?),
                graph_seal_blob: ActiveValue::Set(enc(graph_seal)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_secret_seal::Column::SecretSeal)
                    .do_nothing()
                    .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    // ----- global state -------------------------------------------------------

    fn globals(
        &self,
        contract_id: ContractId,
        type_id: GlobalStateType,
        visibility: Visibility,
    ) -> Result<Vec<GlobalStateRow>, Error> {
        let rows = block_on(
            RgbGlobalState::find()
                .filter(rgb_global_state::Column::ContractId.eq(enc(&contract_id)?))
                .filter(rgb_global_state::Column::TypeId.eq(type_id.into_inner()))
                .all(self.inner()),
        )?;
        let valid_bundles = match visibility {
            Visibility::All => None,
            Visibility::Valid => {
                let mut bundles = BTreeSet::new();
                for m in &rows {
                    if let Some(b) = &m.bundle_id {
                        bundles.insert(dec::<BundleId>(b)?);
                    }
                }
                Some(self.valid_bundles(&bundles)?)
            }
        };
        let mut out = Vec::with_capacity(rows.len());
        for m in &rows {
            let row = global_state_to_row(m)?;
            if let Some(valid) = &valid_bundles
                && let Some(bundle_id) = row.bundle_id
                && !valid.contains(&bundle_id)
            {
                continue;
            }
            out.push(row);
        }
        Ok(out)
    }

    fn put_global(&mut self, global_state: GlobalStateWrite<'_>) -> Result<(), Error> {
        block_on(
            RgbGlobalState::insert(DbGlobalStateActMod {
                contract_id: ActiveValue::Set(enc(&global_state.contract_id)?),
                type_id: ActiveValue::Set(global_state.type_id.into_inner()),
                opid: ActiveValue::Set(enc(&global_state.opid)?),
                out_index: ActiveValue::Set(global_state.index),
                nonce: ActiveValue::Set(global_state.nonce.to_string()),
                bundle_id: ActiveValue::Set(global_state.bundle_id.map(|b| enc(&b)).transpose()?),
                transition_type: ActiveValue::Set(
                    global_state.transition_type.map(|t| t.into_inner()),
                ),
                value_blob: ActiveValue::Set(enc(global_state.value)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::columns([
                    rgb_global_state::Column::ContractId,
                    rgb_global_state::Column::TypeId,
                    rgb_global_state::Column::Opid,
                    rgb_global_state::Column::OutIndex,
                ])
                .do_nothing()
                .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    // ----- allocations (owned state) ------------------------------------------

    fn allocations(
        &self,
        contract_id: ContractId,
        filter: AllocationFilter<'_>,
    ) -> Result<Vec<AllocationRow>, Error> {
        let cid = enc(&contract_id)?;
        let rows = match filter.outpoints {
            None => {
                let mut query = RgbAssignment::find()
                    .filter(rgb_assignment::Column::ContractId.eq(cid.clone()));
                if let Some(state_type) = filter.kind.map(alloc_kind_str) {
                    query = query.filter(rgb_assignment::Column::StateType.eq(state_type));
                }
                if let Some(type_id) = filter.type_id {
                    query = query.filter(rgb_assignment::Column::TypeId.eq(type_id.into_inner()));
                }
                let rows = block_on(query.all(self.inner()))?;
                self.visible_assignments(rows, filter.visibility)?
            }
            Some(outpoints) if outpoints.is_empty() => return Ok(vec![]),
            Some(outpoints) => self
                .assignments_at(Some(&cid), outpoints, filter.visibility)?
                .into_iter()
                .filter(|m| {
                    filter
                        .kind
                        .is_none_or(|k| m.state_type == alloc_kind_str(k))
                        && filter.type_id.is_none_or(|t| m.type_id == t.into_inner())
                })
                .collect(),
        };
        rows.iter().map(assignment_to_row).collect()
    }

    fn all_allocations_at_outpoints(
        &self,
        outpoints: &BTreeSet<OutPoint>,
        visibility: Visibility,
    ) -> Result<Vec<(ContractId, AllocationRow)>, Error> {
        if outpoints.is_empty() {
            return Ok(vec![]);
        }
        self.assignments_at(None, outpoints, visibility)?
            .iter()
            .map(|m| Ok((dec::<ContractId>(&m.contract_id)?, assignment_to_row(m)?)))
            .collect()
    }

    fn put_allocation(&mut self, allocation: AllocationWrite<'_>) -> Result<(), Error> {
        block_on(
            RgbAssignment::insert(DbAssignmentActMod {
                contract_id: ActiveValue::Set(enc(&allocation.contract_id)?),
                state_type: ActiveValue::Set(alloc_kind_str(allocation.kind).to_string()),
                type_id: ActiveValue::Set(allocation.opout.ty.into_inner()),
                opid: ActiveValue::Set(enc(&allocation.opout.op)?),
                output_no: ActiveValue::Set(allocation.opout.no),
                seal_txid: ActiveValue::Set(allocation.seal.txid.as_ref().map(enc).transpose()?),
                seal_vout: ActiveValue::Set(allocation.seal.vout.to_u32()),
                bundle_id: ActiveValue::Set(allocation.bundle_id.map(|b| enc(&b)).transpose()?),
                value_blob: ActiveValue::Set(allocation.value.to_vec()),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::columns([
                    rgb_assignment::Column::ContractId,
                    rgb_assignment::Column::Opid,
                    rgb_assignment::Column::TypeId,
                    rgb_assignment::Column::OutputNo,
                ])
                .do_nothing()
                .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    // ----- WitnessOrd / invalid ops -------------------------------------------

    fn witness_ord(&self, txid: RgbTxid) -> Result<Option<WitnessOrd>, Error> {
        Ok(self.witness_ords(&BTreeSet::from([txid]))?.remove(&txid))
    }

    fn witness_ords(
        &self,
        txids: &BTreeSet<RgbTxid>,
    ) -> Result<BTreeMap<RgbTxid, WitnessOrd>, Error> {
        let keys: Vec<Vec<u8>> = txids.iter().map(enc).collect::<Result<_, Error>>()?;
        let mut ords = BTreeMap::new();
        for chunk in keys.chunks(QUERY_CHUNK) {
            for row in block_on(
                RgbWitnessOrd::find()
                    .filter(rgb_witness_ord::Column::Txid.is_in(chunk.to_vec()))
                    .all(self.inner()),
            )? {
                ords.insert(dec::<RgbTxid>(&row.txid)?, ord_from_row(&row)?);
            }
        }
        Ok(ords)
    }

    fn witness_ords_to_refresh(
        &self,
        min_height: std::num::NonZeroU32,
    ) -> Result<BTreeSet<RgbTxid>, Error> {
        let mut out = BTreeSet::new();
        for row in block_on(
            RgbWitnessOrd::find()
                .filter(
                    Condition::any()
                        .add(
                            Condition::all()
                                .add(rgb_witness_ord::Column::Status.eq(ORD_MINED))
                                .add(rgb_witness_ord::Column::Height.gte(min_height.get())),
                        )
                        .add(
                            rgb_witness_ord::Column::Status
                                .is_in(vec![ORD_TENTATIVE, ORD_ARCHIVED]),
                        ),
                )
                .all(self.inner()),
        )? {
            out.insert(dec::<RgbTxid>(&row.txid)?);
        }
        Ok(out)
    }

    fn all_witness_ords(&self) -> Result<BTreeMap<RgbTxid, WitnessOrd>, Error> {
        let mut ords = BTreeMap::new();
        for row in block_on(RgbWitnessOrd::find().all(self.inner()))? {
            ords.insert(dec::<RgbTxid>(&row.txid)?, ord_from_row(&row)?);
        }
        Ok(ords)
    }

    fn put_witness_ord(&mut self, txid: RgbTxid, ord: WitnessOrd) -> Result<(), Error> {
        let (status, height, timestamp, layer1) = ord_columns(ord);
        block_on(
            RgbWitnessOrd::insert(DbWitnessOrdActMod {
                txid: ActiveValue::Set(enc(&txid)?),
                status: ActiveValue::Set(status),
                height: ActiveValue::Set(height),
                timestamp: ActiveValue::Set(timestamp),
                layer1: ActiveValue::Set(layer1),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_witness_ord::Column::Txid)
                    .update_columns([
                        rgb_witness_ord::Column::Status,
                        rgb_witness_ord::Column::Height,
                        rgb_witness_ord::Column::Timestamp,
                        rgb_witness_ord::Column::Layer1,
                    ])
                    .to_owned(),
            )
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn all_invalid_ops(&self) -> Result<BTreeSet<OpId>, Error> {
        let mut ids = BTreeSet::new();
        for row in block_on(RgbInvalidOp::find().all(self.inner()))? {
            ids.insert(dec::<OpId>(&row.opid)?);
        }
        Ok(ids)
    }

    fn any_op_invalid(&self, opids: &BTreeSet<OpId>) -> Result<bool, Error> {
        Ok(!self.invalid_ops_among(opids)?.is_empty())
    }

    fn set_op_validity(&mut self, opid: OpId, valid: bool) -> Result<(), Error> {
        let key = enc(&opid)?;
        if valid {
            block_on(
                RgbInvalidOp::delete_many()
                    .filter(rgb_invalid_op::Column::Opid.eq(key))
                    .exec(self.inner()),
            )?;
            Ok(())
        } else {
            block_on(
                RgbInvalidOp::insert(DbInvalidOpActMod {
                    opid: ActiveValue::Set(key),
                    ..Default::default()
                })
                .on_conflict(
                    OnConflict::column(rgb_invalid_op::Column::Opid)
                        .do_nothing()
                        .to_owned(),
                )
                .try_insert()
                .exec(self.inner()),
            )?;
            Ok(())
        }
    }

    // ----- index: contract / bundle / op graph --------------------------------

    fn contract_registered(&self, contract_id: ContractId) -> Result<bool, Error> {
        Ok(self.get_genesis(&enc(&contract_id)?)?.is_some())
    }

    fn bundle_contract(&self, bundle_id: BundleId) -> Result<Option<ContractId>, Error> {
        match block_on(
            RgbBundleContract::find()
                .filter(rgb_bundle_contract::Column::BundleId.eq(enc(&bundle_id)?))
                .one(self.inner()),
        )? {
            Some(row) => Ok(Some(dec(&row.contract_id)?)),
            None => Ok(None),
        }
    }

    fn put_bundle_contract(
        &mut self,
        bundle_id: BundleId,
        contract_id: ContractId,
    ) -> Result<(), Error> {
        block_on(
            RgbBundleContract::insert(DbBundleContractActMod {
                bundle_id: ActiveValue::Set(enc(&bundle_id)?),
                contract_id: ActiveValue::Set(enc(&contract_id)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_bundle_contract::Column::BundleId)
                    .update_column(rgb_bundle_contract::Column::ContractId)
                    .to_owned(),
            )
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn bundle_witnesses(&self, bundle_id: BundleId) -> Result<BTreeSet<RgbTxid>, Error> {
        Ok(self
            .witnesses_of_bundles(&BTreeSet::from([bundle_id]))?
            .remove(&bundle_id)
            .unwrap_or_default())
    }

    fn witnesses_of_bundles(
        &self,
        bundle_ids: &BTreeSet<BundleId>,
    ) -> Result<BTreeMap<BundleId, BTreeSet<RgbTxid>>, Error> {
        let keys: Vec<Vec<u8>> = bundle_ids.iter().map(enc).collect::<Result<_, Error>>()?;
        let mut out: BTreeMap<BundleId, BTreeSet<RgbTxid>> = BTreeMap::new();
        for chunk in keys.chunks(QUERY_CHUNK) {
            for row in block_on(
                RgbBundleWitness::find()
                    .filter(rgb_bundle_witness::Column::BundleId.is_in(chunk.to_vec()))
                    .all(self.inner()),
            )? {
                out.entry(dec::<BundleId>(&row.bundle_id)?)
                    .or_default()
                    .insert(dec::<RgbTxid>(&row.txid)?);
            }
        }
        Ok(out)
    }

    fn bundles_of_witness(&self, txid: RgbTxid) -> Result<BTreeSet<BundleId>, Error> {
        let mut out = BTreeSet::new();
        for row in block_on(
            RgbBundleWitness::find()
                .filter(rgb_bundle_witness::Column::Txid.eq(enc(&txid)?))
                .all(self.inner()),
        )? {
            out.insert(dec::<BundleId>(&row.bundle_id)?);
        }
        Ok(out)
    }

    fn put_bundle_witness(&mut self, bundle_id: BundleId, txid: RgbTxid) -> Result<(), Error> {
        block_on(
            RgbBundleWitness::insert(DbBundleWitnessActMod {
                bundle_id: ActiveValue::Set(enc(&bundle_id)?),
                txid: ActiveValue::Set(enc(&txid)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::columns([
                    rgb_bundle_witness::Column::BundleId,
                    rgb_bundle_witness::Column::Txid,
                ])
                .do_nothing()
                .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn bundle_of_op(&self, opid: OpId) -> Result<Option<BundleId>, Error> {
        match block_on(
            RgbOpBundle::find()
                .filter(rgb_op_bundle::Column::Opid.eq(enc(&opid)?))
                .one(self.inner()),
        )? {
            Some(row) => Ok(Some(dec(&row.bundle_id)?)),
            None => Ok(None),
        }
    }

    fn ops_in_bundle(&self, bundle_id: BundleId) -> Result<BTreeSet<OpId>, Error> {
        let mut out = BTreeSet::new();
        for row in block_on(
            RgbOpBundle::find()
                .filter(rgb_op_bundle::Column::BundleId.eq(enc(&bundle_id)?))
                .all(self.inner()),
        )? {
            out.insert(dec::<OpId>(&row.opid)?);
        }
        Ok(out)
    }

    fn put_op_bundle(&mut self, opid: OpId, bundle_id: BundleId) -> Result<(), Error> {
        block_on(
            RgbOpBundle::insert(DbOpBundleActMod {
                opid: ActiveValue::Set(enc(&opid)?),
                bundle_id: ActiveValue::Set(enc(&bundle_id)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(rgb_op_bundle::Column::Opid)
                    .do_nothing()
                    .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn child_bundles_of_op(&self, opid: OpId) -> Result<BTreeSet<BundleId>, Error> {
        let mut out = BTreeSet::new();
        for row in self.get_spend_edges_of(&enc(&opid)?)? {
            out.insert(dec::<BundleId>(&row.child_bundle_id)?);
        }
        Ok(out)
    }

    fn child_ops_of_op(&self, opid: OpId) -> Result<BTreeSet<(OpId, BundleId)>, Error> {
        let mut out = BTreeSet::new();
        for row in self.get_spend_edges_of(&enc(&opid)?)? {
            out.insert((
                dec::<OpId>(&row.child_opid)?,
                dec::<BundleId>(&row.child_bundle_id)?,
            ));
        }
        Ok(out)
    }

    fn input_opouts_for_op(&self, opid: OpId) -> Result<BTreeSet<Opout>, Error> {
        let mut out = BTreeSet::new();
        for row in block_on(
            RgbOpInput::find()
                .filter(rgb_op_input::Column::ChildOpid.eq(enc(&opid)?))
                .all(self.inner()),
        )? {
            let op = dec::<OpId>(&row.opid)?;
            out.insert(Opout::new(
                op,
                AssignmentType::with(row.type_id),
                row.output_no,
            ));
        }
        Ok(out)
    }

    fn put_op_input(
        &mut self,
        spent_opout: Opout,
        child_opid: OpId,
        child_bundle_id: BundleId,
    ) -> Result<(), Error> {
        block_on(
            RgbOpInput::insert(DbOpInputActMod {
                opid: ActiveValue::Set(enc(&spent_opout.op)?),
                type_id: ActiveValue::Set(spent_opout.ty.into_inner()),
                output_no: ActiveValue::Set(spent_opout.no),
                child_opid: ActiveValue::Set(enc(&child_opid)?),
                child_bundle_id: ActiveValue::Set(enc(&child_bundle_id)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::columns([
                    rgb_op_input::Column::Opid,
                    rgb_op_input::Column::TypeId,
                    rgb_op_input::Column::OutputNo,
                    rgb_op_input::Column::ChildOpid,
                ])
                .do_nothing()
                .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    // ----- index: outpoint->opout and secret-seal->opout ------------------------

    fn opouts_at(
        &self,
        contract_id: ContractId,
        outpoints: &BTreeSet<OutPoint>,
    ) -> Result<BTreeMap<OutPoint, BTreeSet<Opout>>, Error> {
        let mut out = BTreeMap::<OutPoint, BTreeSet<Opout>>::new();
        if outpoints.is_empty() {
            return Ok(out);
        }
        let wanted = outpoint_keys(outpoints)?;
        let cid = enc(&contract_id)?;
        for chunk in wanted.chunks(OUTPOINT_CHUNK) {
            for row in block_on(
                RgbOutpointOpout::find()
                    .filter(rgb_outpoint_opout::Column::ContractId.eq(cid.clone()))
                    .filter(outpoint_condition(
                        rgb_outpoint_opout::Column::Txid,
                        rgb_outpoint_opout::Column::Vout,
                        chunk,
                    ))
                    .all(self.inner()),
            )? {
                let outpoint = OutPoint::new(dec::<RgbTxid>(&row.txid)?, row.vout);
                out.entry(outpoint)
                    .or_default()
                    .insert(dec::<Opout>(&row.opout)?);
            }
        }
        Ok(out)
    }

    fn contracts_assigning(
        &self,
        outpoints: &BTreeSet<OutPoint>,
    ) -> Result<BTreeSet<ContractId>, Error> {
        let mut out = BTreeSet::new();
        if outpoints.is_empty() {
            return Ok(out);
        }
        let wanted = outpoint_keys(outpoints)?;
        for chunk in wanted.chunks(OUTPOINT_CHUNK) {
            for row in block_on(
                RgbOutpointOpout::find()
                    .filter(outpoint_condition(
                        rgb_outpoint_opout::Column::Txid,
                        rgb_outpoint_opout::Column::Vout,
                        chunk,
                    ))
                    .all(self.inner()),
            )? {
                out.insert(dec::<ContractId>(&row.contract_id)?);
            }
        }
        Ok(out)
    }

    fn put_outpoint_opout(
        &mut self,
        contract_id: ContractId,
        outpoint: OutPoint,
        opout: Opout,
    ) -> Result<(), Error> {
        block_on(
            RgbOutpointOpout::insert(DbOutpointOpoutActMod {
                contract_id: ActiveValue::Set(enc(&contract_id)?),
                txid: ActiveValue::Set(enc(&outpoint.txid)?),
                vout: ActiveValue::Set(outpoint.vout),
                opout: ActiveValue::Set(enc(&opout)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::columns([
                    rgb_outpoint_opout::Column::ContractId,
                    rgb_outpoint_opout::Column::Txid,
                    rgb_outpoint_opout::Column::Vout,
                    rgb_outpoint_opout::Column::Opout,
                ])
                .do_nothing()
                .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }

    fn opouts_by_secrets(
        &self,
        contract_id: ContractId,
        secret_seals: &BTreeSet<SecretSeal>,
    ) -> Result<BTreeSet<Opout>, Error> {
        let mut out = BTreeSet::new();
        if secret_seals.is_empty() {
            return Ok(out);
        }
        let cid = enc(&contract_id)?;
        let keys: Vec<Vec<u8>> = secret_seals.iter().map(enc).collect::<Result<_, Error>>()?;
        for chunk in keys.chunks(QUERY_CHUNK) {
            for row in block_on(
                RgbSecretSealOpout::find()
                    .filter(rgb_secret_seal_opout::Column::ContractId.eq(cid.clone()))
                    .filter(rgb_secret_seal_opout::Column::SecretSeal.is_in(chunk.to_vec()))
                    .all(self.inner()),
            )? {
                out.insert(dec::<Opout>(&row.opout)?);
            }
        }
        Ok(out)
    }

    fn put_secret_opout(
        &mut self,
        contract_id: ContractId,
        secret_seal: SecretSeal,
        opout: Opout,
    ) -> Result<(), Error> {
        block_on(
            RgbSecretSealOpout::insert(DbSecretSealOpoutActMod {
                contract_id: ActiveValue::Set(enc(&contract_id)?),
                secret_seal: ActiveValue::Set(enc(&secret_seal)?),
                opout: ActiveValue::Set(enc(&opout)?),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::columns([
                    rgb_secret_seal_opout::Column::ContractId,
                    rgb_secret_seal_opout::Column::SecretSeal,
                    rgb_secret_seal_opout::Column::Opout,
                ])
                .do_nothing()
                .to_owned(),
            )
            .try_insert()
            .exec(self.inner()),
        )?;
        Ok(())
    }
}

#[cfg(feature = "rgb_fs_store_migration")]
pub(crate) use legacy::migrate_rgb_fs_to_db;

#[cfg(not(feature = "rgb_fs_store_migration"))]
pub(crate) fn migrate_rgb_fs_to_db(
    _wallet_dir: &Path,
    _database: &RgbLibDatabase,
) -> Result<(), Error> {
    Ok(())
}

#[cfg(feature = "rgb_fs_store_migration")]
mod legacy {
    use super::*;

    use rgbstd::containers::{Consignment, legacy::ConsignmentV0};
    use rgbstd::contract::{KnownState, OpWitness, OutputAssignment};
    use rgbstd::persistence::LegacyFsStore;
    use strict_encoding::{DeserializeError, StreamReader, StrictReader};

    use crate::utils::{RGB_RUNTIME_DIR, atomic_write_with, sync_dir, write_fascia};
    use crate::wallet::{ASSETS_DIR, FASCIA_FILE, TRANSFERS_DIR, UNSIGNED_PSBT_FILE};

    /// Prefix of every RGB container file, followed by a 3-byte magic
    const RGB_FILE_PREFIX: &[u8; 4] = b"RGB\0";

    /// Max number of directory entries [`visit_files`] walks in one directory tree.
    ///
    /// A legacy wallet holds one file per consignment, so a genuine one stays orders of magnitude
    /// below this: the cap only bounds the walk of a tree that has been tampered with.
    const MAX_LEGACY_ENTRIES: usize = 100_000;

    /// One-time migration of the legacy filesystem RGB store and of the legacy
    /// consignment files.
    pub(crate) fn migrate_rgb_fs_to_db(
        wallet_dir: &Path,
        database: &RgbLibDatabase,
    ) -> Result<(), Error> {
        let rgb_dir = wallet_dir.join(RGB_RUNTIME_DIR);
        if !rgb_dir.exists() {
            // finish the job of a run that crashed after retiring the legacy store (the rename
            // below, which is what makes the migration complete) but before deleting it. Failing
            // to delete is not fatal here either, for the same reason as below
            let _ = fs::remove_dir_all(wallet_dir.join(RGB_RUNTIME_DELETING_DIR));
            return Ok(());
        }

        let store = LegacyFsStore::new(rgb_dir.clone());
        let load_err = |what: &str, e: DeserializeError| Error::Internal {
            details: format!("cannot load legacy RGB {what}: {e}"),
        };
        let stash = store.load_stash_v0().map_err(|e| load_err("stash", e))?;
        let state = store.load_state_v0().map_err(|e| load_err("state", e))?;
        let index = store.load_index_v0().map_err(|e| load_err("index", e))?;

        let txn = database.begin_transaction()?;
        let mut store: &DbTxn = &txn;

        // schemata, AluVM libraries and the type system are not migrated: the new store keeps the
        // type libraries a schema was defined by (which a legacy stash does not hold), so the
        // supported schema definitions are re-imported by the wallet on setup instead
        for genesis in stash.geneses().values() {
            store.put_genesis(genesis)?;
        }
        for bundle in stash.bundles().values() {
            store.put_bundle(bundle)?;
        }
        for (txid, witness) in stash.witnesses().iter() {
            // a legacy witness may hold only the txid of its transaction, which cannot be resolved
            // here (no network access while opening the wallet)
            let witness = witness.clone().into_v1(None).map_err(|e| Error::Internal {
                details: format!("cannot migrate legacy RGB witness {txid}: {e}"),
            })?;
            store.put_witness(&witness)?;
        }
        for graph_seal in stash.secret_seals().iter() {
            store.put_secret_seal(graph_seal, graph_seal.conceal())?;
        }

        for (txid, ord) in state.witnesses().iter() {
            store.put_witness_ord(*txid, *ord)?;
        }
        for opid in state.invalid_ops().iter() {
            store.set_op_validity(*opid, false)?;
        }
        for (contract_id, mcs) in state.contracts().iter() {
            // global entries are copied out verbatim: the legacy state files each one under a
            // `GlobalOut` carrying its operation, nonce and index within that operation. Only the
            // bundle has to be resolved through the index, since a `GlobalOut` names the witness
            // transaction of its operation instead of the bundle it sits in
            for (ty, global) in mcs.global().iter() {
                for (out, data) in global.known().iter() {
                    let (bundle_id, transition_type) = match out.op_witness {
                        OpWitness::Genesis => (None, None),
                        // an entry's validity is decided through its bundle, so one stored
                        // without a bundle would read as genesis state, which is always valid.
                        // A transition the legacy index cannot place in a bundle means that
                        // index is inconsistent, so stop rather than migrate the entry as if it
                        // came from the genesis
                        OpWitness::Transition(_, transition_type) => {
                            let Some(bundle_id) = index.op_bundle_index().get(&out.opid).copied()
                            else {
                                return Err(Error::Internal {
                                    details: format!(
                                        "legacy RGB index has no bundle for operation {} of \
                                         contract {contract_id}",
                                        out.opid
                                    ),
                                });
                            };
                            (Some(bundle_id), Some(transition_type))
                        }
                    };
                    store.put_global(GlobalStateWrite {
                        contract_id: *contract_id,
                        type_id: *ty,
                        opid: out.opid,
                        index: out.index,
                        nonce: out.nonce,
                        bundle_id,
                        transition_type,
                        value: data,
                    })?;
                }
            }
            for oa in mcs.rights().iter() {
                migrate_assignment(&mut store, *contract_id, AllocKind::Declarative, oa)?;
            }
            for oa in mcs.fungibles().iter() {
                migrate_assignment(&mut store, *contract_id, AllocKind::Fungible, oa)?;
            }
            for oa in mcs.data().iter() {
                migrate_assignment(&mut store, *contract_id, AllocKind::Structured, oa)?;
            }
        }

        for (contract_id, ci) in index.contract_index().iter() {
            for (output_seal, opouts) in ci.outpoint_opouts().iter() {
                let outpoint = OutPoint::new(output_seal.txid, output_seal.vout.to_u32());
                for opout in opouts.iter() {
                    store.put_outpoint_opout(*contract_id, outpoint, *opout)?;
                }
            }
        }
        // the legacy terminal index is not contract-scoped, so the contract is resolved through the
        // opout's operation: for a transition, through its bundle; for a genesis - which has no
        // bundle, and which the legacy index fed terminals from just like a transition - through
        // the genesis itself. An opout that is neither means the legacy index is inconsistent, so
        // stop rather than migrate a wallet that silently cannot match a blinded receive
        let genesis_contracts: HashMap<OpId, ContractId> = stash
            .geneses()
            .iter()
            .map(|(contract_id, genesis)| (genesis.id(), *contract_id))
            .collect();
        for (secret_seal, opouts) in index.terminal_index().iter() {
            for opout in opouts.iter() {
                let contract_id = index
                    .op_bundle_index()
                    .get(&opout.op)
                    .and_then(|bundle_id| index.bundle_contract_index().get(bundle_id))
                    .or_else(|| genesis_contracts.get(&opout.op))
                    .ok_or_else(|| Error::Internal {
                        details: format!(
                            "legacy RGB index has no contract for operation {} of secret seal {secret_seal}",
                            opout.op
                        ),
                    })?;
                store.put_secret_opout(*contract_id, *secret_seal, *opout)?;
            }
        }
        for (opid, bundle_id) in index.op_bundle_index().iter() {
            store.put_op_bundle(*opid, *bundle_id)?;
        }
        for (bundle_id, bundle) in stash.bundles().iter() {
            for KnownTransition { opid, transition } in &bundle.known_transitions {
                for input in &transition.inputs {
                    store.put_op_input(input, *opid, *bundle_id)?;
                }
            }
        }
        for (bundle_id, contract_id) in index.bundle_contract_index().iter() {
            store.put_bundle_contract(*bundle_id, *contract_id)?;
        }
        for (bundle_id, txids) in index.bundle_witness_index().iter() {
            for txid in txids.iter() {
                store.put_bundle_witness(*bundle_id, *txid)?;
            }
        }

        txn.commit()?;

        // the consignment and fascia files saved alongside the legacy store are in the legacy (V0)
        // container format, which the current RGB libraries no longer read
        migrate_legacy_rgb_files(wallet_dir)?;

        // only after a successful commit: delete the legacy store so we never re-run. The delete
        // goes through a rename so that retiring the store is atomic: a crash mid-delete cannot
        // leave a partially-deleted `rgb` dir for the next open to migrate again (which would fail
        // to load it), only a leftover retired dir that the next open sweeps up
        let deleting_dir = wallet_dir.join(RGB_RUNTIME_DELETING_DIR);
        fs::rename(&rgb_dir, &deleting_dir)?;
        // fsync the wallet dir so the rename (our "already migrated" marker) is durable: the DB
        // writes above are upserts, so a re-run after a power loss that kept later DB commits but
        // lost the rename would overwrite newer RGB state with the legacy snapshot
        sync_dir(wallet_dir)?;
        // the migration is complete and durable by now, so a delete that fails must not fail the
        // wallet open: the leftover is never read and stays out of backups, and the next open
        // tries again
        let _ = fs::remove_dir_all(&deleting_dir);

        Ok(())
    }

    /// Convert every legacy (V0) consignment file under the wallet `assets/` and `transfers/` dirs to
    /// the current V1 container format, and every legacy fascia to the current layout and encoding.
    ///
    /// Consignments come in two flavours: plain ones (`CON`/`TFR` magic, strict-encoded
    /// consignment) and validated ones (`VCO`/`VTF` magic, strict-encoded consignment followed by
    /// the JSON validation status). Plain ones (and the issued contracts, saved validated) are
    /// re-saved as plain V1 consignments. The validated copy of a received transfer is dropped: it
    /// was a duplicate of the plain one plus a validation status which cannot be carried over, and a
    /// transfer still in flight is validated again (and its status cached) when next needed.
    ///
    /// Idempotent: files already in the V1 format (or not RGB containers at all) are left untouched.
    fn migrate_legacy_rgb_files(wallet_dir: &Path) -> Result<(), Error> {
        for dir in [ASSETS_DIR, TRANSFERS_DIR] {
            let dir = wallet_dir.join(dir);
            if dir.is_dir() {
                visit_files(&dir, MAX_LEGACY_ENTRIES, &mut |path| {
                    migrate_legacy_rgb_file(path)
                })?;
            }
        }
        Ok(())
    }

    /// Call `f` on every regular file under `dir`, descending into its subdirectories.
    ///
    /// Symlinks are skipped, never followed, as following them lets a tampered-with wallet dir
    /// derail the walk two ways. One pointing at an ancestor makes the walk re-enter the tree once
    /// per level, until the kernel's symlink limit ends the chain: a couple of them in the same
    /// directory blow the number of walked paths up exponentially. One pointing outside `dir` has
    /// the migration read, rewrite or delete files anywhere on the filesystem, as it is the walked
    /// paths themselves that get rewritten.
    ///
    /// The walk is iterative, keeping the pending directories on the heap, so that a deeply nested
    /// tree cannot exhaust the stack: that abort could not be caught and reported, leaving the user
    /// with a crash and no indication of what to fix. It is also capped at `max_entries` entries,
    /// erroring out rather than running unbounded.
    fn visit_files(
        dir: &Path,
        max_entries: usize,
        f: &mut dyn FnMut(&Path) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut pending = vec![dir.to_path_buf()];
        let mut visited: usize = 0;
        while let Some(current) = pending.pop() {
            for entry in fs::read_dir(&current)? {
                let entry = entry?;
                visited += 1;
                if visited > max_entries {
                    return Err(Error::Internal {
                        details: format!(
                            "cannot migrate legacy RGB files: more than {max_entries} entries \
                             found under {}",
                            dir.display()
                        ),
                    });
                }
                // `DirEntry::file_type` reports the entry itself, while `Path::is_dir` would
                // report what a symlink resolves to
                let file_type = entry.file_type()?;
                if file_type.is_symlink() {
                    continue;
                }
                let path = entry.path();
                if file_type.is_dir() {
                    pending.push(path);
                } else {
                    f(&path)?;
                }
            }
        }
        Ok(())
    }

    /// Convert a legacy file (V0) to the current V1 layout.
    fn migrate_legacy_rgb_file(path: &Path) -> Result<(), Error> {
        if path.file_name().is_some_and(|n| n == FASCIA_FILE) {
            return migrate_legacy_fascia_file(path);
        }
        let bytes = fs::read(path)?;
        let Some((magic, body)) = bytes
            .strip_prefix(RGB_FILE_PREFIX)
            .filter(|rest| rest.len() > 3)
            .map(|rest| rest.split_at(3))
        else {
            return Ok(());
        };
        let transfer = match magic {
            b"VTF" => {
                fs::remove_file(path)?;
                return Ok(());
            }
            b"TFR" => true,
            b"CON" | b"VCO" => false,
            _ => return Ok(()),
        };
        // the strict-encoded consignment starts with its version byte: 0 for legacy V0 containers
        if body[0] != 0 {
            return Ok(());
        }
        // atomic: a crash mid-rewrite must not leave a torn file, which a re-run of the
        // migration would then be unable to recognize or decode
        if transfer {
            let consignment = decode_legacy_consignment::<true>(body, path)?;
            atomic_write_with(path, |tmp| Ok(consignment.save_file(tmp)?))
        } else {
            let consignment = decode_legacy_consignment::<false>(body, path)?;
            atomic_write_with(path, |tmp| Ok(consignment.save_file(tmp)?))
        }
    }

    fn decode_legacy_consignment<const TRANSFER: bool>(
        body: &[u8],
        path: &Path,
    ) -> Result<Consignment<TRANSFER>, Error> {
        let stream = StreamReader::new::<{ amplify::confinement::U32 }>(io::Cursor::new(body));
        let mut reader = StrictReader::with(stream);
        let v0 =
            ConsignmentV0::<TRANSFER>::strict_decode(&mut reader).map_err(|e| Error::Internal {
                details: format!(
                    "cannot decode legacy RGB consignment {}: {e}",
                    path.display()
                ),
            })?;
        v0.into_v1(None).map_err(|e| Error::Internal {
            details: format!(
                "cannot convert legacy RGB consignment {}: {e}",
                path.display()
            ),
        })
    }

    /// Convert a legacy fascia to the current one.
    ///
    /// A legacy fascia was stored as JSON; the current one is strict-encoded like every other RGB
    /// container, so both the layout and the encoding change. A strict-encoded fascia starts with
    /// its witness transaction's version, never with `{`, which is what tells the two apart.
    ///
    /// The legacy seal witness held a `public` witness whose transaction was serialized with the
    /// old bp types, in a JSON layout the current rust-bitcoin ones cannot read, and named its MPC
    /// merkle block `merkleBlock`. The transaction is therefore taken from the unsigned PSBT
    /// sitting next to the fascia in its transfer dir, whose TXID has to match the name of that
    /// dir, whatever is being migrated being the transfer that dir is named after.
    ///
    /// Idempotent: a fascia that is already strict-encoded is left untouched.
    fn migrate_legacy_fascia_file(path: &Path) -> Result<(), Error> {
        let internal = |details: String| Error::Internal { details };
        let bytes = fs::read(path)?;
        if bytes.first() != Some(&b'{') {
            return Ok(());
        }
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| internal(format!("cannot parse fascia {}: {e}", path.display())))?;
        // both fascia layouts carry a `sealWitness` object, so JSON without one is not a fascia
        // and is left alone, like every other file the migration does not recognize
        let Some(seal_witness) = value
            .get_mut("sealWitness")
            .and_then(|sw| sw.as_object_mut())
        else {
            return Ok(());
        };
        // the MPC merkle block field has since been renamed
        if let Some(merkle_block) = seal_witness.remove("merkleBlock") {
            seal_witness.insert(s!("mpcMerkleBlock"), merkle_block);
        }
        if seal_witness.remove("public").is_none() {
            // the current layout, only stored as JSON: re-save it strict-encoded
            let fascia: Fascia = serde_json::from_value(value)
                .map_err(|e| internal(format!("cannot read fascia {}: {e}", path.display())))?;
            return write_fascia(path, &fascia);
        }
        let transfer_dir = path.parent().expect("fascia file is inside a transfer dir");
        let psbt_str = fs::read_to_string(transfer_dir.join(UNSIGNED_PSBT_FILE))?;
        let psbt = Psbt::from_str(&psbt_str)?;
        let tx = psbt.unsigned_tx;
        let txid = tx.compute_txid().to_string();
        if transfer_dir.file_name().is_none_or(|d| d != txid.as_str()) {
            return Err(internal(format!(
                "cannot migrate fascia {}: PSBT TXID {txid} doesn't match the transfer dir",
                path.display()
            )));
        }
        seal_witness.insert(
            s!("tx"),
            serde_json::to_value(&tx).map_err(InternalError::from)?,
        );
        seal_witness.insert(s!("spvProof"), serde_json::Value::Null);
        // assignment seals used to be untagged (a revealed seal object or a concealed seal string);
        // they are now explicitly tagged
        if let Some(bundles) = value.get_mut("bundles").and_then(|b| b.as_object_mut()) {
            for seal in bundles
                .values_mut()
                .filter_map(|bundle| bundle.get_mut("knownTransitions")?.as_array_mut())
                .flatten()
                .filter_map(|kt| {
                    kt.get_mut("transition")?
                        .get_mut("assignments")?
                        .as_object_mut()
                })
                .flat_map(|assignments| assignments.values_mut())
                .filter_map(|typed| typed.get_mut("items")?.as_array_mut())
                .flatten()
                .filter_map(|assign| assign.get_mut("seal"))
            {
                let tag = if seal.is_string() {
                    "concealed"
                } else if seal.is_object() {
                    "revealed"
                } else {
                    return Err(internal(format!(
                        "cannot migrate fascia {}: an assignment seal is neither a concealed seal \
                         string nor a revealed seal object",
                        path.display()
                    )));
                };
                *seal = serde_json::json!({ tag: seal.take() });
            }
        }
        let fascia: Fascia = serde_json::from_value(value)
            .map_err(|e| internal(format!("cannot convert fascia {}: {e}", path.display())))?;
        write_fascia(path, &fascia)
    }

    fn migrate_assignment<State: StrictEncode + KnownState>(
        store: &mut &DbTxn,
        contract_id: ContractId,
        kind: AllocKind,
        oa: &OutputAssignment<State>,
    ) -> Result<(), Error> {
        let value = enc(&oa.state)?;
        // the legacy state holds the seal resolved, while it is now stored as defined. One naming
        // the witness of its own assignment was defined on the witness transaction: a transition
        // cannot name the TXID of the transaction committing to it
        let seal = match oa.witness {
            Some(witness) if witness == oa.seal.txid => AllocSeal::witness_vout(oa.seal.vout),
            _ => AllocSeal::explicit(oa.seal),
        };
        store.put_allocation(AllocationWrite {
            contract_id,
            kind,
            opout: oa.opout,
            seal,
            bundle_id: oa.bundle_id,
            value: &value,
        })
    }

    #[cfg(test)]
    mod test {
        use super::*;

        use std::collections::BTreeSet;

        /// Collect what `visit_files` hands to the callback, as file names.
        fn visited(dir: &Path, max_entries: usize) -> Result<BTreeSet<String>, Error> {
            let mut seen = BTreeSet::new();
            visit_files(dir, max_entries, &mut |path| {
                seen.insert(path.file_name().unwrap().to_string_lossy().into_owned());
                Ok(())
            })?;
            Ok(seen)
        }

        #[test]
        fn visits_nested_files() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            fs::create_dir_all(root.join("a/b/c")).unwrap();
            fs::write(root.join("top"), b"x").unwrap();
            fs::write(root.join("a/mid"), b"x").unwrap();
            fs::write(root.join("a/b/c/deep"), b"x").unwrap();

            let seen = visited(root, 100).unwrap();
            assert_eq!(
                seen,
                ["top", "mid", "deep"]
                    .map(str::to_string)
                    .into_iter()
                    .collect()
            );
        }

        /// Following a symlink pointing at an ancestor makes the walk re-enter the tree,
        /// which must not end in an infinite loop.
        #[test]
        fn symlink_loop_terminates() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            fs::create_dir(root.join("sub")).unwrap();
            fs::write(root.join("sub/real"), b"x").unwrap();
            std::os::unix::fs::symlink(root, root.join("sub/loop")).unwrap();

            let seen = visited(root, 100).unwrap();
            assert_eq!(seen, ["real"].map(str::to_string).into_iter().collect());
        }

        /// A symlink escaping the walked dir must not be handed to the callback, which reads,
        /// rewrites and deletes the files it is given.
        #[test]
        fn symlink_outside_is_not_visited() {
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("victim"), b"x").unwrap();
            let outside_dir = outside.path().join("dir");
            fs::create_dir(&outside_dir).unwrap();
            fs::write(outside_dir.join("nested_victim"), b"x").unwrap();

            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            fs::write(root.join("own"), b"x").unwrap();
            std::os::unix::fs::symlink(outside.path().join("victim"), root.join("to_file"))
                .unwrap();
            std::os::unix::fs::symlink(&outside_dir, root.join("to_dir")).unwrap();

            let seen = visited(root, 100).unwrap();
            assert_eq!(seen, ["own"].map(str::to_string).into_iter().collect());
            // and the escaped files are still there, untouched
            assert!(outside.path().join("victim").exists());
            assert!(outside_dir.join("nested_victim").exists());
        }

        #[test]
        fn errors_over_the_cap() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            for i in 0..5 {
                fs::write(root.join(format!("f{i}")), b"x").unwrap();
            }

            assert!(visited(root, 5).is_ok());
            let err = visited(root, 4).unwrap_err();
            assert_matches!(err, Error::Internal { details } if details.contains("more than 4"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bdk_wallet::bitcoin::absolute::LockTime;
    use strict_encoding::StrictDumb;

    use crate::wallet::core::RGB_LIB_DB_NAME;

    fn test_db() -> (RgbLibDatabase, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join(RGB_LIB_DB_NAME);
        let connection_string = format!("sqlite:{}?mode=rwc", db_path.display());
        let connection = block_on(Database::connect(connection_string)).unwrap();
        block_on(Migrator::up(&connection, None)).unwrap();
        (RgbLibDatabase::new(connection), dir)
    }

    // A seal on the witness transaction is stored once and lands on every witness of its bundle:
    // a whole read returns it as defined, a read at an outpoint returns it resolved to that
    // outpoint, and archiving one witness removes only that witness's outpoint. Two assignments
    // on the same output of one bundle are both found.
    #[test]
    fn witness_seal_lands_on_every_witness() {
        let (db, _dir) = test_db();
        let txn = db.begin_transaction().unwrap();
        let mut store: &DbTxn = &txn;

        let schema = Schema::strict_dumb();
        let mut genesis = Genesis::strict_dumb();
        genesis.schema_id = schema.schema_id();
        let contract_id = genesis.contract_id();
        store.put_schema(&schema).unwrap();
        store.put_genesis(&genesis).unwrap();

        let bundle = TransitionBundle::strict_dumb();
        let bundle_id = bundle.bundle_id();
        store.put_bundle(&bundle).unwrap();
        let witness_ids = [1u32, 2].map(|nonce| {
            let mut witness = SealWitness::strict_dumb();
            witness.tx.lock_time = LockTime::from_consensus(nonce);
            let id = witness.witness_id();
            store.put_witness(&witness).unwrap();
            store.put_bundle_witness(bundle_id, id).unwrap();
            store.put_witness_ord(id, WitnessOrd::Tentative).unwrap();
            id
        });

        let vout = 3u32;
        let opid = OpId::strict_dumb();
        let alloc = |no: u16, bundle_id: Option<BundleId>| AllocationWrite {
            contract_id,
            kind: AllocKind::Declarative,
            opout: Opout::new(opid, AssignmentType::strict_dumb(), no),
            seal: AllocSeal::witness_vout(vout),
            bundle_id,
            value: &[],
        };
        for no in [0, 1] {
            store.put_allocation(alloc(no, Some(bundle_id))).unwrap();
        }
        // consumed again under the other witness: the same row, not a new one
        store.put_allocation(alloc(0, Some(bundle_id))).unwrap();

        let whole = |store: &&DbTxn| {
            store
                .allocations(contract_id, AllocationFilter::all(Visibility::Valid))
                .unwrap()
        };
        let rows = whole(&store);
        assert_eq!(rows.len(), 2, "one row per assignment, not per witness");
        assert!(
            rows.iter()
                .all(|row| row.seal == AllocSeal::witness_vout(vout))
        );

        let at = |store: &&DbTxn, txid: RgbTxid, vout: u32, visibility| {
            let outpoints = BTreeSet::from([OutPoint::new(txid, vout)]);
            let rows = store
                .allocations(
                    contract_id,
                    AllocationFilter::all(visibility).at(&outpoints),
                )
                .unwrap();
            let across = store
                .all_allocations_at_outpoints(&outpoints, visibility)
                .unwrap();
            assert_eq!(rows.len(), across.len(), "both outpoint reads must agree");
            let resolved = AllocSeal::explicit(OutputSeal::with(txid, Vout::from_u32(vout)));
            assert!(rows.iter().all(|row| row.seal == resolved));
            rows.len()
        };
        for id in witness_ids {
            assert_eq!(at(&store, id, vout, Visibility::Valid), 2);
            assert_eq!(at(&store, id, vout + 1, Visibility::Valid), 0);
        }

        store
            .put_witness_ord(witness_ids[1], WitnessOrd::Archived)
            .unwrap();
        assert_eq!(at(&store, witness_ids[1], vout, Visibility::Valid), 0);
        assert_eq!(at(&store, witness_ids[1], vout, Visibility::All), 2);
        assert_eq!(at(&store, witness_ids[0], vout, Visibility::Valid), 2);
        assert_eq!(
            whole(&store).len(),
            2,
            "the bundle is still validly witnessed"
        );

        store.set_op_validity(opid, false).unwrap();
        assert_eq!(at(&store, witness_ids[0], vout, Visibility::Valid), 0);
        assert_eq!(at(&store, witness_ids[0], vout, Visibility::All), 2);

        // genesis has no witness for such a seal to land on
        assert!(store.put_allocation(alloc(2, None)).is_err());
    }
}
