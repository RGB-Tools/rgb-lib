use bdk::LocalUtxo;
use bitcoin::OutPoint;
use futures::executor::block_on;
use sea_orm::entity::EntityTrait;
use sea_orm::{
    ActiveValue, ColumnTrait, DatabaseConnection, DeriveActiveEnum, EnumIter, IntoActiveValue,
    QueryFilter,
};
use std::str::FromStr;

use crate::error::InternalError;
use crate::utils::now;
use crate::wallet::{AssetType, Balance, Outpoint, TransferStatus};
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

impl DbAssetTransfer {
    pub(crate) fn asset_id(&self) -> Option<String> {
        let mut asset_id = self.asset_rgb20_id.clone();
        if asset_id.is_none() {
            asset_id = self.asset_rgb121_id.clone()
        };
        asset_id
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DbAssetTransferData {
    pub(crate) asset_transfer: DbAssetTransfer,
    pub(crate) transfers: Vec<DbTransfer>,
}

impl DbBatchTransfer {
    pub(crate) fn incoming(
        &self,
        asset_transfers: &[DbAssetTransfer],
        transfers: &[DbTransfer],
    ) -> Result<bool, Error> {
        let asset_transfer_ids: Vec<i64> = asset_transfers
            .iter()
            .filter(|t| t.batch_transfer_idx == self.idx)
            .map(|t| t.idx)
            .collect();
        Ok(transfers
            .iter()
            .filter(|t| asset_transfer_ids.contains(&t.asset_transfer_idx))
            .all(|t| t.blinding_secret.is_some()))
    }

    pub(crate) fn get_asset_transfers(
        &self,
        asset_transfers: &[DbAssetTransfer],
    ) -> Result<Vec<DbAssetTransfer>, InternalError> {
        Ok(asset_transfers
            .iter()
            .cloned()
            .filter(|t| t.batch_transfer_idx == self.idx)
            .collect())
    }

    pub(crate) fn get_transfers(
        &self,
        asset_transfers: &[DbAssetTransfer],
        transfers: &[DbTransfer],
    ) -> Result<DbBatchTransferData, InternalError> {
        let asset_transfers = self.get_asset_transfers(asset_transfers)?;
        let mut asset_transfers_data = vec![];
        for asset_transfer in asset_transfers {
            let transfers: Vec<DbTransfer> = transfers
                .iter()
                .cloned()
                .filter(|t| asset_transfer.idx == t.asset_transfer_idx)
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

    pub(crate) fn failed(&self) -> bool {
        self.status.failed()
    }

    pub(crate) fn pending(&self) -> bool {
        self.status.pending()
    }

    pub(crate) fn waiting_confirmations(&self) -> bool {
        self.status.waiting_confirmations()
    }

    pub(crate) fn waiting_counterparty(&self) -> bool {
        self.status.waiting_counterparty()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DbBatchTransferData {
    pub(crate) asset_transfers_data: Vec<DbAssetTransferData>,
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

pub(crate) struct DbData {
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
    ) -> Result<(DbAssetTransfer, DbBatchTransfer), InternalError> {
        let asset_transfer = asset_transfers
            .iter()
            .find(|t| t.idx == self.asset_transfer_idx)
            .expect("transfer should be connected to an asset transfer");
        let batch_transfer = batch_transfers
            .iter()
            .find(|t| t.idx == asset_transfer.batch_transfer_idx)
            .expect("asset transfer should be connected to a batch transfer");

        Ok((asset_transfer.clone(), batch_transfer.clone()))
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
    pub rgb_allocations: Vec<LocalRgbAllocation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LocalRgbAllocation {
    /// Asset ID
    pub asset_id: Option<String>,
    /// RGB amount
    pub amount: u64,
    /// Defines the allocation status
    pub status: TransferStatus,
    /// Defines if the allocation is incoming
    pub incoming: bool,
    /// Defines if the allocation is on a spent TXO
    pub txo_spent: bool,
}

impl LocalRgbAllocation {
    pub(crate) fn settled(&self) -> bool {
        !self.status.failed()
            && ((!self.txo_spent && self.incoming && self.status.settled())
                || (self.txo_spent && !self.incoming && self.status.waiting_confirmations()))
    }

    pub(crate) fn future(&self) -> bool {
        !self.txo_spent && self.incoming && !self.status.failed() && !self.settled()
    }
}

#[derive(Debug)]
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
        asset_transfers: Vec<DbAssetTransfer>,
    ) -> Vec<DbAssetTransfer> {
        asset_transfers
            .into_iter()
            .filter(|t| t.asset_id() == Some(asset_id.clone()))
            .collect()
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

    pub(crate) fn get_db_data(&self, empty_transfers: bool) -> Result<DbData, InternalError> {
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

    pub(crate) fn get_unspent_txos(&self, txos: Vec<DbTxo>) -> Result<Vec<DbTxo>, InternalError> {
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
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        colorings: Option<Vec<DbColoring>>,
        txos: Option<Vec<DbTxo>>,
    ) -> Result<Balance, Error> {
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
        let txos = if let Some(t) = txos {
            t
        } else {
            self.iter_txos()?
        };

        let txos_allocations = self.get_rgb_allocations(
            txos,
            Some(colorings),
            Some(batch_transfers),
            Some(asset_transfers),
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
            .map(|a| a.amount)
            .sum();

        let ass_pending_incoming: u64 = ass_allocations
            .iter()
            .filter(|a| !a.txo_spent && a.incoming && a.status.pending())
            .map(|a| a.amount)
            .sum();
        let ass_pending_outgoing: u64 = ass_allocations
            .iter()
            .filter(|a| !a.incoming && a.status.pending())
            .map(|a| a.amount)
            .sum();
        let ass_pending: i128 = ass_pending_incoming as i128 - ass_pending_outgoing as i128;

        let future = settled as i128 + ass_pending;

        let unspendable: u64 = txos_allocations
            .into_iter()
            .filter(|u| {
                (!u.utxo.spent
                    && u.rgb_allocations.iter().any(|a| {
                        (!a.incoming && !a.status.failed()) || (a.incoming && a.status.pending())
                    }))
                    || (u.utxo.spent
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
                    .map(|a| a.amount)
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
        batch_transfers: &[DbBatchTransfer],
    ) -> Result<DbBatchTransfer, Error> {
        if let Some(batch_transfer) = batch_transfers
            .iter()
            .find(|t| t.txid == Some(txid.clone()))
        {
            Ok(batch_transfer.clone())
        } else {
            Err(Error::BatchTransferNotFound(txid))
        }
    }

    pub(crate) fn get_transfer_or_fail(
        &self,
        blinded_utxo: String,
        transfers: &[DbTransfer],
    ) -> Result<DbTransfer, Error> {
        if let Some(transfer) = transfers
            .iter()
            .find(|t| t.blinded_utxo == Some(blinded_utxo.clone()))
        {
            Ok(transfer.clone())
        } else {
            Err(Error::TransferNotFound(blinded_utxo))
        }
    }

    pub(crate) fn get_incoming_transfer(
        &self,
        batch_transfer_data: &DbBatchTransferData,
    ) -> Result<(DbAssetTransfer, DbTransfer), Error> {
        let asset_transfer_data = batch_transfer_data
            .asset_transfers_data
            .first()
            .expect("asset transfer should be connected to a batch transfer");
        let transfer = asset_transfer_data
            .transfers
            .first()
            .expect("transfer should be connected to an asset transfer");
        Ok((asset_transfer_data.asset_transfer.clone(), transfer.clone()))
    }

    pub(crate) fn get_transfer_data(
        &self,
        asset_transfer: &DbAssetTransfer,
        batch_transfer: &DbBatchTransfer,
        txos: &[DbTxo],
        colorings: &[DbColoring],
    ) -> Result<TransferData, Error> {
        let filtered_coloring = colorings
            .iter()
            .cloned()
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
        let transfer_txos: Vec<DbTxo> = txos
            .iter()
            .cloned()
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
        colorings: Vec<DbColoring>,
        asset_transfers: Vec<DbAssetTransfer>,
        batch_transfers: Vec<DbBatchTransfer>,
    ) -> Result<Vec<LocalRgbAllocation>, Error> {
        let utxo_colorings: Vec<&DbColoring> =
            colorings.iter().filter(|c| c.txo_idx == utxo.idx).collect();

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

            let coloring_amount = c
                .amount
                .parse::<u64>()
                .expect("DB should contain a valid u64 value");

            allocations.push(LocalRgbAllocation {
                asset_id: asset_transfer.asset_id(),
                amount: coloring_amount,
                status: batch_transfer.status,
                incoming: c.incoming(),
                txo_spent: utxo.spent,
            });
        });

        Ok(allocations)
    }

    pub(crate) fn get_rgb_allocations(
        &self,
        utxos: Vec<DbTxo>,
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
                        colorings.clone(),
                        asset_transfers.clone(),
                        batch_transfers.clone(),
                    )?,
                })
            })
            .collect()
    }
}
