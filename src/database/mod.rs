use bdk::bitcoin::OutPoint as BdkOutPoint;
use bdk::LocalUtxo;
use futures::executor::block_on;
use sea_orm::{ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::error::InternalError;
use crate::utils::now;
use crate::wallet::{Balance, Outpoint, RecipientData, TransferKind};
use crate::Error;

pub(crate) mod entities;

use crate::database::entities::asset_transfer::{
    ActiveModel as DbAssetTransferActMod, Model as DbAssetTransfer,
};
use crate::database::entities::backup_info::{
    ActiveModel as DbBackupInfoActMod, Model as DbBackupInfo,
};
use crate::database::entities::batch_transfer::{
    ActiveModel as DbBatchTransferActMod, Model as DbBatchTransfer,
};
use entities::asset::{ActiveModel as DbAssetActMod, Model as DbAsset};
use entities::coloring::{ActiveModel as DbColoringActMod, Model as DbColoring};
use entities::transfer::{ActiveModel as DbTransferActMod, Model as DbTransfer};
use entities::transfer_transport_endpoint::{
    ActiveModel as DbTransferTransportEndpointActMod, Model as DbTransferTransportEndpoint,
};
use entities::transport_endpoint::{
    ActiveModel as DbTransportEndpointActMod, Model as DbTransportEndpoint,
};
use entities::txo::{ActiveModel as DbTxoActMod, Model as DbTxo};
use entities::wallet_transaction::{
    ActiveModel as DbWalletTransactionActMod, Model as DbWalletTransaction,
};
use entities::{
    asset, asset_transfer, backup_info, batch_transfer, coloring, transfer,
    transfer_transport_endpoint, transport_endpoint, txo, wallet_transaction,
};

use self::enums::{ColoringType, RecipientType, TransferStatus, TransportType};

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
        let asset_transfer_ids: Vec<i32> = asset_transfers
            .iter()
            .filter(|t| t.batch_transfer_idx == self.idx)
            .map(|t| t.idx)
            .collect();
        Ok(transfers
            .iter()
            .filter(|t| asset_transfer_ids.contains(&t.asset_transfer_idx))
            .all(|t| t.incoming))
    }

    pub(crate) fn get_asset_transfers(
        &self,
        asset_transfers: &[DbAssetTransfer],
    ) -> Result<Vec<DbAssetTransfer>, InternalError> {
        Ok(asset_transfers
            .iter()
            .filter(|&t| t.batch_transfer_idx == self.idx)
            .cloned()
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
        [
            ColoringType::Receive,
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

impl From<DbTxo> for BdkOutPoint {
    fn from(x: DbTxo) -> BdkOutPoint {
        BdkOutPoint::from_str(&x.outpoint().to_string())
            .expect("DB should contain a valid outpoint")
    }
}

impl From<LocalUtxo> for DbTxoActMod {
    fn from(x: LocalUtxo) -> DbTxoActMod {
        DbTxoActMod {
            idx: ActiveValue::NotSet,
            txid: ActiveValue::Set(x.outpoint.txid.to_string()),
            vout: ActiveValue::Set(x.outpoint.vout),
            btc_amount: ActiveValue::Set(x.txout.value.to_string()),
            spent: ActiveValue::Set(false),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LocalTransportEndpoint {
    pub transport_type: TransportType,
    pub endpoint: String,
    pub used: bool,
    pub usable: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct LocalUnspent {
    /// Database UTXO
    pub utxo: DbTxo,
    /// RGB allocations on the UTXO
    pub rgb_allocations: Vec<LocalRgbAllocation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LocalRecipient {
    pub recipient_data: RecipientData,
    pub amount: u64,
    pub transport_endpoints: Vec<LocalTransportEndpoint>,
    pub vout: Option<u32>,
}

impl LocalRecipient {
    pub(crate) fn recipient_id(&self) -> String {
        self.recipient_data.recipient_id()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LocalRgbAllocation {
    /// Asset ID
    pub asset_id: Option<String>,
    /// RGB amount
    pub amount: u64,
    /// The status of the transfer that produced the RGB allocation
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
    pub(crate) kind: TransferKind,
    pub(crate) status: TransferStatus,
    pub(crate) txid: Option<String>,
    pub(crate) receive_utxo: Option<Outpoint>,
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

    pub(crate) fn set_asset(&self, asset: DbAssetActMod) -> Result<i32, InternalError> {
        let res = block_on(asset::Entity::insert(asset).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_asset_transfer(
        &self,
        asset_transfer: DbAssetTransferActMod,
    ) -> Result<i32, InternalError> {
        let res =
            block_on(asset_transfer::Entity::insert(asset_transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_backup_info(
        &self,
        backup_info: DbBackupInfoActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(backup_info::Entity::insert(backup_info).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_batch_transfer(
        &self,
        batch_transfer: DbBatchTransferActMod,
    ) -> Result<i32, InternalError> {
        let mut batch_transfer = batch_transfer;
        batch_transfer.updated_at = batch_transfer.created_at.clone();
        let res =
            block_on(batch_transfer::Entity::insert(batch_transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_coloring(&self, coloring: DbColoringActMod) -> Result<i32, InternalError> {
        let res = block_on(coloring::Entity::insert(coloring).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transport_endpoint(
        &self,
        transport_endpoint: DbTransportEndpointActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(
            transport_endpoint::Entity::insert(transport_endpoint).exec(self.get_connection()),
        )?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transfer(&self, transfer: DbTransferActMod) -> Result<i32, InternalError> {
        let res = block_on(transfer::Entity::insert(transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transfer_transport_endpoint(
        &self,
        transfer_transport_endpoint: DbTransferTransportEndpointActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(
            transfer_transport_endpoint::Entity::insert(transfer_transport_endpoint)
                .exec(self.get_connection()),
        )?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_txo(&self, txo: DbTxoActMod) -> Result<i32, InternalError> {
        let res = block_on(txo::Entity::insert(txo).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_wallet_transaction(
        &self,
        wallet_transaction: DbWalletTransactionActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(
            wallet_transaction::Entity::insert(wallet_transaction).exec(self.get_connection()),
        )?;
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

    pub(crate) fn update_backup_info(
        &self,
        backup_info: &mut DbBackupInfoActMod,
    ) -> Result<DbBackupInfo, InternalError> {
        Ok(block_on(
            backup_info::Entity::update(backup_info.clone()).exec(self.get_connection()),
        )?)
    }

    pub(crate) fn update_batch_transfer(
        &self,
        batch_transfer: &mut DbBatchTransferActMod,
    ) -> Result<DbBatchTransfer, InternalError> {
        let now = now().unix_timestamp();
        batch_transfer.updated_at = ActiveValue::Set(now);
        Ok(block_on(
            batch_transfer::Entity::update(batch_transfer.clone()).exec(self.get_connection()),
        )?)
    }

    pub(crate) fn update_transfer_transport_endpoint(
        &self,
        transfer_transport_endpoint: &mut DbTransferTransportEndpointActMod,
    ) -> Result<DbTransferTransportEndpoint, InternalError> {
        Ok(block_on(
            transfer_transport_endpoint::Entity::update(transfer_transport_endpoint.clone())
                .exec(self.get_connection()),
        )?)
    }

    pub(crate) fn update_txo(&self, txo: DbTxoActMod) -> Result<(), InternalError> {
        block_on(txo::Entity::update(txo).exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn del_backup_info(&self) -> Result<(), InternalError> {
        block_on(backup_info::Entity::delete_many().exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn del_batch_transfer(
        &self,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<(), InternalError> {
        block_on(transfer::Entity::delete_by_id(batch_transfer.idx).exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn del_coloring(&self, asset_transfer_idx: i32) -> Result<(), InternalError> {
        block_on(
            coloring::Entity::delete_many()
                .filter(coloring::Column::AssetTransferIdx.eq(asset_transfer_idx))
                .exec(self.get_connection()),
        )?;
        Ok(())
    }

    pub(crate) fn get_backup_info(&self) -> Result<Option<DbBackupInfo>, InternalError> {
        Ok(block_on(
            backup_info::Entity::find().one(self.get_connection()),
        )?)
    }

    pub(crate) fn get_transport_endpoint(
        &self,
        endpoint: String,
    ) -> Result<Option<DbTransportEndpoint>, InternalError> {
        Ok(block_on(
            transport_endpoint::Entity::find()
                .filter(transport_endpoint::Column::Endpoint.eq(endpoint))
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

    pub(crate) fn iter_assets(&self) -> Result<Vec<DbAsset>, InternalError> {
        Ok(block_on(asset::Entity::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_asset_transfers(&self) -> Result<Vec<DbAssetTransfer>, InternalError> {
        Ok(block_on(
            asset_transfer::Entity::find().all(self.get_connection()),
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

    pub(crate) fn iter_wallet_transactions(
        &self,
    ) -> Result<Vec<DbWalletTransaction>, InternalError> {
        Ok(block_on(
            wallet_transaction::Entity::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn get_transfer_transport_endpoints_data(
        &self,
        transfer_idx: i32,
    ) -> Result<Vec<(DbTransferTransportEndpoint, DbTransportEndpoint)>, InternalError> {
        Ok(block_on(
            transfer_transport_endpoint::Entity::find()
                .filter(transfer_transport_endpoint::Column::TransferIdx.eq(transfer_idx))
                .find_also_related(transport_endpoint::Entity)
                .order_by_asc(transfer_transport_endpoint::Column::Idx)
                .all(self.get_connection()),
        )?
        .into_iter()
        .map(|(tte, ce)| (tte, ce.expect("should be connected")))
        .collect())
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
        transfers: Option<Vec<DbTransfer>>,
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
        let transfers = if let Some(t) = transfers {
            t
        } else {
            self.iter_transfers()?
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
            Some(batch_transfers.clone()),
            Some(asset_transfers.clone()),
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

        let mut ass_pending_incoming: u64 = ass_allocations
            .iter()
            .filter(|a| !a.txo_spent && a.incoming && a.status.pending())
            .map(|a| a.amount)
            .sum();
        let witness_pending: u64 = transfers
            .iter()
            .filter(|t| t.incoming && t.recipient_type == Some(RecipientType::Witness))
            .filter_map(
                |t| match t.related_transfers(&asset_transfers, &batch_transfers) {
                    Ok((_, bt)) => {
                        if bt.status.waiting_confirmations() {
                            Some(Ok(t.amount.parse::<u64>().unwrap()))
                        } else {
                            None
                        }
                    }
                    Err(e) => Some(Err(e)),
                },
            )
            .collect::<Result<Vec<u64>, InternalError>>()?
            .iter()
            .sum();
        ass_pending_incoming += witness_pending;
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
            .iter_assets()?
            .iter()
            .map(|a| a.asset_id.clone())
            .collect())
    }

    pub(crate) fn check_asset_exists(&self, asset_id: String) -> Result<DbAsset, Error> {
        match block_on(
            asset::Entity::find()
                .filter(asset::Column::AssetId.eq(asset_id.clone()))
                .one(self.get_connection()),
        )
        .map_err(InternalError::from)?
        {
            Some(a) => Ok(a),
            None => Err(Error::AssetNotFound { asset_id }),
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
            Err(Error::BatchTransferNotFound { txid })
        }
    }

    pub(crate) fn get_transfer_or_fail(
        &self,
        recipient_id: String,
        transfers: &[DbTransfer],
    ) -> Result<DbTransfer, Error> {
        if let Some(transfer) = transfers
            .iter()
            .find(|t| t.recipient_id == Some(recipient_id.clone()))
        {
            Ok(transfer.clone())
        } else {
            Err(Error::TransferNotFound { recipient_id })
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
        let kind = if incoming {
            if filtered_coloring.clone().count() > 0
                && filtered_coloring
                    .clone()
                    .all(|c| c.coloring_type == ColoringType::Issue)
            {
                TransferKind::Issuance
            } else {
                match transfer.recipient_type.unwrap() {
                    RecipientType::Blind => TransferKind::ReceiveBlind,
                    RecipientType::Witness => TransferKind::ReceiveWitness,
                }
            }
        } else {
            TransferKind::Send
        };

        let txo_ids: Vec<i32> = filtered_coloring.clone().map(|c| c.txo_idx).collect();
        let transfer_txos: Vec<DbTxo> = txos
            .iter()
            .filter(|&t| txo_ids.contains(&t.idx))
            .cloned()
            .collect();
        let (receive_utxo, change_utxo) = match kind {
            TransferKind::ReceiveBlind | TransferKind::ReceiveWitness => {
                let received_txo_idx: Vec<i32> = filtered_coloring
                    .filter(|c| c.coloring_type == ColoringType::Receive)
                    .map(|c| c.txo_idx)
                    .collect();
                let receive_utxo = transfer_txos
                    .into_iter()
                    .filter(|t| received_txo_idx.contains(&t.idx))
                    .map(|t| t.outpoint())
                    .collect::<Vec<Outpoint>>()
                    .first()
                    .cloned();
                (receive_utxo, None)
            }
            TransferKind::Send => {
                let change_txo_idx: Vec<i32> = filtered_coloring
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
                (None, change_utxo)
            }
            TransferKind::Issuance => (None, None),
        };

        Ok(TransferData {
            kind,
            status: batch_transfer.status,
            txid: batch_transfer.txid.clone(),
            receive_utxo,
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
                asset_id: asset_transfer.asset_id.clone(),
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

pub(crate) mod enums;
