//! Persistence of the BDK wallet [`ChangeSet`] inside rgb-lib's own database.
//!
//! This reimplements, over sea-orm, the mapping that `bdk_chain` performs against `rusqlite`: it
//! must go through the very same [`DatabaseTransaction`] rgb-lib is already using (SQLite is
//! single-writer and rgb-lib holds the single pooled connection across BDK persist points, so a
//! separate connection would deadlock).
//!
//! The tables mirror `bdk_chain`'s schema (names singularized to match rgb-lib's convention, and
//! versioning handled by rgb-lib's own migrator rather than a `bdk_schemas` table). Keys follow
//! rgb-lib's convention too: every table but the single-row wallet one has an auto-increment `idx`
//! primary key, with `bdk_chain`'s natural key kept as a unique index instead. The one part left
//! out is the derived SPK cache: BDK only ever stages it when the wallet is built with
//! `use_spk_cache`, which rgb-lib does not enable, so `ChangeSet::spk_cache` is always empty here.
//!
//! The signing and watch-only wallets of a fingerprint share the same store: their persisted
//! `ChangeSet` is identical (BDK keeps only the public descriptor and the private keys live in the
//! in-memory signers), so there is nothing to distinguish.

use super::*;

use crate::database::entities::{
    bdk_anchor, bdk_block, bdk_descriptor_last_revealed, bdk_tx, bdk_txout,
    bdk_wallet as bdk_wallet_entity, bdk_wallet_locked_outpoint,
    prelude::{
        BdkAnchor, BdkBlock, BdkDescriptorLastRevealed, BdkTx, BdkTxout,
        BdkWallet as BdkWalletEntity, BdkWalletLockedOutpoint,
    },
};

// The wallet-level data is stored in a single row, pinned to this ID, as in BDK.
const BDK_WALLET_ID: i32 = 0;

/// The `bdk_tx` columns a [`bdk_wallet::chain::tx_graph::ChangeSet`] can carry for one transaction.
#[derive(Default)]
struct TxRow {
    raw_tx: Option<Vec<u8>>,
    first_seen: Option<u64>,
    last_seen: Option<u64>,
    last_evicted: Option<u64>,
}

fn encode_tx(tx: &BdkTransaction) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    tx.consensus_encode(&mut bytes)
        .map_err(InternalError::from)?;
    Ok(bytes)
}

fn decode_tx(bytes: &[u8]) -> Result<BdkTransaction, Error> {
    let tx = BdkTransaction::consensus_decode_from_finite_reader(&mut &bytes[..])
        .map_err(InternalError::from)?;
    Ok(tx)
}

/// Upsert expression keeping the stored `bdk_tx` value when the incoming one is NULL.
fn keep_stored_bdk_tx(column: bdk_tx::Column) -> SimpleExpr {
    Func::coalesce([
        Expr::col((Alias::new("excluded"), column)),
        Expr::col((bdk_tx::Entity, column)),
    ])
    .into()
}

impl DbTxn {
    /// Get the aggregate BDK [`ChangeSet`] from the database.
    pub(crate) fn get_bdk_changeset(&self) -> Result<ChangeSet, Error> {
        block_on(load_changeset(self.inner()))
    }

    /// Merge the given BDK [`ChangeSet`] delta into what is stored.
    pub(crate) fn update_bdk_changeset(&self, changeset: &ChangeSet) -> Result<(), Error> {
        block_on(persist_changeset(self.inner(), changeset))
    }
}

