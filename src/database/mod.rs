pub(crate) mod entities;

use super::*;

use crate::database::entities::{
    asset, coloring, media, prelude::*, transfer_transport_endpoint, transport_endpoint, txo,
    wallet_transaction,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::database::entities::{batch_transfer, pending_witness_script, reserved_txo};

#[derive(Debug, Clone)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) struct DbAssetTransferData {
    pub(crate) asset_transfer: DbAssetTransfer,
    pub(crate) transfers: Vec<DbTransfer>,
}

impl DbBatchTransfer {
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn incoming(
        &self,
        asset_transfers: &[DbAssetTransfer],
        transfers: &[DbTransfer],
    ) -> bool {
        let asset_transfer_ids: Vec<i32> = asset_transfers
            .iter()
            .filter(|t| t.batch_transfer_idx == self.idx)
            .map(|t| t.idx)
            .collect();
        transfers
            .iter()
            .filter(|t| asset_transfer_ids.contains(&t.asset_transfer_idx))
            .all(|t| t.incoming)
    }

    pub(crate) fn get_asset_transfers(
        &self,
        asset_transfers: &[DbAssetTransfer],
    ) -> Vec<DbAssetTransfer> {
        asset_transfers
            .iter()
            .filter(|&t| t.batch_transfer_idx == self.idx)
            .cloned()
            .collect()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn get_transfers(
        &self,
        asset_transfers: &[DbAssetTransfer],
        transfers: &[DbTransfer],
    ) -> Result<DbBatchTransferData, Error> {
        let asset_transfers = self.get_asset_transfers(asset_transfers);
        let mut asset_transfers_data = vec![];
        for asset_transfer in asset_transfers {
            let transfers: Vec<DbTransfer> = transfers
                .iter()
                .filter(|&t| asset_transfer.idx == t.asset_transfer_idx)
                .cloned()
                .collect();
            asset_transfers_data.push(DbAssetTransferData {
                asset_transfer,
                transfers,
            })
        }
        Ok(DbBatchTransferData {
            asset_transfers_data,
        })
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn get_incoming_transfer(
        &self,
        asset_transfers: &[DbAssetTransfer],
        transfers: &[DbTransfer],
    ) -> Result<(DbAssetTransfer, DbTransfer), Error> {
        let batch_transfer_data = self.get_transfers(asset_transfers, transfers)?;
        let asset_transfer_data = batch_transfer_data
            .asset_transfers_data
            .first() // incoming batch transfer has only one asset transfer
            .expect("asset transfer should be connected to a batch transfer");
        let transfer = asset_transfer_data
            .transfers
            .first() // incoming asset transfer has only one transfer
            .expect("transfer should be connected to an asset transfer");
        Ok((asset_transfer_data.asset_transfer.clone(), transfer.clone()))
    }

    pub(crate) fn failed(&self) -> bool {
        self.status.failed()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn waiting(&self) -> bool {
        self.status.waiting()
    }

    pub(crate) fn waiting_confirmations(&self) -> bool {
        self.status.waiting_confirmations()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn is_fallible(&self) -> bool {
        self.status.is_fallible()
    }
}

#[derive(Debug, Clone)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) struct DbBatchTransferData {
    pub(crate) asset_transfers_data: Vec<DbAssetTransferData>,
}

impl DbColoring {
    pub(crate) fn incoming(&self) -> bool {
        [
            ColoringType::Receive,
            ColoringType::Change,
            ColoringType::Issue,
        ]
        .contains(&self.r#type)
    }
}

pub struct DbData {
    pub(crate) batch_transfers: Vec<DbBatchTransfer>,
    pub(crate) asset_transfers: Vec<DbAssetTransfer>,
    pub(crate) transfers: Vec<DbTransfer>,
    pub(crate) colorings: Vec<DbColoring>,
    pub(crate) txos: Vec<DbTxo>,
}

impl DbTransfer {
    pub(crate) fn related_transfers(
        &self,
        asset_transfers: &[DbAssetTransfer],
        batch_transfers: &[DbBatchTransfer],
    ) -> (DbAssetTransfer, DbBatchTransfer) {
        let asset_transfer = asset_transfers
            .iter()
            .find(|t| t.idx == self.asset_transfer_idx)
            .expect("transfer should be connected to an asset transfer");
        let batch_transfer = batch_transfers
            .iter()
            .find(|t| t.idx == asset_transfer.batch_transfer_idx)
            .expect("asset transfer should be connected to a batch transfer");

        (asset_transfer.clone(), batch_transfer.clone())
    }
}

impl DbTxo {
    pub(crate) fn outpoint(&self) -> Outpoint {
        Outpoint {
            txid: self.txid.to_string(),
            vout: self.vout,
        }
    }

    fn get_utxo_allocations(
        &self,
        colorings: &[DbColoring],
        asset_transfers: &[DbAssetTransfer],
        batch_transfers: &[DbBatchTransfer],
    ) -> Result<Vec<LocalRgbAllocation>, Error> {
        let utxo_colorings: Vec<&DbColoring> =
            colorings.iter().filter(|c| c.txo_idx == self.idx).collect();

        let mut allocations: Vec<LocalRgbAllocation> = vec![];
        utxo_colorings.iter().for_each(|c| {
            let asset_transfer: &DbAssetTransfer = asset_transfers
                .iter()
                .find(|t| t.idx == c.asset_transfer_idx)
                .expect("coloring should be connected to an asset transfer");
            let batch_transfer: &DbBatchTransfer = batch_transfers
                .iter()
                .find(|t| asset_transfer.batch_transfer_idx == t.idx)
                .expect("asset transfer should be connected to a batch transfer");

            allocations.push(LocalRgbAllocation {
                asset_id: asset_transfer.asset_id.clone(),
                assignment: c.assignment.clone(),
                status: batch_transfer.status,
                incoming: c.incoming(),
                txo_spent: self.spent,
            });
        });

        Ok(allocations)
    }
}

impl From<DbReservedTxo> for BdkOutPoint {
    fn from(r: DbReservedTxo) -> BdkOutPoint {
        BdkOutPoint::from_str(&format!("{}:{}", r.txid, r.vout))
            .expect("DB should contain a valid outpoint")
    }
}

impl From<DbTxo> for BdkOutPoint {
    fn from(x: DbTxo) -> BdkOutPoint {
        BdkOutPoint::from_str(&x.outpoint().to_string())
            .expect("DB should contain a valid outpoint")
    }
}

impl From<LocalOutput> for DbTxoActMod {
    fn from(x: LocalOutput) -> DbTxoActMod {
        DbTxoActMod {
            idx: ActiveValue::NotSet,
            txid: ActiveValue::Set(x.outpoint.txid.to_string()),
            vout: ActiveValue::Set(x.outpoint.vout),
            btc_amount: ActiveValue::Set(x.txout.value.to_sat().to_string()),
            spent: ActiveValue::Set(x.is_spent),
            exists: ActiveValue::Set(true),
            pending_witness: ActiveValue::Set(false),
        }
    }
}

pub struct RgbLibDatabase {
    connection: DatabaseConnection,
}

impl RgbLibDatabase {
    pub(crate) fn new(connection: DatabaseConnection) -> Self {
        Self { connection }
    }

    pub(crate) fn begin_transaction(&self) -> Result<DbTxn, Error> {
        Ok(DbTxn {
            txn: Some(block_on(self.connection.begin())?),
        })
    }
}

pub struct DbTxn {
    txn: Option<DatabaseTransaction>,
}

impl Drop for DbTxn {
    fn drop(&mut self) {
        if let Some(txn) = self.txn.take() {
            // run the rollback inside our runtime so the async
            // release has a tokio context (panics otherwise)
            let _ = block_on(txn.rollback());
        }
    }
}

impl DbTxn {
    fn inner(&self) -> &DatabaseTransaction {
        self.txn.as_ref().expect("txn already consumed")
    }

    pub(crate) fn commit(mut self) -> Result<(), Error> {
        let txn = self.txn.take().expect("txn already consumed");
        Ok(block_on(txn.commit())?)
    }

    pub(crate) fn set_asset(&self, asset: DbAssetActMod) -> Result<i32, Error> {
        let res = block_on(Asset::insert(asset).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_asset_transfer(
        &self,
        asset_transfer: DbAssetTransferActMod,
    ) -> Result<i32, Error> {
        let res = block_on(AssetTransfer::insert(asset_transfer).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_backup_info(&self, backup_info: DbBackupInfoActMod) -> Result<i32, Error> {
        let res = block_on(BackupInfo::insert(backup_info).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_batch_transfer(
        &self,
        mut batch_transfer: DbBatchTransferActMod,
    ) -> Result<i32, Error> {
        batch_transfer.updated_at = batch_transfer.created_at.clone();
        let res = block_on(BatchTransfer::insert(batch_transfer).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_coloring(&self, coloring: DbColoringActMod) -> Result<i32, Error> {
        let res = block_on(Coloring::insert(coloring).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_media(&self, media: DbMediaActMod) -> Result<i32, Error> {
        let res = block_on(Media::insert(media).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_pending_witness_script(
        &self,
        pending_witness_script: DbPendingWitnessScriptActMod,
    ) -> Result<i32, Error> {
        let res =
            block_on(PendingWitnessScript::insert(pending_witness_script).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn set_reserved_txos(
        &self,
        reserved_txos: Vec<DbReservedTxoActMod>,
    ) -> Result<(), Error> {
        block_on(ReservedTxo::insert_many(reserved_txos).exec(self.inner()))?;
        Ok(())
    }

    pub(crate) fn set_token(&self, token: DbTokenActMod) -> Result<i32, Error> {
        let res = block_on(Token::insert(token).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_token_media(&self, token_media: DbTokenMediaActMod) -> Result<i32, Error> {
        let res = block_on(TokenMedia::insert(token_media).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transport_endpoint(
        &self,
        transport_endpoint: DbTransportEndpointActMod,
    ) -> Result<i32, Error> {
        let res = block_on(TransportEndpoint::insert(transport_endpoint).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transfer(&self, transfer: DbTransferActMod) -> Result<i32, Error> {
        let res = block_on(Transfer::insert(transfer).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transfer_transport_endpoint(
        &self,
        transfer_transport_endpoint: DbTransferTransportEndpointActMod,
    ) -> Result<i32, Error> {
        let res = block_on(
            TransferTransportEndpoint::insert(transfer_transport_endpoint).exec(self.inner()),
        )?;
        Ok(res.last_insert_id)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn set_txo(&self, txo: DbTxoActMod) -> Result<i32, Error> {
        let mut on_conflict =
            sea_query::OnConflict::columns([txo::Column::Txid, txo::Column::Vout]);
        let mut update = false;
        if txo.exists.clone().unwrap() {
            update = true;
            // update exists only if updated value is true
            on_conflict.update_column(txo::Column::Exists);
        }
        if txo.btc_amount.clone().unwrap() != "0" {
            update = true;
            // update btc_amount only if updated value is positive
            on_conflict.update_column(txo::Column::BtcAmount);
        }
        if !update {
            on_conflict.do_nothing();
        }
        // this returns RecordNotInserted if the TXO already exists and on_conflict is do_nothing
        let conn = self.inner();
        let res = block_on(
            Txo::insert(txo.clone())
                .on_conflict(on_conflict.to_owned())
                .exec(conn),
        );
        let idx = match res {
            Ok(insert_result) => insert_result.last_insert_id,
            Err(DbErr::RecordNotInserted) => {
                // insert skipped due to ON CONFLICT DO NOTHING -> fetch existing record's idx
                let existing = self.get_txo(&Outpoint {
                    txid: txo.txid.unwrap(),
                    vout: txo.vout.unwrap(),
                })?;
                match existing {
                    Some(record) => record.idx,
                    None => unreachable!("RecordNotInserted means it already exists"),
                }
            }
            Err(err) => {
                return Err(err.into());
            }
        };
        Ok(idx)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn set_wallet_transaction(
        &self,
        wallet_transaction: DbWalletTransactionActMod,
    ) -> Result<i32, Error> {
        let res = block_on(WalletTransaction::insert(wallet_transaction).exec(self.inner()))?;
        Ok(res.last_insert_id)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn update_transfer(
        &self,
        transfer: &mut DbTransferActMod,
    ) -> Result<DbTransfer, Error> {
        Ok(block_on(
            Transfer::update(transfer.clone()).exec(self.inner()),
        )?)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn update_asset(&self, asset: &mut DbAssetActMod) -> Result<DbAsset, Error> {
        Ok(block_on(Asset::update(asset.clone()).exec(self.inner()))?)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn update_asset_transfer(
        &self,
        asset_transfer: &mut DbAssetTransferActMod,
    ) -> Result<DbAssetTransfer, Error> {
        Ok(block_on(
            AssetTransfer::update(asset_transfer.clone()).exec(self.inner()),
        )?)
    }

    pub(crate) fn update_backup_info(
        &self,
        backup_info: &mut DbBackupInfoActMod,
    ) -> Result<DbBackupInfo, Error> {
        Ok(block_on(
            BackupInfo::update(backup_info.clone()).exec(self.inner()),
        )?)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn update_batch_transfer(
        &self,
        batch_transfer: &mut DbBatchTransferActMod,
    ) -> Result<DbBatchTransfer, Error> {
        let now = now().unix_timestamp();
        batch_transfer.updated_at = ActiveValue::Set(now);
        Ok(block_on(
            BatchTransfer::update(batch_transfer.clone()).exec(self.inner()),
        )?)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn update_transfer_transport_endpoint(
        &self,
        transfer_transport_endpoint: &mut DbTransferTransportEndpointActMod,
    ) -> Result<DbTransferTransportEndpoint, Error> {
        Ok(block_on(
            TransferTransportEndpoint::update(transfer_transport_endpoint.clone())
                .exec(self.inner()),
        )?)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn update_txo(&self, txo: DbTxoActMod) -> Result<(), Error> {
        block_on(Txo::update(txo).exec(self.inner()))?;
        Ok(())
    }

    pub(crate) fn del_backup_info(&self) -> Result<(), Error> {
        block_on(BackupInfo::delete_many().exec(self.inner()))?;
        Ok(())
    }

    pub(crate) fn del_batch_transfer(&self, batch_transfer: &DbBatchTransfer) -> Result<(), Error> {
        block_on(BatchTransfer::delete_by_id(batch_transfer.idx).exec(self.inner()))?;
        Ok(())
    }

    pub(crate) fn del_coloring(&self, asset_transfer_idx: i32) -> Result<(), Error> {
        block_on(
            Coloring::delete_many()
                .filter(coloring::Column::AssetTransferIdx.eq(asset_transfer_idx))
                .exec(self.inner()),
        )?;
        Ok(())
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn del_pending_witness_script(&self, script: String) -> Result<(), Error> {
        block_on(
            PendingWitnessScript::delete_many()
                .filter(pending_witness_script::Column::Script.eq(script))
                .exec(self.inner()),
        )?;
        Ok(())
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn del_reserved_txos(&self, reserved_txos: &[DbReservedTxo]) -> Result<(), Error> {
        let idxs = reserved_txos.iter().map(|r| r.idx).collect::<Vec<_>>();
        block_on(
            ReservedTxo::delete_many()
                .filter(reserved_txo::Column::Idx.is_in(idxs))
                .exec(self.inner()),
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn del_transfer_transport_endpoint(&self, idx: i32) -> Result<(), Error> {
        block_on(transfer_transport_endpoint::Entity::delete_by_id(idx).exec(self.inner()))?;
        Ok(())
    }

    pub(crate) fn del_txo(&self, idx: i32) -> Result<(), Error> {
        block_on(Txo::delete_by_id(idx).exec(self.inner()))?;
        Ok(())
    }

    pub(crate) fn del_wallet_transaction(&self, idx: i32) -> Result<(), Error> {
        block_on(WalletTransaction::delete_by_id(idx).exec(self.inner()))?;
        Ok(())
    }

    pub(crate) fn get_asset(&self, asset_id: String) -> Result<Option<DbAsset>, Error> {
        Ok(block_on(
            Asset::find()
                .filter(asset::Column::Id.eq(asset_id))
                .one(self.inner()),
        )?)
    }

    pub(crate) fn get_backup_info(&self) -> Result<Option<DbBackupInfo>, Error> {
        Ok(block_on(BackupInfo::find().one(self.inner()))?)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn get_batch_transfer_by_txid(
        &self,
        txid: &str,
    ) -> Result<Option<DbBatchTransfer>, Error> {
        Ok(block_on(
            BatchTransfer::find()
                .filter(batch_transfer::Column::Txid.eq(txid))
                .one(self.inner()),
        )?)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn get_media(&self, media_idx: i32) -> Result<Option<DbMedia>, Error> {
        Ok(block_on(Media::find_by_id(media_idx).one(self.inner()))?)
    }

    pub(crate) fn get_media_by_digest(&self, digest: String) -> Result<Option<DbMedia>, Error> {
        Ok(block_on(
            Media::find()
                .filter(media::Column::Digest.eq(digest))
                .one(self.inner()),
        )?)
    }

    pub(crate) fn get_transport_endpoint(
        &self,
        endpoint: String,
    ) -> Result<Option<DbTransportEndpoint>, Error> {
        Ok(block_on(
            TransportEndpoint::find()
                .filter(transport_endpoint::Column::Endpoint.eq(endpoint))
                .one(self.inner()),
        )?)
    }

    pub(crate) fn get_txo(&self, outpoint: &Outpoint) -> Result<Option<DbTxo>, Error> {
        Ok(block_on(
            Txo::find()
                .filter(txo::Column::Txid.eq(outpoint.txid.clone()))
                .filter(txo::Column::Vout.eq(outpoint.vout))
                .one(self.inner()),
        )?)
    }

    pub(crate) fn get_wallet_transactions_by_idxs(
        &self,
        idxs: &[i32],
    ) -> Result<Vec<DbWalletTransaction>, Error> {
        Ok(block_on(
            WalletTransaction::find()
                .filter(wallet_transaction::Column::Idx.is_in(idxs.to_vec()))
                .all(self.inner()),
        )?)
    }

    pub(crate) fn get_wallet_transaction_with_reserved_txos_by_txid(
        &self,
        txid: &str,
    ) -> Result<Option<(DbWalletTransaction, Vec<DbReservedTxo>)>, Error> {
        Ok(block_on(
            WalletTransaction::find()
                .filter(wallet_transaction::Column::Txid.eq(txid))
                .find_with_related(ReservedTxo)
                .all(self.inner()),
        )?
        .into_iter()
        .next())
    }

    pub(crate) fn iter_assets(&self) -> Result<Vec<DbAsset>, Error> {
        Ok(block_on(Asset::find().all(self.inner()))?)
    }

    pub(crate) fn iter_asset_transfers(&self) -> Result<Vec<DbAssetTransfer>, Error> {
        Ok(block_on(AssetTransfer::find().all(self.inner()))?)
    }

    pub(crate) fn iter_batch_transfers(&self) -> Result<Vec<DbBatchTransfer>, Error> {
        Ok(block_on(BatchTransfer::find().all(self.inner()))?)
    }

    pub(crate) fn iter_colorings(&self) -> Result<Vec<DbColoring>, Error> {
        Ok(block_on(Coloring::find().all(self.inner()))?)
    }

    pub(crate) fn iter_media(&self) -> Result<Vec<DbMedia>, Error> {
        Ok(block_on(Media::find().all(self.inner()))?)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn iter_pending_witness_scripts(
        &self,
    ) -> Result<Vec<DbPendingWitnessScript>, Error> {
        Ok(block_on(PendingWitnessScript::find().all(self.inner()))?)
    }

    pub(crate) fn iter_reserved_txos(&self) -> Result<Vec<DbReservedTxo>, Error> {
        Ok(block_on(ReservedTxo::find().all(self.inner()))?)
    }

    pub(crate) fn iter_token_medias(&self) -> Result<Vec<DbTokenMedia>, Error> {
        Ok(block_on(TokenMedia::find().all(self.inner()))?)
    }

    pub(crate) fn iter_tokens(&self) -> Result<Vec<DbToken>, Error> {
        Ok(block_on(Token::find().all(self.inner()))?)
    }

    pub(crate) fn iter_transfers(&self) -> Result<Vec<DbTransfer>, Error> {
        Ok(block_on(Transfer::find().all(self.inner()))?)
    }

    pub(crate) fn iter_txos(&self) -> Result<Vec<DbTxo>, Error> {
        Ok(block_on(Txo::find().all(self.inner()))?)
    }

    pub(crate) fn iter_wallet_transactions(&self) -> Result<Vec<DbWalletTransaction>, Error> {
        Ok(block_on(WalletTransaction::find().all(self.inner()))?)
    }

    pub(crate) fn get_transfer_transport_endpoints_data(
        &self,
        transfer_idx: i32,
    ) -> Result<Vec<(DbTransferTransportEndpoint, DbTransportEndpoint)>, Error> {
        Ok(block_on(
            TransferTransportEndpoint::find()
                .filter(transfer_transport_endpoint::Column::TransferIdx.eq(transfer_idx))
                .find_also_related(TransportEndpoint)
                .order_by_asc(transfer_transport_endpoint::Column::Idx)
                .all(self.inner()),
        )?
        .into_iter()
        .map(|(tte, ce)| (tte, ce.expect("should be connected")))
        .collect())
    }

    pub(crate) fn get_db_data(&self, empty_transfers: bool) -> Result<DbData, Error> {
        let batch_transfers = self.iter_batch_transfers()?;
        let asset_transfers = self.iter_asset_transfers()?;
        let colorings = self.iter_colorings()?;
        let transfers = if empty_transfers {
            vec![]
        } else {
            self.iter_transfers()?
        };
        let txos = self.iter_txos()?;
        Ok(DbData {
            batch_transfers,
            asset_transfers,
            transfers,
            colorings,
            txos,
        })
    }

    pub(crate) fn get_unspent_txos(&self, txos: Vec<DbTxo>) -> Result<Vec<DbTxo>, Error> {
        let txos = if txos.is_empty() {
            self.iter_txos()?
        } else {
            txos
        };
        Ok(txos.into_iter().filter(|t| !t.spent).collect())
    }

    pub(crate) fn get_asset_balance(
        &self,
        asset_id: String,
        transfers: Option<Vec<DbTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
    ) -> Result<Balance, Error> {
        let batch_transfers = batch_transfers
            .map(Ok)
            .unwrap_or_else(|| self.iter_batch_transfers())?;
        let asset_transfers = asset_transfers
            .map(Ok)
            .unwrap_or_else(|| self.iter_asset_transfers())?;
        let transfers = transfers.map(Ok).unwrap_or_else(|| self.iter_transfers())?;
        let colorings = colorings.map(Ok).unwrap_or_else(|| self.iter_colorings())?;
        let txos = txos.map(Ok).unwrap_or_else(|| self.iter_txos())?;

        let txos_allocations = self.get_rgb_allocations(
            txos,
            Some(colorings),
            Some(batch_transfers.clone()),
            Some(asset_transfers.clone()),
            Some(transfers.clone()),
        )?;

        let mut allocations: Vec<LocalRgbAllocation> = vec![];
        txos_allocations
            .iter()
            .for_each(|u| allocations.extend(u.rgb_allocations.clone()));
        let ass_allocations: Vec<LocalRgbAllocation> = allocations
            .into_iter()
            .filter(|a| a.asset_id == Some(asset_id.clone()))
            .collect();

        let settled: u64 = ass_allocations
            .iter()
            .filter(|a| a.settled())
            .map(|a| a.assignment.main_amount())
            .sum();

        let mut ass_pending_incoming: u64 = ass_allocations
            .iter()
            .filter(|a| !a.txo_spent && a.incoming && a.status.pending())
            .map(|a| a.assignment.main_amount())
            .sum();
        let witness_pending: u64 = transfers
            .iter()
            .filter(|t| {
                t.incoming && matches!(t.recipient_type, Some(RecipientTypeFull::Witness { .. }))
            })
            .filter_map(|t| {
                let (at, bt) = t.related_transfers(&asset_transfers, &batch_transfers);
                if bt.status.waiting_confirmations() {
                    // filter for asset ID (always present in WaitingConfirmations status)
                    if at.asset_id.unwrap() != asset_id {
                        return None;
                    }
                    Some(
                        t.requested_assignment
                            .as_ref()
                            .map(|a| a.main_amount())
                            .unwrap_or(0),
                    )
                } else {
                    None
                }
            })
            .sum();
        ass_pending_incoming += witness_pending;
        let ass_pending_outgoing: u64 = ass_allocations
            .iter()
            .filter(|a| !a.incoming && a.status.pending())
            .map(|a| a.assignment.main_amount())
            .sum();
        let ass_pending: i128 = ass_pending_incoming as i128 - ass_pending_outgoing as i128;

        let future = settled as i128 + ass_pending;

        let unspendable: u64 = txos_allocations
            .into_iter()
            .filter(|u| {
                // unspent
                (!u.utxo.spent
                    // and with transfers either outgoing and not failed or incoming and pending
                    && (u.rgb_allocations.iter().any(|a| {
                        (!a.incoming && !a.status.failed()) || (a.incoming && a.status.pending())
                    })
                    // or with pending blinded incoming transfers
                    || u.pending_blinded > 0
                    ))
                    // spent
                    || (u.utxo.spent
                    // and with transfers outgoing and in WaitingConfirmations
                        && u.rgb_allocations
                            .iter()
                            .any(|a| !a.incoming && a.status.waiting_confirmations()))
            })
            .collect::<Vec<LocalUnspent>>()
            .iter()
            .map(|u| {
                u.rgb_allocations
                    .iter()
                    .filter(|a| a.asset_id == Some(asset_id.clone()) && a.settled())
                    .map(|a| a.assignment.main_amount())
                    .sum::<u64>()
            })
            .sum();

        let spendable = settled - unspendable;

        Ok(Balance {
            settled,
            future: future as u64,
            spendable,
        })
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn get_asset_ids(&self) -> Result<Vec<String>, Error> {
        Ok(self.iter_assets()?.into_iter().map(|a| a.id).collect())
    }

    pub(crate) fn check_asset_exists(&self, asset_id: String) -> Result<DbAsset, Error> {
        match self.get_asset(asset_id.clone())? {
            Some(a) => Ok(a),
            None => Err(Error::AssetNotFound { asset_id }),
        }
    }

    pub(crate) fn get_batch_transfer_or_fail(
        &self,
        idx: i32,
        batch_transfers: &[DbBatchTransfer],
    ) -> Result<DbBatchTransfer, Error> {
        if let Some(batch_transfer) = batch_transfers.iter().find(|t| t.idx == idx) {
            Ok(batch_transfer.clone())
        } else {
            Err(Error::BatchTransferNotFound { idx })
        }
    }

    pub(crate) fn get_or_insert_media(&self, digest: String, mime: String) -> Result<i32, Error> {
        Ok(match self.get_media_by_digest(digest.clone())? {
            Some(media) => media.idx,
            None => self.set_media(DbMediaActMod {
                digest: ActiveValue::Set(digest),
                mime: ActiveValue::Set(mime),
                ..Default::default()
            })?,
        })
    }

    pub(crate) fn get_rgb_allocations(
        &self,
        utxos: Vec<DbTxo>,
        colorings: Option<Vec<DbColoring>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        transfers: Option<Vec<DbTransfer>>,
    ) -> Result<Vec<LocalUnspent>, Error> {
        let batch_transfers = batch_transfers
            .map(Ok)
            .unwrap_or_else(|| self.iter_batch_transfers())?;
        let asset_transfers = asset_transfers
            .map(Ok)
            .unwrap_or_else(|| self.iter_asset_transfers())?;
        let colorings = colorings.map(Ok).unwrap_or_else(|| self.iter_colorings())?;
        let transfers = transfers.map(Ok).unwrap_or_else(|| self.iter_transfers())?;

        let pending_blinded_utxos = transfers
            .iter()
            .filter_map(|t| match (&t.recipient_type, t.incoming) {
                (Some(RecipientTypeFull::Blind { unblinded_utxo }), true) => {
                    let (_, bt) = t.related_transfers(&asset_transfers, &batch_transfers);
                    bt.status.waiting_counterparty().then_some(unblinded_utxo)
                }
                _ => None,
            })
            .fold(HashMap::new(), |mut acc, utxo| {
                *acc.entry(utxo).or_insert(0) += 1;
                acc
            });

        utxos
            .iter()
            .map(|t| {
                Ok(LocalUnspent {
                    utxo: t.clone(),
                    rgb_allocations: t.get_utxo_allocations(
                        &colorings,
                        &asset_transfers,
                        &batch_transfers,
                    )?,
                    pending_blinded: *pending_blinded_utxos.get(&t.outpoint()).unwrap_or(&0),
                })
            })
            .collect()
    }
}

pub(crate) mod enums;
