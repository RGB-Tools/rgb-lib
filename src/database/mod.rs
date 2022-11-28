use bdk::LocalUtxo;
use bitcoin::OutPoint;
use futures::executor::block_on;
use sea_orm::entity::EntityTrait;
use sea_orm::{
    ActiveValue, ColumnTrait, DatabaseConnection, DeriveActiveEnum, EnumIter, IntoActiveValue,
    ModelTrait, QueryFilter,
};
use sea_query::query::Condition;
use std::str::FromStr;
use std::sync::Arc;

use crate::error::InternalError;
use crate::utils::now;
use crate::wallet::{AssetType, Balance, Outpoint, RgbAllocation, TransferStatus};
use crate::Error;

pub(crate) mod entities;

use crate::database::entities::asset_transfer::{
    ActiveModel as DbAssetTransferActMod, Model as DbAssetTransfer,
};
use crate::database::entities::batch_transfer::{
    ActiveModel as DbBatchTransferActMod, Model as DbBatchTransfer,
};
use entities::asset_rgb121::{ActiveModel as DbAssetRgb121ActMod, Model as DbAssetRgb121};
use entities::asset_rgb20::{ActiveModel as DbAssetRgb20ActMod, Model as DbAssetRgb20};
use entities::coloring::{ActiveModel as DbColoringActMod, Model as DbColoring};
use entities::transfer::{ActiveModel as DbTransferActMod, Model as DbTransfer};
use entities::txo::{ActiveModel as DbTxoActMod, Model as DbTxo};
use entities::{
    asset_rgb121, asset_rgb20, asset_transfer, batch_transfer, coloring, transfer, txo,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum ColoringType {
    #[sea_orm(num_value = 1)]
    Blind = 1,
    #[sea_orm(num_value = 2)]
    Issue = 2,
    #[sea_orm(num_value = 3)]
    Input = 3,
    #[sea_orm(num_value = 4)]
    Change = 4,
}

impl IntoActiveValue<ColoringType> for ColoringType {
    fn into_active_value(self) -> ActiveValue<ColoringType> {
        ActiveValue::Set(self)
    }
}

impl DbBatchTransfer {
    pub(crate) fn incoming(&self, database: Arc<RgbLibDatabase>) -> Result<bool, Error> {
        let asset_transfer_ids: Vec<i64> = database
            .iter_asset_transfers()?
            .into_iter()
            .filter(|t| t.batch_transfer_idx == self.idx)
            .map(|t| t.idx)
            .collect();
        Ok(database
            .iter_transfers()?
            .into_iter()
            .filter(|t| asset_transfer_ids.contains(&t.asset_transfer_idx))
            .all(|t| t.blinding_secret.is_some()))
    }

    pub(crate) fn failed(&self) -> bool {
        self.status == TransferStatus::Failed
    }

    pub(crate) fn pending(&self) -> bool {
        vec![
            TransferStatus::WaitingCounterparty,
            TransferStatus::WaitingConfirmations,
        ]
        .contains(&self.status)
    }

    pub(crate) fn settled(&self) -> bool {
        self.status == TransferStatus::Settled
    }

    pub(crate) fn waiting_confirmations(&self) -> bool {
        self.status == TransferStatus::WaitingConfirmations
    }

    pub(crate) fn waiting_counterparty(&self) -> bool {
        self.status == TransferStatus::WaitingCounterparty
    }
}

impl DbColoring {
    pub(crate) fn incoming(&self) -> bool {
        vec![
            ColoringType::Blind,
            ColoringType::Change,
            ColoringType::Issue,
        ]
        .contains(&self.coloring_type)
    }
}

impl DbTransfer {
    pub(crate) fn related_transfers(
        &self,
        database: Arc<RgbLibDatabase>,
    ) -> Result<(DbAssetTransfer, DbBatchTransfer), InternalError> {
        let db_conn = database.get_connection();
        let asset_transfer = block_on(self.find_related(asset_transfer::Entity).one(db_conn))?
            .expect("transfer should be connected to an asset transfer");
        let batch_transfer = block_on(
            asset_transfer
                .find_related(batch_transfer::Entity)
                .one(db_conn),
        )?
        .expect("asset transfer should be connected to a batch transfer");
        Ok((asset_transfer, batch_transfer))
    }
}

impl DbTxo {
    pub(crate) fn outpoint(&self) -> Outpoint {
        Outpoint {
            txid: self.txid.to_string(),
            vout: self.vout,
        }
    }
}

impl From<DbTxo> for OutPoint {
    fn from(x: DbTxo) -> OutPoint {
        OutPoint::from_str(&x.outpoint().to_string()).expect("DB should contain a valid outpoint")
    }
}

impl From<LocalUtxo> for DbTxoActMod {
    fn from(x: LocalUtxo) -> DbTxoActMod {
        DbTxoActMod {
            idx: ActiveValue::NotSet,
            txid: ActiveValue::Set(x.outpoint.txid.to_string()),
            vout: ActiveValue::Set(x.outpoint.vout),
            btc_amount: ActiveValue::Set(x.txout.value.to_string()),
            colorable: ActiveValue::Set(false),
            spent: ActiveValue::Set(false),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalUnspent {
    /// Database UTXO
    pub utxo: DbTxo,
    /// RGB allocations on the UTXO
    pub rgb_allocations: Vec<RgbAllocation>,
}

pub(crate) struct TransferData {
    pub(crate) incoming: bool,
    pub(crate) status: TransferStatus,
    pub(crate) txid: Option<String>,
    pub(crate) unblinded_utxo: Option<Outpoint>,
    pub(crate) change_utxo: Option<Outpoint>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) expiration: Option<i64>,
}

pub struct RgbLibDatabase {
    connection: DatabaseConnection,
}

impl RgbLibDatabase {
    pub(crate) fn new(connection: DatabaseConnection) -> Self {
        Self { connection }
    }

    pub(crate) fn get_connection(&self) -> &DatabaseConnection {
        &self.connection
    }

    pub(crate) fn set_asset_rgb20(&self, asset_rgb20: DbAssetRgb20) -> Result<i64, InternalError> {
        let mut asset: DbAssetRgb20ActMod = asset_rgb20.into();
        asset.idx = ActiveValue::NotSet;
        let res = block_on(asset_rgb20::Entity::insert(asset).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_asset_rgb121(
        &self,
        asset_rgb121: DbAssetRgb121,
    ) -> Result<i64, InternalError> {
        let mut asset: DbAssetRgb121ActMod = asset_rgb121.into();
        asset.idx = ActiveValue::NotSet;
        let res = block_on(asset_rgb121::Entity::insert(asset).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_asset_transfer(
        &self,
        asset_transfer: DbAssetTransferActMod,
    ) -> Result<i64, InternalError> {
        let res =
            block_on(asset_transfer::Entity::insert(asset_transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_batch_transfer(
        &self,
        batch_transfer: DbBatchTransferActMod,
    ) -> Result<i64, InternalError> {
        let now = now().unix_timestamp();
        let mut batch_transfer = batch_transfer;
        batch_transfer.created_at = ActiveValue::Set(now);
        batch_transfer.updated_at = ActiveValue::Set(now);
        let res =
            block_on(batch_transfer::Entity::insert(batch_transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_coloring(&self, coloring: DbColoringActMod) -> Result<i64, InternalError> {
        let res = block_on(coloring::Entity::insert(coloring).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transfer(&self, transfer: DbTransferActMod) -> Result<i64, InternalError> {
        let res = block_on(transfer::Entity::insert(transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_txo(&self, txo: DbTxoActMod) -> Result<i64, InternalError> {
        let res = block_on(txo::Entity::insert(txo).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn update_coloring(&self, coloring: DbColoringActMod) -> Result<(), InternalError> {
        block_on(coloring::Entity::update(coloring).exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn update_transfer(
        &self,
        transfer: &mut DbTransferActMod,
    ) -> Result<DbTransfer, InternalError> {
        Ok(block_on(
            transfer::Entity::update(transfer.clone()).exec(self.get_connection()),
        )?)
    }

    pub(crate) fn update_asset_transfer(
        &self,
        asset_transfer: &mut DbAssetTransferActMod,
    ) -> Result<DbAssetTransfer, InternalError> {
        Ok(block_on(
            asset_transfer::Entity::update(asset_transfer.clone()).exec(self.get_connection()),
        )?)
    }

    pub(crate) fn update_batch_transfer(
        &self,
        batch_transfer: &mut DbBatchTransferActMod,
    ) -> Result<DbBatchTransfer, InternalError> {
        let now = now().unix_timestamp();
        batch_transfer.updated_at = ActiveValue::Set(now);
        batch_transfer.updated_at = ActiveValue::Set(now);
        Ok(block_on(
            batch_transfer::Entity::update(batch_transfer.clone()).exec(self.get_connection()),
        )?)
    }

    pub(crate) fn update_txo(&self, txo: DbTxoActMod) -> Result<(), InternalError> {
        block_on(txo::Entity::update(txo).exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn del_batch_transfer(
        &self,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<(), InternalError> {
        block_on(transfer::Entity::delete_by_id(batch_transfer.idx).exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn del_coloring(&self, asset_transfer_idx: i64) -> Result<(), InternalError> {
        block_on(
            coloring::Entity::delete_many()
                .filter(coloring::Column::AssetTransferIdx.eq(asset_transfer_idx))
                .exec(self.get_connection()),
        )?;
        Ok(())
    }

    pub(crate) fn get_batch_transfer(
        &self,
        txid: String,
    ) -> Result<Option<DbBatchTransfer>, InternalError> {
        Ok(block_on(
            batch_transfer::Entity::find()
                .filter(batch_transfer::Column::Txid.eq(txid))
                .one(self.get_connection()),
        )?)
    }

    pub(crate) fn get_transfer(
        &self,
        blinded_utxo: String,
    ) -> Result<Option<DbTransfer>, InternalError> {
        Ok(block_on(
            transfer::Entity::find()
                .filter(transfer::Column::BlindedUtxo.eq(blinded_utxo))
                .one(self.get_connection()),
        )?)
    }

    pub(crate) fn get_txo(&self, outpoint: Outpoint) -> Result<Option<DbTxo>, InternalError> {
        Ok(block_on(
            txo::Entity::find()
                .filter(txo::Column::Txid.eq(outpoint.txid))
                .filter(txo::Column::Vout.eq(outpoint.vout))
                .one(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_assets_rgb20(&self) -> Result<Vec<DbAssetRgb20>, InternalError> {
        Ok(block_on(
            asset_rgb20::Entity::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_assets_rgb121(&self) -> Result<Vec<DbAssetRgb121>, InternalError> {
        Ok(block_on(
            asset_rgb121::Entity::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_asset_transfers(&self) -> Result<Vec<DbAssetTransfer>, InternalError> {
        Ok(block_on(
            asset_transfer::Entity::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_asset_asset_transfers(
        &self,
        asset_id: String,
    ) -> Result<Vec<DbAssetTransfer>, InternalError> {
        Ok(block_on(
            asset_transfer::Entity::find()
                .filter(
                    Condition::any()
                        .add(asset_transfer::Column::AssetRgb20Id.eq(asset_id.clone()))
                        .add(asset_transfer::Column::AssetRgb121Id.eq(asset_id)),
                )
                .all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_batch_asset_transfers(
        &self,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<Vec<DbAssetTransfer>, InternalError> {
        Ok(block_on(
            batch_transfer
                .find_related(asset_transfer::Entity)
                .all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_batch_transfers(&self) -> Result<Vec<DbBatchTransfer>, InternalError> {
        Ok(block_on(
            batch_transfer::Entity::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_colorings(&self) -> Result<Vec<DbColoring>, InternalError> {
        Ok(block_on(
            coloring::Entity::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_transfers(&self) -> Result<Vec<DbTransfer>, InternalError> {
        Ok(block_on(
            transfer::Entity::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_txos(&self) -> Result<Vec<DbTxo>, InternalError> {
        Ok(block_on(txo::Entity::find().all(self.get_connection()))?)
    }

    pub(crate) fn get_unspent_txos(&self) -> Result<Vec<DbTxo>, InternalError> {
        Ok(self.iter_txos()?.into_iter().filter(|t| !t.spent).collect())
    }

    pub(crate) fn get_unspendable_utxo_ids(
        &self,
        utxo_ids: Vec<i64>,
        pending_batch_transfer_ids: Vec<i64>,
        failed_batch_transfer_ids: Vec<i64>,
        colorings: Vec<DbColoring>,
    ) -> Result<Vec<i64>, Error> {
        let failed_asset_transfer_ids: Vec<i64> = self
            .iter_asset_transfers()?
            .into_iter()
            .filter(|t| failed_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let pending_asset_transfer_ids: Vec<i64> = self
            .iter_asset_transfers()?
            .into_iter()
            .filter(|t| pending_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let mut unspendable_txo_ids: Vec<i64> = colorings
            .into_iter()
            .filter(|c| utxo_ids.contains(&c.txo_idx))
            .filter(|c| {
                (!c.incoming() && !failed_asset_transfer_ids.contains(&c.asset_transfer_idx))
                    || (c.incoming() && pending_asset_transfer_ids.contains(&c.asset_transfer_idx))
            })
            .map(|c| c.txo_idx)
            .collect();

        unspendable_txo_ids.sort();
        unspendable_txo_ids.dedup();

        Ok(unspendable_txo_ids)
    }

    pub(crate) fn get_asset_balance(&self, asset_id: String) -> Result<Balance, Error> {
        let asset_transfers = self.iter_asset_asset_transfers(asset_id)?;

        let batch_transfers = self.iter_batch_transfers()?;
        let colorings = self.iter_colorings()?;
        let txos = self.iter_txos()?;

        let pending_batch_transfer_ids: Vec<i64> = batch_transfers
            .clone()
            .into_iter()
            .filter(|t| t.pending())
            .map(|t| t.idx)
            .collect();
        let settled_batch_transfer_ids: Vec<i64> = batch_transfers
            .clone()
            .into_iter()
            .filter(|t| t.settled())
            .map(|t| t.idx)
            .collect();
        let waiting_confs_batch_transfer_ids: Vec<i64> = batch_transfers
            .clone()
            .into_iter()
            .filter(|t| t.waiting_confirmations())
            .map(|t| t.idx)
            .collect();

        let ass_pending_transfer_ids: Vec<i64> = asset_transfers
            .clone()
            .into_iter()
            .filter(|t| pending_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let ass_pending_colorings: Vec<DbColoring> = colorings
            .clone()
            .into_iter()
            .filter(|c| ass_pending_transfer_ids.contains(&c.asset_transfer_idx))
            .collect();
        let ass_pending_incoming: u64 = ass_pending_colorings
            .clone()
            .into_iter()
            .filter(|c| c.incoming())
            .map(|c| {
                c.amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value")
            })
            .sum();
        let ass_pending_outgoing: u64 = ass_pending_colorings
            .into_iter()
            .filter(|c| !c.incoming())
            .map(|c| {
                c.amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value")
            })
            .sum();

        let ass_settled_transfer_ids: Vec<i64> = asset_transfers
            .clone()
            .into_iter()
            .filter(|t| settled_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let unspent_txo_ids: Vec<i64> = txos
            .clone()
            .into_iter()
            .filter(|t| !t.spent)
            .map(|u| u.idx)
            .collect();
        let ass_waiting_confs_transfer_ids: Vec<i64> = asset_transfers
            .into_iter()
            .filter(|t| waiting_confs_batch_transfer_ids.contains(&t.batch_transfer_idx))
            .map(|t| t.idx)
            .collect();
        let spent_txos_ids: Vec<i64> = txos
            .into_iter()
            .filter(|t| t.spent)
            .map(|u| u.idx)
            .collect();
        let settled_colorings = colorings.clone().into_iter().filter(|c| {
            (ass_settled_transfer_ids.contains(&c.asset_transfer_idx)
                && unspent_txo_ids.contains(&c.txo_idx))
                || (ass_waiting_confs_transfer_ids.contains(&c.asset_transfer_idx)
                    && spent_txos_ids.contains(&c.txo_idx))
        });
        let settled: u64 = settled_colorings
            .clone()
            .map(|c| {
                c.amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value")
            })
            .sum();

        let ass_pending: i128 = ass_pending_incoming as i128 - ass_pending_outgoing as i128;
        let future = settled as i128 + ass_pending;

        let failed_batch_transfer_ids: Vec<i64> = batch_transfers
            .into_iter()
            .filter(|t| t.failed())
            .map(|t| t.idx)
            .collect();
        let unspendable_utxo_ids = self.get_unspendable_utxo_ids(
            unspent_txo_ids.clone(),
            pending_batch_transfer_ids,
            failed_batch_transfer_ids,
            colorings,
        )?;
        let global_pending: u64 = settled_colorings
            .filter(|c| {
                unspendable_utxo_ids.contains(&c.txo_idx)
                    || (ass_waiting_confs_transfer_ids.contains(&c.asset_transfer_idx)
                        && spent_txos_ids.contains(&c.txo_idx))
            })
            .map(|c| {
                c.amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value")
            })
            .sum();

        let spendable = settled - global_pending;

        Ok(Balance {
            settled,
            future: future as u64,
            spendable,
        })
    }

    pub(crate) fn get_asset_ids(&self) -> Result<Vec<String>, InternalError> {
        Ok(self
            .iter_assets_rgb20()?
            .iter()
            .map(|a| a.asset_id.clone())
            .chain(
                self.iter_assets_rgb121()?
                    .iter()
                    .map(|a| a.asset_id.clone()),
            )
            .collect())
    }

    pub(crate) fn get_asset_or_fail(&self, asset_id: String) -> Result<AssetType, Error> {
        if block_on(
            asset_rgb20::Entity::find()
                .filter(asset_rgb20::Column::AssetId.eq(asset_id.clone()))
                .one(self.get_connection()),
        )
        .map_err(InternalError::from)?
        .is_some()
        {
            Ok(AssetType::Rgb20)
        } else if block_on(
            asset_rgb121::Entity::find()
                .filter(asset_rgb121::Column::AssetId.eq(asset_id.clone()))
                .one(self.get_connection()),
        )
        .map_err(InternalError::from)?
        .is_some()
        {
            Ok(AssetType::Rgb121)
        } else {
            Err(Error::AssetNotFound(asset_id))
        }
    }

    pub(crate) fn get_batch_transfer_or_fail(
        &self,
        txid: String,
    ) -> Result<DbBatchTransfer, Error> {
        if let Some(batch_transfer) = self.get_batch_transfer(txid.clone())? {
            Ok(batch_transfer)
        } else {
            Err(Error::BatchTransferNotFound(txid))
        }
    }

    pub(crate) fn get_transfer_or_fail(&self, blinded_utxo: String) -> Result<DbTransfer, Error> {
        if let Some(transfer) = self.get_transfer(blinded_utxo.clone())? {
            Ok(transfer)
        } else {
            Err(Error::TransferNotFound(blinded_utxo))
        }
    }

    pub(crate) fn get_incoming_transfer(
        &self,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<(DbAssetTransfer, DbTransfer), Error> {
        let asset_transfers: Vec<DbAssetTransfer> =
            self.iter_batch_asset_transfers(batch_transfer)?;
        let asset_transfer = asset_transfers
            .first()
            .expect("asset transfer should be connected to a batch transfer");
        let transfers: Vec<DbTransfer> = self
            .iter_transfers()?
            .into_iter()
            .filter(|t| t.asset_transfer_idx == asset_transfer.idx)
            .collect();
        let transfer = transfers
            .first()
            .expect("transfer should be connected to an asset transfer");
        Ok((asset_transfer.clone(), transfer.clone()))
    }

    pub(crate) fn get_transfer_data(&self, transfer: &DbTransfer) -> Result<TransferData, Error> {
        let asset_transfers: Vec<DbAssetTransfer> = self
            .iter_asset_transfers()?
            .into_iter()
            .filter(|t| t.idx == transfer.asset_transfer_idx)
            .collect();
        let asset_transfer = asset_transfers
            .first()
            .expect("transfer should be connected to an asset transfer");
        let batch_transfers: Vec<DbBatchTransfer> = self
            .iter_batch_transfers()?
            .into_iter()
            .filter(|t| t.idx == asset_transfer.batch_transfer_idx)
            .collect();
        let batch_transfer = batch_transfers
            .first()
            .expect("asset transfer should be connected to a batch transfer");

        let filtered_coloring = self
            .iter_colorings()?
            .into_iter()
            .filter(|c| c.asset_transfer_idx == asset_transfer.idx);

        let received: u64 = filtered_coloring
            .clone()
            .filter(|c| c.incoming())
            .map(|c| {
                c.amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value")
            })
            .sum();

        let sent: u64 = filtered_coloring
            .clone()
            .filter(|c| !c.incoming())
            .map(|c| {
                c.amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value")
            })
            .sum();

        let incoming = if received == 0 && sent == 0 {
            true
        } else {
            received > sent
        };
        let txo_ids: Vec<i64> = filtered_coloring.clone().map(|c| c.txo_idx).collect();
        let transfer_txos: Vec<DbTxo> = self
            .iter_txos()?
            .into_iter()
            .filter(|t| txo_ids.contains(&t.idx))
            .collect();

        let blinded_txo_idx: Vec<i64> = filtered_coloring
            .clone()
            .filter(|c| c.coloring_type == ColoringType::Blind)
            .map(|c| c.txo_idx)
            .collect();
        let unblinded_utxo = transfer_txos
            .clone()
            .into_iter()
            .filter(|t| blinded_txo_idx.contains(&t.idx))
            .map(|t| t.outpoint())
            .collect::<Vec<Outpoint>>()
            .first()
            .cloned();

        let change_txo_idx: Vec<i64> = filtered_coloring
            .filter(|c| c.coloring_type == ColoringType::Change)
            .map(|c| c.txo_idx)
            .collect();
        let change_utxo = transfer_txos
            .into_iter()
            .filter(|t| change_txo_idx.contains(&t.idx))
            .map(|t| t.outpoint())
            .collect::<Vec<Outpoint>>()
            .first()
            .cloned();

        Ok(TransferData {
            incoming,
            status: batch_transfer.status,
            txid: batch_transfer.txid.clone(),
            unblinded_utxo,
            change_utxo,
            created_at: batch_transfer.created_at,
            updated_at: batch_transfer.updated_at,
            expiration: batch_transfer.expiration,
        })
    }

    fn _get_utxo_allocations(
        &self,
        utxo: &DbTxo,
        settled_only: bool,
        colorings: Vec<DbColoring>,
        asset_transfers: Vec<DbAssetTransfer>,
        batch_transfers: Vec<DbBatchTransfer>,
    ) -> Result<Vec<RgbAllocation>, Error> {
        let utxo_colorings: Vec<&DbColoring> =
            colorings.iter().filter(|c| c.txo_idx == utxo.idx).collect();

        let mut allocations: Vec<RgbAllocation> = vec![];
        utxo_colorings.iter().for_each(|c| {
            let asset_transfer: &DbAssetTransfer = asset_transfers
                .iter()
                .filter(|t| t.idx == c.asset_transfer_idx)
                .collect::<Vec<&DbAssetTransfer>>()
                .first()
                .expect("coloring should be connected to an asset transfer");
            let batch_transfer: &DbBatchTransfer = batch_transfers
                .iter()
                .filter(|t| asset_transfer.batch_transfer_idx == t.idx)
                .collect::<Vec<&DbBatchTransfer>>()
                .first()
                .expect("asset transfer should be connected to a batch transfer");

            if (batch_transfer.status == TransferStatus::Settled && !utxo.spent && c.incoming())
                || (batch_transfer.status == TransferStatus::WaitingConfirmations
                    && utxo.spent
                    && !c.incoming())
            {
                let coloring_amount = c
                    .amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value");
                let mut asset_id = asset_transfer.asset_rgb20_id.clone();
                if asset_id.is_none() {
                    asset_id = asset_transfer.asset_rgb121_id.clone()
                };
                allocations.push(RgbAllocation {
                    asset_id,
                    amount: coloring_amount,
                    settled: true,
                });
            }

            if settled_only {
                return;
            }

            if batch_transfer.pending() && !utxo.spent && c.incoming() {
                let coloring_amount = c
                    .amount
                    .parse::<u64>()
                    .expect("DB should contain a valid u64 value");
                let mut asset_id = asset_transfer.asset_rgb20_id.clone();
                if asset_id.is_none() {
                    asset_id = asset_transfer.asset_rgb121_id.clone()
                };
                allocations.push(RgbAllocation {
                    asset_id,
                    amount: coloring_amount,
                    settled: false,
                });
            }
        });

        Ok(allocations)
    }

    pub(crate) fn get_rgb_allocations(
        &self,
        utxos: Vec<DbTxo>,
        settled_only: bool,
        colorings: Option<Vec<DbColoring>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
    ) -> Result<Vec<LocalUnspent>, Error> {
        let batch_transfers = if let Some(bt) = batch_transfers {
            bt
        } else {
            self.iter_batch_transfers()?
        };
        let asset_transfers = if let Some(at) = asset_transfers {
            at
        } else {
            self.iter_asset_transfers()?
        };
        let colorings = if let Some(cs) = colorings {
            cs
        } else {
            self.iter_colorings()?
        };
        utxos
            .iter()
            .map(|t| {
                Ok(LocalUnspent {
                    utxo: t.clone(),
                    rgb_allocations: self._get_utxo_allocations(
                        t,
                        settled_only,
                        colorings.clone(),
                        asset_transfers.clone(),
                        batch_transfers.clone(),
                    )?,
                })
            })
            .collect()
    }
}