async fn load_changeset(connection: &DatabaseTransaction) -> Result<ChangeSet, Error> {
    let mut changeset = ChangeSet::default();

    // wallet-level data
    if let Some(row) = BdkWalletEntity::find_by_id(BDK_WALLET_ID)
        .one(connection)
        .await?
    {
        changeset.descriptor = row
            .descriptor
            .map(|d| Descriptor::<DescriptorPublicKey>::from_str(&d))
            .transpose()
            .map_err(InternalError::from)?;
        changeset.change_descriptor = row
            .change_descriptor
            .map(|d| Descriptor::<DescriptorPublicKey>::from_str(&d))
            .transpose()
            .map_err(InternalError::from)?;
        changeset.network = row
            .network
            .map(|n| BdkNetwork::from_str(&n))
            .transpose()
            .map_err(InternalError::from)?;
    }

    // locked outpoints
    for row in BdkWalletLockedOutpoint::find().all(connection).await? {
        let outpoint = OutPoint::new(
            Txid::from_str(&row.txid).map_err(InternalError::from)?,
            row.vout,
        );
        changeset.locked_outpoints.outpoints.insert(outpoint, true);
    }

    // local chain
    for row in BdkBlock::find().all(connection).await? {
        let hash = BlockHash::from_str(&row.block_hash).map_err(InternalError::from)?;
        changeset
            .local_chain
            .blocks
            .insert(row.block_height, Some(hash));
    }

    // tx graph
    for row in BdkTx::find().all(connection).await? {
        let txid = Txid::from_str(&row.txid).map_err(InternalError::from)?;
        if let Some(raw_tx) = row.raw_tx {
            changeset.tx_graph.txs.insert(Arc::new(decode_tx(&raw_tx)?));
        }
        if let Some(first_seen) = row.first_seen {
            changeset
                .tx_graph
                .first_seen
                .insert(txid, first_seen.parse().map_err(InternalError::from)?);
        }
        if let Some(last_seen) = row.last_seen {
            changeset
                .tx_graph
                .last_seen
                .insert(txid, last_seen.parse().map_err(InternalError::from)?);
        }
        if let Some(last_evicted) = row.last_evicted {
            changeset
                .tx_graph
                .last_evicted
                .insert(txid, last_evicted.parse().map_err(InternalError::from)?);
        }
    }
    for row in BdkTxout::find().all(connection).await? {
        let outpoint = OutPoint::new(
            Txid::from_str(&row.txid).map_err(InternalError::from)?,
            row.vout,
        );
        changeset.tx_graph.txouts.insert(
            outpoint,
            TxOut {
                value: BdkAmount::from_sat(row.value.parse().map_err(InternalError::from)?),
                script_pubkey: ScriptBuf::from_bytes(row.script),
            },
        );
    }
    for row in BdkAnchor::find().all(connection).await? {
        let txid = Txid::from_str(&row.txid).map_err(InternalError::from)?;
        let hash = BlockHash::from_str(&row.block_hash).map_err(InternalError::from)?;
        changeset.tx_graph.anchors.insert((
            ConfirmationBlockTime {
                block_id: BlockId {
                    height: row.block_height,
                    hash,
                },
                confirmation_time: row.confirmation_time.parse().map_err(InternalError::from)?,
            },
            txid,
        ));
    }

    // indexer (`spk_cache` is left out, see the module note)
    for row in BdkDescriptorLastRevealed::find().all(connection).await? {
        let descriptor_id =
            DescriptorId::from_str(&row.descriptor_id).map_err(InternalError::from)?;
        changeset
            .indexer
            .last_revealed
            .insert(descriptor_id, row.last_revealed);
    }

    Ok(changeset)
}

