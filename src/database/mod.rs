pub(crate) mod entities;

use super::*;

use crate::database::entities::{
    asset, coloring, media, pending_witness_script, prelude::*, transfer_transport_endpoint,
    transport_endpoint, txo,
};

#[derive(Clone, Debug)]
pub(crate) struct DbAssetTransferData {
    pub(crate) asset_transfer: DbAssetTransfer,
    pub(crate) transfers: Vec<DbTransfer>,
}

impl DbBatchTransfer {
    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
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

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
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

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn pending(&self) -> bool {
        self.status.pending()
    }

    pub(crate) fn waiting_confirmations(&self) -> bool {
        self.status.waiting_confirmations()
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
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
        .contains(&self.r#type)
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

impl From<LocalOutput> for DbTxoActMod {
    fn from(x: LocalOutput) -> DbTxoActMod {
        DbTxoActMod {
            idx: ActiveValue::NotSet,
            txid: ActiveValue::Set(x.outpoint.txid.to_string()),
            vout: ActiveValue::Set(x.outpoint.vout),
            btc_amount: ActiveValue::Set(x.txout.value.to_sat().to_string()),
            spent: ActiveValue::Set(false),
            exists: ActiveValue::Set(true),
            pending_witness: ActiveValue::Set(false),
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
    /// Number of pending blind receive operations
    pub pending_blinded: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LocalWitnessData {
    pub amount_sat: u64,
    pub blinding: Option<u64>,
    pub vout: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum LocalRecipientData {
    Blind(SecretSeal),
    Witness(LocalWitnessData),
}

impl LocalRecipientData {
    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn vout(&self) -> Option<u32> {
        match &self {
            LocalRecipientData::Blind(_) => None,
            LocalRecipientData::Witness(d) => Some(d.vout),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LocalRecipient {
    pub recipient_id: String,
    pub local_recipient_data: LocalRecipientData,
    pub assignment: Assignment,
    pub transport_endpoints: Vec<LocalTransportEndpoint>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct LocalRgbAllocation {
    /// Asset ID
    pub asset_id: Option<String>,
    /// RGB assignment
    pub assignment: Assignment,
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
    pub(crate) batch_transfer_idx: i32,
    pub(crate) assignments: Vec<Assignment>,
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
        let res = block_on(Asset::insert(asset).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_asset_transfer(
        &self,
        asset_transfer: DbAssetTransferActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(AssetTransfer::insert(asset_transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_backup_info(
        &self,
        backup_info: DbBackupInfoActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(BackupInfo::insert(backup_info).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_batch_transfer(
        &self,
        batch_transfer: DbBatchTransferActMod,
    ) -> Result<i32, InternalError> {
        let mut batch_transfer = batch_transfer;
        batch_transfer.updated_at = batch_transfer.created_at.clone();
        let res = block_on(BatchTransfer::insert(batch_transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_coloring(&self, coloring: DbColoringActMod) -> Result<i32, InternalError> {
        let res = block_on(Coloring::insert(coloring).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_media(&self, media: DbMediaActMod) -> Result<i32, InternalError> {
        let res = block_on(Media::insert(media).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_pending_witness_script(
        &self,
        pending_witness_script: DbPendingWitnessScriptActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(
            PendingWitnessScript::insert(pending_witness_script).exec(self.get_connection()),
        )?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_token(&self, token: DbTokenActMod) -> Result<i32, InternalError> {
        let res = block_on(Token::insert(token).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_token_media(
        &self,
        token_media: DbTokenMediaActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(TokenMedia::insert(token_media).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transport_endpoint(
        &self,
        transport_endpoint: DbTransportEndpointActMod,
    ) -> Result<i32, InternalError> {
        let res =
            block_on(TransportEndpoint::insert(transport_endpoint).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transfer(&self, transfer: DbTransferActMod) -> Result<i32, InternalError> {
        let res = block_on(Transfer::insert(transfer).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    pub(crate) fn set_transfer_transport_endpoint(
        &self,
        transfer_transport_endpoint: DbTransferTransportEndpointActMod,
    ) -> Result<i32, InternalError> {
        let res = block_on(
            TransferTransportEndpoint::insert(transfer_transport_endpoint)
                .exec(self.get_connection()),
        )?;
        Ok(res.last_insert_id)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn set_txo(&self, txo: DbTxoActMod) -> Result<i32, InternalError> {
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
        let conn = self.get_connection();
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

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn set_wallet_transaction(
        &self,
        wallet_transaction: DbWalletTransactionActMod,
    ) -> Result<i32, InternalError> {
        let res =
            block_on(WalletTransaction::insert(wallet_transaction).exec(self.get_connection()))?;
        Ok(res.last_insert_id)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn update_transfer(
        &self,
        transfer: &mut DbTransferActMod,
    ) -> Result<DbTransfer, InternalError> {
        Ok(block_on(
            Transfer::update(transfer.clone()).exec(self.get_connection()),
        )?)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn update_asset_transfer(
        &self,
        asset_transfer: &mut DbAssetTransferActMod,
    ) -> Result<DbAssetTransfer, InternalError> {
        Ok(block_on(
            AssetTransfer::update(asset_transfer.clone()).exec(self.get_connection()),
        )?)
    }

    pub(crate) fn update_backup_info(
        &self,
        backup_info: &mut DbBackupInfoActMod,
    ) -> Result<DbBackupInfo, InternalError> {
        Ok(block_on(
            BackupInfo::update(backup_info.clone()).exec(self.get_connection()),
        )?)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn update_batch_transfer(
        &self,
        batch_transfer: &mut DbBatchTransferActMod,
    ) -> Result<DbBatchTransfer, InternalError> {
        let now = now().unix_timestamp();
        batch_transfer.updated_at = ActiveValue::Set(now);
        Ok(block_on(
            BatchTransfer::update(batch_transfer.clone()).exec(self.get_connection()),
        )?)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn update_transfer_transport_endpoint(
        &self,
        transfer_transport_endpoint: &mut DbTransferTransportEndpointActMod,
    ) -> Result<DbTransferTransportEndpoint, InternalError> {
        Ok(block_on(
            TransferTransportEndpoint::update(transfer_transport_endpoint.clone())
                .exec(self.get_connection()),
        )?)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn update_txo(&self, txo: DbTxoActMod) -> Result<(), InternalError> {
        block_on(Txo::update(txo).exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn del_backup_info(&self) -> Result<(), InternalError> {
        block_on(BackupInfo::delete_many().exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn del_batch_transfer(
        &self,
        batch_transfer: &DbBatchTransfer,
    ) -> Result<(), InternalError> {
        block_on(Transfer::delete_by_id(batch_transfer.idx).exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn del_coloring(&self, asset_transfer_idx: i32) -> Result<(), InternalError> {
        block_on(
            Coloring::delete_many()
                .filter(coloring::Column::AssetTransferIdx.eq(asset_transfer_idx))
                .exec(self.get_connection()),
        )?;
        Ok(())
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn del_pending_witness_script(&self, script: String) -> Result<(), InternalError> {
        block_on(
            PendingWitnessScript::delete_many()
                .filter(pending_witness_script::Column::Script.eq(script))
                .exec(self.get_connection()),
        )?;
        Ok(())
    }

    pub(crate) fn del_txo(&self, idx: i32) -> Result<(), InternalError> {
        block_on(Coloring::delete_by_id(idx).exec(self.get_connection()))?;
        Ok(())
    }

    pub(crate) fn get_asset(&self, asset_id: String) -> Result<Option<DbAsset>, InternalError> {
        Ok(block_on(
            Asset::find()
                .filter(asset::Column::Id.eq(asset_id.clone()))
                .one(self.get_connection()),
        )?)
    }

    pub(crate) fn get_backup_info(&self) -> Result<Option<DbBackupInfo>, InternalError> {
        Ok(block_on(BackupInfo::find().one(self.get_connection()))?)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn get_media(&self, media_idx: i32) -> Result<Option<DbMedia>, InternalError> {
        Ok(block_on(
            Media::find()
                .filter(media::Column::Idx.eq(media_idx))
                .one(self.get_connection()),
        )?)
    }

    pub(crate) fn get_media_by_digest(
        &self,
        digest: String,
    ) -> Result<Option<DbMedia>, InternalError> {
        Ok(block_on(
            Media::find()
                .filter(media::Column::Digest.eq(digest))
                .one(self.get_connection()),
        )?)
    }

    pub(crate) fn get_transport_endpoint(
        &self,
        endpoint: String,
    ) -> Result<Option<DbTransportEndpoint>, InternalError> {
        Ok(block_on(
            TransportEndpoint::find()
                .filter(transport_endpoint::Column::Endpoint.eq(endpoint))
                .one(self.get_connection()),
        )?)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn get_txo(&self, outpoint: &Outpoint) -> Result<Option<DbTxo>, InternalError> {
        Ok(block_on(
            Txo::find()
                .filter(txo::Column::Txid.eq(outpoint.txid.clone()))
                .filter(txo::Column::Vout.eq(outpoint.vout))
                .one(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_assets(&self) -> Result<Vec<DbAsset>, InternalError> {
        Ok(block_on(Asset::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_asset_transfers(&self) -> Result<Vec<DbAssetTransfer>, InternalError> {
        Ok(block_on(AssetTransfer::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_batch_transfers(&self) -> Result<Vec<DbBatchTransfer>, InternalError> {
        Ok(block_on(BatchTransfer::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_colorings(&self) -> Result<Vec<DbColoring>, InternalError> {
        Ok(block_on(Coloring::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_media(&self) -> Result<Vec<DbMedia>, InternalError> {
        Ok(block_on(Media::find().all(self.get_connection()))?)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn iter_pending_witness_scripts(
        &self,
    ) -> Result<Vec<DbPendingWitnessScript>, InternalError> {
        Ok(block_on(
            PendingWitnessScript::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn iter_token_medias(&self) -> Result<Vec<DbTokenMedia>, InternalError> {
        Ok(block_on(TokenMedia::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_tokens(&self) -> Result<Vec<DbToken>, InternalError> {
        Ok(block_on(Token::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_transfers(&self) -> Result<Vec<DbTransfer>, InternalError> {
        Ok(block_on(Transfer::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_txos(&self) -> Result<Vec<DbTxo>, InternalError> {
        Ok(block_on(Txo::find().all(self.get_connection()))?)
    }

    pub(crate) fn iter_wallet_transactions(
        &self,
    ) -> Result<Vec<DbWalletTransaction>, InternalError> {
        Ok(block_on(
            WalletTransaction::find().all(self.get_connection()),
        )?)
    }

    pub(crate) fn get_transfer_transport_endpoints_data(
        &self,
        transfer_idx: i32,
    ) -> Result<Vec<(DbTransferTransportEndpoint, DbTransportEndpoint)>, InternalError> {
        Ok(block_on(
            TransferTransportEndpoint::find()
                .filter(transfer_transport_endpoint::Column::TransferIdx.eq(transfer_idx))
                .find_also_related(TransportEndpoint)
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
            .filter_map(
                |t| match t.related_transfers(&asset_transfers, &batch_transfers) {
                    Ok((at, bt)) => {
                        if bt.status.waiting_confirmations() {
                            // filter for asset ID (always present in WaitingConfirmations status)
                            if at.asset_id.unwrap() != asset_id {
                                return None;
                            }
                            Some(Ok(t
                                .requested_assignment
                                .as_ref()
                                .map(|a| a.main_amount())
                                .unwrap_or(0)))
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

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn get_asset_ids(&self) -> Result<Vec<String>, InternalError> {
        Ok(self.iter_assets()?.iter().map(|a| a.id.clone()).collect())
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

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
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

        let assignments = filtered_coloring
            .clone()
            .filter(|c| c.r#type != ColoringType::Input)
            .map(|c| c.assignment)
            .collect();

        let incoming = transfer.incoming;
        let kind = if incoming {
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
            TransferKind::Send
        };

        let txo_ids: Vec<i32> = filtered_coloring.clone().map(|c| c.txo_idx).collect();
        let transfer_txos: Vec<DbTxo> = txos
            .iter()
            .filter(|&t| txo_ids.contains(&t.idx))
            .cloned()
            .collect();
        let receive_utxo = match &transfer.recipient_type {
            Some(RecipientTypeFull::Blind { unblinded_utxo }) => Some(unblinded_utxo.clone()),
            Some(RecipientTypeFull::Witness { .. }) => {
                let received_txo_idx: Vec<i32> = filtered_coloring
                    .clone()
                    .filter(|c| c.r#type == ColoringType::Receive)
                    .map(|c| c.txo_idx)
                    .collect();
                transfer_txos
                    .clone()
                    .into_iter()
                    .filter(|t| received_txo_idx.contains(&t.idx))
                    .map(|t| t.outpoint())
                    .collect::<Vec<Outpoint>>()
                    .first()
                    .cloned()
            }
            _ => None,
        };
        let change_utxo = match kind {
            TransferKind::ReceiveBlind | TransferKind::ReceiveWitness => None,
            TransferKind::Send => {
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

            allocations.push(LocalRgbAllocation {
                asset_id: asset_transfer.asset_id.clone(),
                assignment: c.assignment.clone(),
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
        transfers: Option<Vec<DbTransfer>>,
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
        let transfers = if let Some(ts) = transfers {
            ts
        } else {
            self.iter_transfers()?
        };

        let pending_blinded_utxos = transfers
            .iter()
            .filter_map(|t| match (&t.recipient_type, t.incoming) {
                (Some(RecipientTypeFull::Blind { unblinded_utxo }), true) => t
                    .related_transfers(&asset_transfers, &batch_transfers)
                    .ok()
                    .filter(|(_, bt)| bt.status.waiting_counterparty())
                    .map(|_| unblinded_utxo),
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
                    rgb_allocations: self._get_utxo_allocations(
                        t,
                        colorings.clone(),
                        asset_transfers.clone(),
                        batch_transfers.clone(),
                    )?,
                    pending_blinded: *pending_blinded_utxos.get(&t.outpoint()).unwrap_or(&0),
                })
            })
            .collect()
    }
}

pub(crate) mod enums;