async fn persist_changeset(
    connection: &DatabaseTransaction,
    changeset: &ChangeSet,
) -> Result<(), Error> {
    // wallet-level data
    if let Some(descriptor) = &changeset.descriptor {
        BdkWalletEntity::insert(bdk_wallet_entity::ActiveModel {
            id: ActiveValue::Set(BDK_WALLET_ID),
            descriptor: ActiveValue::Set(Some(descriptor.to_string())),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(bdk_wallet_entity::Column::Id)
                .update_column(bdk_wallet_entity::Column::Descriptor)
                .to_owned(),
        )
        .exec(connection)
        .await?;
    }
    if let Some(change_descriptor) = &changeset.change_descriptor {
        BdkWalletEntity::insert(bdk_wallet_entity::ActiveModel {
            id: ActiveValue::Set(BDK_WALLET_ID),
            change_descriptor: ActiveValue::Set(Some(change_descriptor.to_string())),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(bdk_wallet_entity::Column::Id)
                .update_column(bdk_wallet_entity::Column::ChangeDescriptor)
                .to_owned(),
        )
        .exec(connection)
        .await?;
    }
    if let Some(network) = &changeset.network {
        BdkWalletEntity::insert(bdk_wallet_entity::ActiveModel {
            id: ActiveValue::Set(BDK_WALLET_ID),
            network: ActiveValue::Set(Some(network.to_string())),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(bdk_wallet_entity::Column::Id)
                .update_column(bdk_wallet_entity::Column::Network)
                .to_owned(),
        )
        .exec(connection)
        .await?;
    }

    // locked outpoints
    for (outpoint, is_locked) in &changeset.locked_outpoints.outpoints {
        if *is_locked {
            let res = BdkWalletLockedOutpoint::insert(bdk_wallet_locked_outpoint::ActiveModel {
                idx: ActiveValue::NotSet,
                txid: ActiveValue::Set(outpoint.txid.to_string()),
                vout: ActiveValue::Set(outpoint.vout),
            })
            .on_conflict(
                OnConflict::columns([
                    bdk_wallet_locked_outpoint::Column::Txid,
                    bdk_wallet_locked_outpoint::Column::Vout,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec(connection)
            .await;
            // this returns RecordNotInserted if the outpoint is already stored and on_conflict is
            // do_nothing, which is the intended outcome for insert-or-ignore
            match res {
                Ok(_) | Err(DbErr::RecordNotInserted) => {}
                Err(err) => return Err(err.into()),
            }
        } else {
            BdkWalletLockedOutpoint::delete_many()
                .filter(bdk_wallet_locked_outpoint::Column::Txid.eq(outpoint.txid.to_string()))
                .filter(bdk_wallet_locked_outpoint::Column::Vout.eq(outpoint.vout))
                .exec(connection)
                .await?;
        }
    }

    // local chain
    for (height, hash) in &changeset.local_chain.blocks {
        match hash {
            Some(hash) => {
                BdkBlock::insert(bdk_block::ActiveModel {
                    idx: ActiveValue::NotSet,
                    block_height: ActiveValue::Set(*height),
                    block_hash: ActiveValue::Set(hash.to_string()),
                })
                .on_conflict(
                    OnConflict::column(bdk_block::Column::BlockHeight)
                        .update_column(bdk_block::Column::BlockHash)
                        .to_owned(),
                )
                .exec(connection)
                .await?;
            }
            None => {
                BdkBlock::delete_many()
                    .filter(bdk_block::Column::BlockHeight.eq(*height))
                    .exec(connection)
                    .await?;
            }
        }
    }

    // tx graph: a transaction's raw bytes and its timestamps arrive in separate maps, and an
    // anchor needs its transaction to exist for the foreign key, so gather them and write each
    // txid once
    let mut tx_rows: BTreeMap<Txid, TxRow> = BTreeMap::new();
    for tx in &changeset.tx_graph.txs {
        tx_rows.entry(tx.compute_txid()).or_default().raw_tx = Some(encode_tx(tx)?);
    }
    for (txid, first_seen) in &changeset.tx_graph.first_seen {
        tx_rows.entry(*txid).or_default().first_seen = Some(*first_seen);
    }
    for (txid, last_seen) in &changeset.tx_graph.last_seen {
        tx_rows.entry(*txid).or_default().last_seen = Some(*last_seen);
    }
    for (txid, last_evicted) in &changeset.tx_graph.last_evicted {
        tx_rows.entry(*txid).or_default().last_evicted = Some(*last_evicted);
    }
    // the anchor references its transaction, which may not be stored in full yet; a stub row
    // keeps the foreign key satisfied, exactly as bdk_chain does
    for (_, txid) in &changeset.tx_graph.anchors {
        tx_rows.entry(*txid).or_default();
    }
    for (txid, row) in tx_rows {
        BdkTx::insert(bdk_tx::ActiveModel {
            idx: ActiveValue::NotSet,
            txid: ActiveValue::Set(txid.to_string()),
            raw_tx: ActiveValue::Set(row.raw_tx),
            first_seen: ActiveValue::Set(row.first_seen.map(|v| v.to_string())),
            last_seen: ActiveValue::Set(row.last_seen.map(|v| v.to_string())),
            last_evicted: ActiveValue::Set(row.last_evicted.map(|v| v.to_string())),
        })
        .on_conflict(
            // a changeset carries any subset of these columns, so an absent value must leave
            // what is already stored alone instead of overwriting it with NULL
            OnConflict::column(bdk_tx::Column::Txid)
                .value(
                    bdk_tx::Column::RawTx,
                    keep_stored_bdk_tx(bdk_tx::Column::RawTx),
                )
                .value(
                    bdk_tx::Column::FirstSeen,
                    keep_stored_bdk_tx(bdk_tx::Column::FirstSeen),
                )
                .value(
                    bdk_tx::Column::LastSeen,
                    keep_stored_bdk_tx(bdk_tx::Column::LastSeen),
                )
                .value(
                    bdk_tx::Column::LastEvicted,
                    keep_stored_bdk_tx(bdk_tx::Column::LastEvicted),
                )
                .to_owned(),
        )
        .exec(connection)
        .await?;
    }
    for (outpoint, txout) in &changeset.tx_graph.txouts {
        BdkTxout::insert(bdk_txout::ActiveModel {
            idx: ActiveValue::NotSet,
            txid: ActiveValue::Set(outpoint.txid.to_string()),
            vout: ActiveValue::Set(outpoint.vout),
            value: ActiveValue::Set(txout.value.to_sat().to_string()),
            script: ActiveValue::Set(txout.script_pubkey.to_bytes()),
        })
        .on_conflict(
            OnConflict::columns([bdk_txout::Column::Txid, bdk_txout::Column::Vout])
                .update_columns([bdk_txout::Column::Value, bdk_txout::Column::Script])
                .to_owned(),
        )
        .exec(connection)
        .await?;
    }
    for (anchor, txid) in &changeset.tx_graph.anchors {
        BdkAnchor::insert(bdk_anchor::ActiveModel {
            idx: ActiveValue::NotSet,
            txid: ActiveValue::Set(txid.to_string()),
            block_height: ActiveValue::Set(anchor.block_id.height),
            block_hash: ActiveValue::Set(anchor.block_id.hash.to_string()),
            confirmation_time: ActiveValue::Set(anchor.confirmation_time.to_string()),
        })
        .on_conflict(
            OnConflict::columns([
                bdk_anchor::Column::Txid,
                bdk_anchor::Column::BlockHeight,
                bdk_anchor::Column::BlockHash,
            ])
            .update_column(bdk_anchor::Column::ConfirmationTime)
            .to_owned(),
        )
        .exec(connection)
        .await?;
    }

    // indexer (`spk_cache` is left out, see the module note)
    for (descriptor_id, last_revealed) in &changeset.indexer.last_revealed {
        let res = BdkDescriptorLastRevealed::insert(bdk_descriptor_last_revealed::ActiveModel {
            idx: ActiveValue::NotSet,
            descriptor_id: ActiveValue::Set(descriptor_id.to_string()),
            last_revealed: ActiveValue::Set(*last_revealed),
        })
        // never lower the stored index: BDK's `ChangeSet::merge` keeps the greater value and the
        // persisted one has to behave the same way, or a stale writer would silently un-reveal
        // indices that have already been handed out
        .on_conflict(
            OnConflict::column(bdk_descriptor_last_revealed::Column::DescriptorId)
                .update_column(bdk_descriptor_last_revealed::Column::LastRevealed)
                .action_and_where(
                    Expr::col((
                        Alias::new("excluded"),
                        bdk_descriptor_last_revealed::Column::LastRevealed,
                    ))
                    .gt(Expr::col((
                        bdk_descriptor_last_revealed::Entity,
                        bdk_descriptor_last_revealed::Column::LastRevealed,
                    ))),
                )
                .to_owned(),
        )
        .exec(connection)
        .await;
        // the guarded update touches no row when the stored index is already the greater one
        match res {
            Ok(_) | Err(DbErr::RecordNotInserted) => {}
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicBool, Ordering};

    use bdk_wallet::bitcoin::{TxIn, absolute::LockTime, transaction::Version};

    use crate::wallet::core::RGB_LIB_DB_NAME;

    const TPUB: &str = "tpubD6NzVbkrYhZ4WLczPJWReQycCJdd6YVWXubbVUFnJ5KgU5MDQrD998ZJLSmaB7GVcCnJSDWprxmrGkJ6SvgQC6QAffVpqSvonXmeizXcrkN";

    fn test_db() -> (RgbLibDatabase, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join(RGB_LIB_DB_NAME);
        let connection_string = format!("sqlite:{}?mode=rwc", db_path.display());
        let connection = block_on(Database::connect(connection_string)).unwrap();
        block_on(Migrator::up(&connection, None)).unwrap();
        (RgbLibDatabase::new(connection), dir)
    }

    // Persist a fully-populated ChangeSet and read it back, exercising every table (wallet-level,
    // locked outpoints, blocks, tx_graph, anchors, txouts, indexer) which the offline wallet tests
    // do not reach.
    #[test]
    fn changeset_round_trip() {
        let (db, _dir) = test_db();

        let tx = BdkTransaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn::default()],
            output: vec![TxOut {
                value: BdkAmount::from_sat(1000),
                script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
            }],
        };
        let txid = tx.compute_txid();
        let descriptor_id = DescriptorId::from_str(&"03".repeat(32)).unwrap();
        let descriptor =
            Descriptor::<DescriptorPublicKey>::from_str(&format!("wpkh({TPUB}/0/*)")).unwrap();
        let change_descriptor =
            Descriptor::<DescriptorPublicKey>::from_str(&format!("wpkh({TPUB}/1/*)")).unwrap();

        let mut changeset = ChangeSet {
            descriptor: Some(descriptor),
            change_descriptor: Some(change_descriptor),
            network: Some(BdkNetwork::Regtest),
            ..Default::default()
        };
        changeset
            .local_chain
            .blocks
            .insert(100, Some(BlockHash::from_byte_array([7; 32])));
        changeset.tx_graph.txs.insert(Arc::new(tx.clone()));
        changeset.tx_graph.txouts.insert(
            OutPoint::new(txid, 0),
            TxOut {
                value: BdkAmount::from_sat(2000),
                script_pubkey: ScriptBuf::from_bytes(vec![0x52]),
            },
        );
        changeset.tx_graph.anchors.insert((
            ConfirmationBlockTime {
                block_id: BlockId {
                    height: 123,
                    hash: BlockHash::from_byte_array([9; 32]),
                },
                confirmation_time: 456,
            },
            txid,
        ));
        changeset.tx_graph.first_seen.insert(txid, 111);
        changeset.tx_graph.last_seen.insert(txid, 222);
        // the u64 columns must cover their full range, beyond i64::MAX
        changeset.tx_graph.last_evicted.insert(txid, u64::MAX);
        changeset.indexer.last_revealed.insert(descriptor_id, 5);
        changeset
            .locked_outpoints
            .outpoints
            .insert(OutPoint::new(txid, 1), true);

        // persist twice through separate transactions to also cover the merge/upsert path
        for _ in 0..2 {
            let txn = db.begin_transaction().unwrap();
            txn.update_bdk_changeset(&changeset).unwrap();
            txn.commit().unwrap();
        }

        let txn = db.begin_transaction().unwrap();
        let loaded = txn.get_bdk_changeset().unwrap();
        txn.commit().unwrap();

        assert_eq!(loaded, changeset);
    }

    // A changeset carries any subset of a transaction's columns: writing one must not blank out
    // the others, since raw bytes and each timestamp arrive in separate maps.
    #[test]
    fn partial_tx_changeset_keeps_stored_columns() {
        let (db, _dir) = test_db();

        let tx = BdkTransaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn::default()],
            output: vec![TxOut {
                value: BdkAmount::from_sat(1000),
                script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
            }],
        };
        let txid = tx.compute_txid();

        let persist = |changeset: &ChangeSet| {
            let txn = db.begin_transaction().unwrap();
            txn.update_bdk_changeset(changeset).unwrap();
            txn.commit().unwrap();
        };
        let load = || {
            let txn = db.begin_transaction().unwrap();
            let changeset = txn.get_bdk_changeset().unwrap();
            txn.commit().unwrap();
            changeset
        };

        // the full transaction, with one timestamp
        let mut changeset = ChangeSet::default();
        changeset.tx_graph.txs.insert(Arc::new(tx.clone()));
        changeset.tx_graph.first_seen.insert(txid, 111);
        persist(&changeset);

        // a later changeset carrying only another timestamp
        let mut changeset = ChangeSet::default();
        changeset.tx_graph.last_seen.insert(txid, 222);
        persist(&changeset);

        let loaded = load();
        assert_eq!(
            loaded.tx_graph.txs.iter().next().map(|t| t.compute_txid()),
            Some(txid),
            "raw transaction was lost"
        );
        assert_eq!(loaded.tx_graph.first_seen.get(&txid), Some(&111));
        assert_eq!(loaded.tx_graph.last_seen.get(&txid), Some(&222));
    }

    // `last_revealed` must never regress: BDK's `ChangeSet::merge` keeps the greater value, so
    // the persisted one has to as well, or a writer holding a stale index would un-reveal indices
    // that have already been handed out.
    #[test]
    fn last_revealed_never_regresses() {
        let (db, _dir) = test_db();
        let descriptor_id = DescriptorId::from_str(&"03".repeat(32)).unwrap();

        let persist = |last_revealed: u32| {
            let mut changeset = ChangeSet::default();
            changeset
                .indexer
                .last_revealed
                .insert(descriptor_id, last_revealed);
            let txn = db.begin_transaction().unwrap();
            txn.update_bdk_changeset(&changeset).unwrap();
            txn.commit().unwrap();
        };
        let load = || {
            let txn = db.begin_transaction().unwrap();
            let changeset = txn.get_bdk_changeset().unwrap();
            txn.commit().unwrap();
            changeset.indexer.last_revealed.get(&descriptor_id).copied()
        };

        persist(4);
        assert_eq!(load(), Some(4));
        // a writer holding a stale index must not lower it
        persist(0);
        assert_eq!(load(), Some(4));
        // ...but a higher one still wins
        persist(7);
        assert_eq!(load(), Some(7));
    }

    // Callbacks registered on a transaction must run only if it commits: nothing still holding
    // the BDK changes (the in-memory pending buffer, its on-disk copy, the legacy store) may be
    // discarded before they are durably in the DB.
    #[test]
    fn on_commit_runs_only_when_committed() {
        let (db, _dir) = test_db();

        let committed = Arc::new(AtomicBool::new(false));
        let txn = db.begin_transaction().unwrap();
        let flag = Arc::clone(&committed);
        txn.on_commit(move || flag.store(true, Ordering::Relaxed));
        assert!(!committed.load(Ordering::Relaxed));
        txn.commit().unwrap();
        assert!(committed.load(Ordering::Relaxed));

        let rolled_back = Arc::new(AtomicBool::new(false));
        let txn = db.begin_transaction().unwrap();
        let flag = Arc::clone(&rolled_back);
        txn.on_commit(move || flag.store(true, Ordering::Relaxed));
        drop(txn);
        assert!(!rolled_back.load(Ordering::Relaxed));
    }

    // A failing savepoint must roll back only its own changes and callbacks, while a succeeding
    // one keeps them, with its callbacks running only once the enclosing transaction commits.
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    #[test]
    fn savepoint_rolls_back_only_on_error() {
        let (db, _dir) = test_db();

        let changeset_with_tx = |out: u8| {
            let tx = BdkTransaction {
                version: Version::TWO,
                lock_time: LockTime::ZERO,
                input: vec![TxIn::default()],
                output: vec![TxOut {
                    value: BdkAmount::from_sat(1000),
                    script_pubkey: ScriptBuf::from_bytes(vec![out]),
                }],
            };
            let txid = tx.compute_txid();
            let mut changeset = ChangeSet::default();
            changeset.tx_graph.txs.insert(Arc::new(tx));
            (changeset, txid)
        };
        let (kept_changeset, kept_txid) = changeset_with_tx(0x51);
        let (dropped_changeset, dropped_txid) = changeset_with_tx(0x52);

        let kept_cb = Arc::new(AtomicBool::new(false));
        let dropped_cb = Arc::new(AtomicBool::new(false));
        let txn = db.begin_transaction().unwrap();
        let flag = Arc::clone(&kept_cb);
        txn.with_savepoint(|sp| {
            sp.update_bdk_changeset(&kept_changeset)?;
            sp.on_commit(move || flag.store(true, Ordering::Relaxed));
            Ok(())
        })
        .unwrap();
        let flag = Arc::clone(&dropped_cb);
        let res: Result<(), Error> = txn.with_savepoint(|sp| {
            sp.update_bdk_changeset(&dropped_changeset)?;
            sp.on_commit(move || flag.store(true, Ordering::Relaxed));
            Err(Error::Internal {
                details: s!("savepoint failure"),
            })
        });
        assert!(res.is_err());
        assert!(!kept_cb.load(Ordering::Relaxed));
        txn.commit().unwrap();
        assert!(kept_cb.load(Ordering::Relaxed));
        assert!(!dropped_cb.load(Ordering::Relaxed));

        let txn = db.begin_transaction().unwrap();
        let txids: Vec<_> = txn
            .get_bdk_changeset()
            .unwrap()
            .tx_graph
            .txs
            .iter()
            .map(|t| t.compute_txid())
            .collect();
        txn.commit().unwrap();
        assert!(txids.contains(&kept_txid));
        assert!(!txids.contains(&dropped_txid));
    }
}
