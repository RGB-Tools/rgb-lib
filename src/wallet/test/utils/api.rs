use super::*;

// singlesig party without an online handle (for offline-only test APIs)
pub(crate) struct OfflineSinglesigParty {
    pub(crate) wallet: Wallet,
}

// singlesig party (allows uniform access to some functionality via SigParty trait)
pub(crate) struct SinglesigParty {
    pub(crate) wallet: Wallet,
    pub(crate) online: Online,
}

// convenience macro to instantiate OfflineSinglesigParty
macro_rules! offline_party {
    ($wallet:expr) => {
        OfflineSinglesigParty { wallet: $wallet }
    };
}

// convenience macro to instantiate SinglesigParty
macro_rules! party {
    ($wallet:expr, $online:expr) => {
        SinglesigParty {
            wallet: $wallet,
            online: $online,
        }
    };
}

// convenience trait to allow uniform access to offline functionality from all parties
pub(crate) trait OfflineSigParty {
    type W: RgbWalletOpsOffline;

    fn wlt(&self) -> &Self::W;

    fn wlt_mut(&mut self) -> &mut Self::W;

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn check_test_transfer_status_recipient(
        &self,
        recipient_id: &str,
        expected_status: TransferStatus,
    ) -> bool {
        let transfers = self.db_transfers();
        let mut recipient_transfers = transfers
            .iter()
            .filter(|t| t.recipient_id.as_deref() == Some(recipient_id));
        let transfer = recipient_transfers.next().unwrap();
        assert!(recipient_transfers.next().is_none());
        let db_data = self.db_data(false);
        let (asset_transfer, batch_transfer) =
            transfer.related_transfers(&db_data.asset_transfers, &db_data.batch_transfers);
        let transfer_data = self
            .wlt()
            .get_transfer_data(
                transfer,
                &asset_transfer,
                &batch_transfer,
                &db_data.txos,
                &db_data.colorings,
            )
            .unwrap();
        println!(
            "receive with recipient_id {} is in status {:?}",
            recipient_id, &transfer_data.status
        );
        transfer_data.status == expected_status
    }

    fn check_test_transfer_status_sender(
        &self,
        txid: &str,
        expected_status: TransferStatus,
    ) -> bool {
        let batch_transfers: Vec<_> = self
            .db_batch_transfers()
            .into_iter()
            .filter(|b| b.txid == Some(txid.to_string()))
            .collect();
        assert_eq!(batch_transfers.len(), 1);
        let batch_transfer = batch_transfers.first().unwrap();
        println!(
            "send with txid {} is in status {:?}",
            txid, &batch_transfer.status
        );
        batch_transfer.status == expected_status
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn check_test_wallet_data(
        &mut self,
        asset: &AssetNIA,
        custom_issued_supply: Option<u64>,
        transfer_num: usize,
        spent_amount: u64,
    ) {
        println!("checking wallet data...");
        let issued_supply = match custom_issued_supply {
            Some(supply) => supply,
            None => AMOUNT,
        };
        // asset list
        let assets = self.list_assets(&[]);
        let nia_assets = assets.nia.unwrap();
        let cfa_assets = assets.cfa.unwrap();
        assert_eq!(nia_assets.len(), 1);
        assert_eq!(cfa_assets.len(), 0);
        let nia_asset = nia_assets.first().unwrap();
        assert_eq!(nia_asset.asset_id, asset.asset_id);
        // asset balance
        let balance = self.get_asset_balance(&asset.asset_id);
        assert_eq!(
            balance,
            Balance {
                settled: asset.balance.settled - spent_amount,
                future: asset.balance.future - spent_amount,
                spendable: asset.balance.spendable - spent_amount,
            }
        );
        // asset metadata
        let metadata = self.get_asset_metadata(&asset.asset_id);
        assert_eq!(metadata.asset_schema, AssetSchema::Nia);
        assert_eq!(metadata.initial_supply, issued_supply);
        assert_eq!(metadata.name, asset.name);
        assert_eq!(metadata.precision, asset.precision);
        assert_eq!(metadata.ticker.unwrap(), asset.ticker);
        // transfer list
        let transfers = self.list_transfers(Some(&asset.asset_id));
        assert_eq!(transfers.len(), 1 + transfer_num);
        assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);
        assert_eq!(transfers.last().unwrap().kind, TransferKind::Send);
        assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
        // unspent list
        let unspents = self.list_unspents(false);
        assert_eq!(unspents.len(), 6);
    }

    fn data_dir(&self) -> String {
        self.wlt().get_wallet_data().data_dir
    }

    fn db_asset(&self, asset_id: &str) -> DbAsset {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let asset = txn.get_asset(asset_id.to_string()).unwrap().unwrap();
        txn.commit().unwrap();
        asset
    }

    fn db_asset_transfers(&self) -> Vec<DbAssetTransfer> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let asset_transfers = txn.iter_asset_transfers().unwrap();
        txn.commit().unwrap();
        asset_transfers
    }

    fn db_asset_transfers_filtered(&self, batch_transfer_idx: i32) -> Vec<DbAssetTransfer> {
        self.db_asset_transfers()
            .into_iter()
            .filter(|at| at.batch_transfer_idx == batch_transfer_idx)
            .collect()
    }

    fn db_backup_info(&self) -> DbBackupInfo {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let bak_info = txn.get_backup_info().unwrap().unwrap();
        txn.commit().unwrap();
        bak_info
    }

    fn db_backup_info_opt(&self) -> Option<DbBackupInfo> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let bak_info = txn.get_backup_info().unwrap();
        txn.commit().unwrap();
        bak_info
    }

    fn db_batch_transfers(&self) -> Vec<DbBatchTransfer> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let batch_transfers = txn.iter_batch_transfers().unwrap();
        txn.commit().unwrap();
        batch_transfers
    }

    fn db_batch_transfers_filtered(&self, txid: &str) -> Vec<DbBatchTransfer> {
        self.db_batch_transfers()
            .into_iter()
            .filter(|b| b.txid == Some(txid.to_string()))
            .collect()
    }

    fn db_check_asset_exists(&self, asset_id: &str) -> Result<DbAsset, Error> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let res = txn.check_asset_exists(asset_id.to_string());
        txn.commit().unwrap();
        res
    }

    fn db_colorings(&self) -> Vec<DbColoring> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let colorings = txn.iter_colorings().unwrap();
        txn.commit().unwrap();
        colorings
    }

    fn db_colorings_filtered(&self, asset_transfer_idx: i32) -> Vec<DbColoring> {
        self.db_colorings()
            .into_iter()
            .filter(|c| c.asset_transfer_idx == asset_transfer_idx)
            .collect()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn db_data(&self, empty_transfers: bool) -> DbData {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let db_data = txn.get_db_data(empty_transfers).unwrap();
        txn.commit().unwrap();
        db_data
    }

    fn db_del_transfer_transport_endpoint(&self, idx: i32) {
        let txn = self.wlt().database().begin_transaction().unwrap();
        txn.del_transfer_transport_endpoint(idx).unwrap();
        txn.commit().unwrap();
    }

    fn db_get_or_insert_media(&self, digest: &str, mime: &str) -> i32 {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let media_idx = txn
            .get_or_insert_media(digest.to_string(), mime.to_string())
            .unwrap();
        txn.commit().unwrap();
        media_idx
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn db_media(&self, media_idx: i32) -> DbMedia {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let media = txn.get_media(media_idx).unwrap().unwrap();
        txn.commit().unwrap();
        media
    }

    fn db_medias(&self) -> Vec<DbMedia> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let medias = txn.iter_media().unwrap();
        txn.commit().unwrap();
        medias
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn db_pending_witness_scripts(&self) -> Vec<DbPendingWitnessScript> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let pending_witness_scripts = txn.iter_pending_witness_scripts().unwrap();
        txn.commit().unwrap();
        pending_witness_scripts
    }

    fn db_reserved_txos(&self) -> Vec<DbReservedTxo> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let reserved_txos = txn.iter_reserved_txos().unwrap();
        txn.commit().unwrap();
        reserved_txos
    }

    fn db_rgb_allocations(
        &self,
        utxos: Vec<DbTxo>,
        colorings: Option<Vec<DbColoring>>,
        batch_transfers: Option<Vec<DbBatchTransfer>>,
        asset_transfers: Option<Vec<DbAssetTransfer>>,
        transfers: Option<Vec<DbTransfer>>,
    ) -> Vec<LocalUnspent> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let rgb_allocations = txn
            .get_rgb_allocations(
                utxos,
                colorings,
                batch_transfers,
                asset_transfers,
                transfers,
            )
            .unwrap();
        txn.commit().unwrap();
        rgb_allocations
    }

    fn db_token_medias(&self) -> Vec<DbTokenMedia> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let token_medias = txn.iter_token_medias().unwrap();
        txn.commit().unwrap();
        token_medias
    }

    fn db_tokens(&self) -> Vec<DbToken> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let tokens = txn.iter_tokens().unwrap();
        txn.commit().unwrap();
        tokens
    }

    fn db_transfer_transport_endpoints_data(
        &self,
        idx: i32,
    ) -> Vec<(DbTransferTransportEndpoint, DbTransportEndpoint)> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let tte_data = txn.get_transfer_transport_endpoints_data(idx).unwrap();
        txn.commit().unwrap();
        tte_data
    }

    fn db_transfers(&self) -> Vec<DbTransfer> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let transfers = txn.iter_transfers().unwrap();
        txn.commit().unwrap();
        transfers
    }

    fn db_transfers_filtered(&self, asset_transfer_idx: i32) -> Vec<DbTransfer> {
        self.db_transfers()
            .into_iter()
            .filter(|t| t.asset_transfer_idx == asset_transfer_idx)
            .collect()
    }

    fn db_txo(&self, outpoint: &Outpoint) -> Option<DbTxo> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let txo = txn.get_txo(outpoint).unwrap();
        txn.commit().unwrap();
        txo
    }

    fn db_txos(&self) -> Vec<DbTxo> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let txos = txn.iter_txos().unwrap();
        txn.commit().unwrap();
        txos
    }

    fn db_unspent_txos(&self, txos: Vec<DbTxo>) -> Vec<DbTxo> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let unspent_txos = txn.get_unspent_txos(txos).unwrap();
        txn.commit().unwrap();
        unspent_txos
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn db_update_asset(&self, asset: &mut DbAssetActMod) -> DbAsset {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let asset = txn.update_asset(asset).unwrap();
        txn.commit().unwrap();
        asset
    }

    fn db_wallet_transaction_with_reserved_txos_by_txid(
        &self,
        txid: &str,
    ) -> Option<(DbWalletTransaction, Vec<DbReservedTxo>)> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let res = txn
            .get_wallet_transaction_with_reserved_txos_by_txid(txid)
            .unwrap();
        txn.commit().unwrap();
        res
    }

    fn db_wallet_transactions(&self) -> Vec<DbWalletTransaction> {
        let txn = self.wlt().database().begin_transaction().unwrap();
        let transactions = txn.iter_wallet_transactions().unwrap();
        txn.commit().unwrap();
        transactions
    }

    fn delete_transfers(&mut self, batch_transfer_idx: Option<i32>, no_asset_only: bool) -> bool {
        self.delete_transfers_result(batch_transfer_idx, no_asset_only)
            .unwrap()
    }

    fn delete_transfers_result(
        &self,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
    ) -> Result<bool, Error> {
        self.wlt()
            .delete_transfers(batch_transfer_idx, no_asset_only)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn extract_opouts_from_transfer(&self, asset_id: &str, txid: &str) -> Vec<Opout> {
        let batch_transfers = self.db_batch_transfers_filtered(txid);
        assert_eq!(batch_transfers.len(), 1);
        let batch_transfer = batch_transfers.first().unwrap();
        let asset_transfers = self.db_asset_transfers_filtered(batch_transfer.idx);
        let asset_transfers = asset_transfers
            .iter()
            .filter(|at| at.asset_id.as_ref() == Some(&asset_id.to_string()))
            .filter(|t| t.user_driven)
            .collect::<Vec<_>>();
        assert_eq!(asset_transfers.len(), 1);
        let asset_transfer = asset_transfers.first().unwrap();
        let colorings: Vec<DbColoring> = self
            .db_colorings()
            .into_iter()
            .filter(|c| c.asset_transfer_idx == asset_transfer.idx)
            .collect();
        if colorings.is_empty() {
            panic!("cannot find colorings for this transfer");
        }
        let txo_indices = colorings.iter().map(|c| c.txo_idx).collect::<Vec<_>>();
        let relevant_txos = self
            .db_txos()
            .into_iter()
            .filter(|t| txo_indices.contains(&t.idx));
        let mut outpoints = relevant_txos
            .map(|txo| OutPoint::from(txo.clone()))
            .peekable();
        if outpoints.peek().is_none() {
            panic!("cannot find outpoints for this transfer");
        }
        let contract_id = ContractId::from_str(asset_id).unwrap();
        let runtime = self.wlt().rgb_runtime().unwrap();
        let assignments = runtime
            .contract_assignments_for(contract_id, outpoints)
            .unwrap();
        let mut opouts = Vec::new();
        for (_explicit_seal, opout_state_map) in assignments {
            for (opout, _state) in opout_state_map {
                opouts.push(opout);
            }
        }
        opouts
    }

    fn get_asset_balance(&self, asset_id: &str) -> Balance {
        self.get_asset_balance_result(asset_id).unwrap()
    }

    fn get_asset_balance_result(&self, asset_id: &str) -> Result<Balance, Error> {
        self.wlt().get_asset_balance(asset_id.to_string())
    }

    fn get_asset_metadata(&self, asset_id: &str) -> Metadata {
        self.get_asset_metadata_result(asset_id).unwrap()
    }

    fn get_asset_metadata_result(&self, asset_id: &str) -> Result<Metadata, Error> {
        self.wlt().get_asset_metadata(asset_id.to_string())
    }

    fn get_btc_balance(&mut self) -> BtcBalance {
        self.wlt_mut().get_btc_balance(None, true).unwrap()
    }

    fn get_pending_blind_transfers(&self) -> Vec<Transfer> {
        self.wlt()
            .list_transfers(None)
            .unwrap()
            .into_iter()
            .filter(|t| t.status.pending() && t.kind == TransferKind::ReceiveBlind)
            .collect()
    }

    fn get_test_asset_transfer(&self, batch_transfer_idx: i32) -> DbAssetTransfer {
        let asset_transfers = self.db_asset_transfers_filtered(batch_transfer_idx);
        let mut user_driven_transfers = asset_transfers.into_iter().filter(|t| t.user_driven);
        let user_driven_transfer = user_driven_transfers.next().unwrap();
        assert!(user_driven_transfers.next().is_none());
        user_driven_transfer
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn get_test_transfer_data(&self, transfer: &DbTransfer) -> (TransferData, DbAssetTransfer) {
        let db_data = self.db_data(false);
        let (asset_transfer, batch_transfer) =
            transfer.related_transfers(&db_data.asset_transfers, &db_data.batch_transfers);
        let transfer_data = self
            .wlt()
            .get_transfer_data(
                transfer,
                &asset_transfer,
                &batch_transfer,
                &db_data.txos,
                &db_data.colorings,
            )
            .unwrap();
        (transfer_data, asset_transfer)
    }

    fn get_test_transfer_recipient(&self, recipient_id: &str) -> DbTransfer {
        let mut transfers = self
            .db_transfers()
            .into_iter()
            .filter(|t| t.recipient_id == Some(recipient_id.to_string()) && t.incoming);
        let transfer = transfers.next().unwrap();
        assert!(transfers.next().is_none());
        transfer
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn get_test_transfer_related(
        &self,
        transfer: &DbTransfer,
    ) -> (DbAssetTransfer, DbBatchTransfer) {
        let db_data = self.db_data(false);
        transfer.related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)
    }

    fn get_test_transfer_sender(
        &self,
        txid: &str,
    ) -> (DbTransfer, DbAssetTransfer, DbBatchTransfer) {
        let batch_transfers = self.db_batch_transfers_filtered(txid);
        assert_eq!(batch_transfers.len(), 1);
        let batch_transfer = batch_transfers.into_iter().next().unwrap();
        let asset_transfer = self.get_test_asset_transfer(batch_transfer.idx);
        let mut transfers = self.db_transfers_filtered(asset_transfer.idx).into_iter();
        let transfer = transfers.next().unwrap();
        assert!(transfers.next().is_none());
        (transfer, asset_transfer, batch_transfer)
    }

    fn get_test_transfers_sender(
        &self,
        txid: &str,
    ) -> (
        HashMap<String, Vec<DbTransfer>>,
        Vec<DbAssetTransfer>,
        DbBatchTransfer,
    ) {
        let batch_transfers = self.db_batch_transfers_filtered(txid);
        assert_eq!(batch_transfers.len(), 1);
        let batch_transfer = batch_transfers.into_iter().next().unwrap();
        let asset_transfers = self.db_asset_transfers_filtered(batch_transfer.idx);
        let mut transfers: HashMap<String, Vec<DbTransfer>> = HashMap::new();
        for asset_transfer in &asset_transfers {
            let asset_id = asset_transfer.asset_id.clone().unwrap();
            transfers.insert(asset_id, self.db_transfers_filtered(asset_transfer.idx));
        }
        (transfers, asset_transfers, batch_transfer)
    }

    fn get_wallet_data(&self) -> WalletData {
        self.wlt().get_wallet_data()
    }

    fn list_assets(&self, filter_asset_schemas: &[AssetSchema]) -> Assets {
        self.wlt()
            .list_assets(filter_asset_schemas.to_vec())
            .unwrap()
    }

    fn list_transactions(&mut self) -> Vec<Transaction> {
        self.wlt_mut().list_transactions(None, true).unwrap()
    }

    fn list_transfers(&self, asset_id: Option<&str>) -> Vec<Transfer> {
        self.list_transfers_result(asset_id).unwrap()
    }

    fn list_transfers_result(&self, asset_id: Option<&str>) -> Result<Vec<Transfer>, Error> {
        self.wlt().list_transfers(asset_id.map(|a| a.to_string()))
    }

    fn list_unspents(&mut self, settled_only: bool) -> Vec<Unspent> {
        self.wlt_mut()
            .list_unspents(None, settled_only, true)
            .unwrap()
    }

    /// print the provided message, then get colorings for each wallet unspent and print their
    /// status, type, amount and asset
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn show_unspent_colorings(&mut self, msg: &str) {
        println!(
            "\nwallet {} unspent colorings ({msg})",
            self.wlt().get_wallet_data().data_dir
        );
        let unspents = self
            .wlt_mut()
            .list_unspents(None, false, true)
            .unwrap()
            .into_iter()
            .filter(|u| u.utxo.colorable);
        let db_txos = self.db_txos();
        let db_colorings = self.db_colorings();
        let db_asset_transfers = self.db_asset_transfers();
        let db_batch_transfers = self.db_batch_transfers();
        let pending_blind_transfers = self.get_pending_blind_transfers();
        for unspent in unspents {
            let outpoint = unspent.utxo.outpoint;
            let db_txo = db_txos
                .iter()
                .find(|t| t.txid == outpoint.txid && t.vout == outpoint.vout)
                .unwrap();
            let txo_pending_blind_transfers = pending_blind_transfers.iter().filter(|t| {
                if let Some(txo) = &t.receive_utxo {
                    db_txo.outpoint() == *txo
                } else {
                    false
                }
            });
            println!(
                "> {}:{}, {} sat{}",
                outpoint.txid,
                outpoint.vout,
                unspent.utxo.btc_amount,
                if !unspent.utxo.exists {
                    " - tx not broadcast yet"
                } else {
                    ""
                },
            );
            let txo_db_colorings = db_colorings.iter().filter(|c| c.txo_idx == db_txo.idx);
            for db_coloring in txo_db_colorings {
                let db_asset_transfer = db_asset_transfers
                    .iter()
                    .find(|a| a.idx == db_coloring.asset_transfer_idx)
                    .unwrap();
                let db_batch_transfer = db_batch_transfers
                    .iter()
                    .find(|b| b.idx == db_asset_transfer.batch_transfer_idx)
                    .unwrap();
                println!(
                    "\t- {:?} {:?} of {:?} for {:?}",
                    db_batch_transfer.status,
                    db_coloring.r#type,
                    db_coloring.assignment,
                    db_asset_transfer.asset_id.as_ref(),
                );
            }
            for pbt in txo_pending_blind_transfers {
                println!("\t- pending blind receive with transfer ID {}", pbt.idx);
            }
        }
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn wait_for_asset_balance(&self, asset_id: &str, expected_balance: &Balance) {
        println!("waiting for asset balance");
        let mut current_balance = Balance::default();
        let check = || {
            current_balance = self.get_asset_balance(asset_id);
            if &current_balance == expected_balance {
                return true;
            }
            false
        };
        if !wait_for_function(check, 10, 500) {
            println!("current balance: {current_balance:?}");
            println!("expected balance: {expected_balance:?}");
            panic!("asset balance is not becoming the expected one");
        }
    }
}

// convenience trait to allow uniform access to online functionality from all parties
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) trait SigParty: OfflineSigParty<W: RgbWalletOpsOnline> {
    fn party_online(&self) -> Online;

    fn fail_transfers(
        &mut self,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
        skip_sync: bool,
    ) -> Result<bool, Error> {
        let online = self.party_online();
        self.wlt_mut()
            .fail_transfers(online, batch_transfer_idx, no_asset_only, skip_sync)
    }

    fn fail_transfers_all(&mut self) -> bool {
        let online = self.party_online();
        self.wlt_mut()
            .fail_transfers(online, None, false, false)
            .unwrap()
    }

    fn fail_transfers_single(&mut self, batch_transfer_idx: i32) -> bool {
        let online = self.party_online();
        self.wlt_mut()
            .fail_transfers(online, Some(batch_transfer_idx), false, false)
            .unwrap()
    }

    fn get_btc_balance_with_sync(&mut self) -> BtcBalance {
        let online = self.party_online();
        self.wlt_mut().get_btc_balance(Some(online), false).unwrap()
    }

    fn get_colorable_unspents_with_sync(&mut self, settled_only: bool) -> Vec<Unspent> {
        self.list_unspents_with_sync(settled_only)
            .into_iter()
            .filter(|u| u.utxo.colorable)
            .collect()
    }

    fn list_transactions_with_sync(&mut self) -> Vec<Transaction> {
        let online = self.party_online();
        self.wlt_mut()
            .list_transactions(Some(online), false)
            .unwrap()
    }

    fn list_unspents_with_sync(&mut self, settled_only: bool) -> Vec<Unspent> {
        let online = self.party_online();
        self.wlt_mut()
            .list_unspents(Some(online), settled_only, false)
            .unwrap()
    }

    fn wait_for_refresh(&mut self, asset_id: Option<&str>) {
        self.wait_for_refresh_raw(asset_id, None);
    }

    fn refresh_result(
        &mut self,
        asset_id: Option<&str>,
        filter: &[RefreshFilter],
    ) -> Result<RefreshResult, Error> {
        let online = self.party_online();
        self.wlt_mut().refresh(
            online,
            asset_id.map(|a| a.to_string()),
            filter.to_vec(),
            false,
        )
    }

    fn sync(&mut self, options: SyncOptions) {
        self.sync_result(options).unwrap()
    }

    fn sync_result(&mut self, options: SyncOptions) -> Result<(), Error> {
        let online = self.party_online();
        self.wlt_mut().sync(online, options)
    }

    fn wait_for_btc_balance(&mut self, expected_balance: &BtcBalance) {
        println!("waiting for BTC balance");
        let mut current_balance = BtcBalance::default();
        let check = || {
            current_balance = self.get_btc_balance_with_sync();
            if &current_balance == expected_balance {
                return true;
            }
            false
        };
        if !wait_for_function(check, 10, 500) {
            println!("current balance: {current_balance:?}");
            println!("expected balance: {expected_balance:?}");
            panic!("BTC balance is not becoming the expected one");
        }
    }

    fn wait_for_refresh_raw(&mut self, asset_id: Option<&str>, transfer_ids: Option<&[i32]>) {
        println!(
            "waiting for refresh ({})",
            self.wlt().get_wallet_data().data_dir
        );
        let mut seen = HashSet::new();
        let mut target_set = HashSet::new();
        if let Some(t_ids) = transfer_ids {
            assert!(!t_ids.is_empty());
            target_set = t_ids.iter().copied().collect();
        }
        let check = || {
            let result = self.refresh_result(asset_id, &[]);
            if let Ok(refresh_res) = result {
                let mut non_fatal_error = false;
                refresh_res.iter().for_each(|(i, rt)| {
                    if let Some(ref e) = rt.failure {
                        eprintln!("refresh of {i} failure: {e} ({e:?})");
                        match e {
                            Error::Internal { details } => {
                                println!("refresh of {i} internal error: {e}, details: {details}");
                                non_fatal_error = true;
                            }
                            Error::InvalidTxid => {
                                println!("refresh of {i} invalid TXID: {e}");
                                non_fatal_error = true;
                            }
                            Error::Network { details } => {
                                println!("refresh of {i} network error: {e}, details: {details}");
                                non_fatal_error = true;
                            }
                            _ => panic!("refresh of {i} fatal error: {e}"),
                        }
                    }
                });
                if non_fatal_error {
                    return false;
                }
                if transfer_ids.is_some() {
                    for (id, rt) in refresh_res {
                        if rt.updated_status.is_some() && target_set.contains(&id) {
                            seen.insert(id);
                        }
                    }
                    if seen == target_set {
                        return true;
                    }
                } else if refresh_res.transfers_changed() {
                    return true;
                }
            } else {
                eprintln!("refresh error: {result:?}");
                return false;
            };
            false
        };
        if !wait_for_function(check, 10, 500) {
            panic!("transfer(s) are not refreshing");
        }
    }

    fn wait_for_unspents(&mut self, settled_only: bool, expected_len: u8) {
        println!("waiting for unspents");
        let mut unspents = vec![];
        let check = || {
            unspents = self.list_unspents_with_sync(settled_only);
            unspents.len() == expected_len as usize
        };
        if !wait_for_function(check, 10, 500) {
            panic!(
                "UTXO num {} is not becoming the expected {expected_len}",
                unspents.len()
            );
        }
    }
}

// shared singlesig-party test helpers that call inherent Wallet methods
// (which OfflineSigParty's RgbWalletOpsOffline bound can't reach)
pub(crate) trait SinglesigWalletParty {
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn abort_pending_vanilla_tx(&self, txid: &str);

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn abort_pending_vanilla_tx_result(&self, txid: &str) -> Result<(), Error>;

    fn blind_receive(&mut self) -> ReceiveData;

    fn blind_receive_asset_expiry(
        &mut self,
        asset_id: Option<String>,
        expiration: Option<u64>,
    ) -> ReceiveData;

    fn blind_receive_result(&mut self) -> Result<ReceiveData, Error>;

    fn blind_receive_with_endpoints(
        &mut self,
        expiration: Option<u64>,
        transport_endpoints: Vec<String>,
    ) -> ReceiveData;

    fn get_address(&mut self) -> String;

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn go_online(&mut self, skip_consistency_check: bool, indexer_url: Option<&str>) -> Online;

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn go_online_result(
        &mut self,
        skip_consistency_check: bool,
        indexer_url: Option<&str>,
    ) -> Result<Online, Error>;

    fn issue_asset_cfa(&mut self, amounts: Option<&[u64]>, file_path: Option<String>) -> AssetCFA;

    fn issue_asset_cfa_result(
        &mut self,
        amounts: Option<&[u64]>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, Error>;

    fn issue_asset_ifa(
        &mut self,
        amounts: Option<&[u64]>,
        inflation_amounts: Option<&[u64]>,
        reject_list_url: Option<String>,
    ) -> AssetIFA;

    fn issue_asset_ifa_result(
        &mut self,
        amounts: Option<&[u64]>,
        inflation_amounts: Option<&[u64]>,
        reject_list_url: Option<String>,
    ) -> Result<AssetIFA, Error>;

    fn issue_asset_nia(&mut self, amounts: Option<&[u64]>) -> AssetNIA;

    fn issue_asset_nia_result(&mut self, amounts: Option<&[u64]>) -> Result<AssetNIA, Error>;

    fn issue_asset_uda(
        &mut self,
        details: Option<&str>,
        media_file_path: Option<&str>,
        attachments_file_paths: Vec<&str>,
    ) -> AssetUDA;

    fn issue_asset_uda_result(
        &mut self,
        details: Option<&str>,
        media_file_path: Option<&str>,
        attachments_file_paths: Vec<&str>,
    ) -> Result<AssetUDA, Error>;

    fn witness_receive(&mut self) -> ReceiveData;
}

impl OfflineSigParty for OfflineSinglesigParty {
    type W = Wallet;

    fn wlt(&self) -> &Wallet {
        &self.wallet
    }

    fn wlt_mut(&mut self) -> &mut Wallet {
        &mut self.wallet
    }
}

impl OfflineSigParty for SinglesigParty {
    type W = Wallet;

    fn wlt(&self) -> &Wallet {
        &self.wallet
    }

    fn wlt_mut(&mut self) -> &mut Wallet {
        &mut self.wallet
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl SigParty for SinglesigParty {
    fn party_online(&self) -> Online {
        self.online
    }
}

impl<T: OfflineSigParty<W = Wallet>> SinglesigWalletParty for T {
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn abort_pending_vanilla_tx(&self, txid: &str) {
        self.abort_pending_vanilla_tx_result(txid).unwrap();
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn abort_pending_vanilla_tx_result(&self, txid: &str) -> Result<(), Error> {
        self.wlt().abort_pending_vanilla_tx(txid.to_string())
    }

    fn blind_receive(&mut self) -> ReceiveData {
        self.blind_receive_result().unwrap()
    }

    fn blind_receive_asset_expiry(
        &mut self,
        asset_id: Option<String>,
        expiration: Option<u64>,
    ) -> ReceiveData {
        self.wlt_mut()
            .blind_receive(
                asset_id,
                Assignment::Any,
                expiration,
                TRANSPORT_ENDPOINTS.clone(),
                MIN_CONFIRMATIONS,
            )
            .unwrap()
    }

    fn blind_receive_result(&mut self) -> Result<ReceiveData, Error> {
        self.wlt_mut().blind_receive(
            None,
            Assignment::Any,
            Some((now().unix_timestamp() + DURATION_RCV_TRANSFER as i64) as u64),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
    }

    fn blind_receive_with_endpoints(
        &mut self,
        expiration: Option<u64>,
        transport_endpoints: Vec<String>,
    ) -> ReceiveData {
        self.wlt_mut()
            .blind_receive(
                None,
                Assignment::Any,
                expiration,
                transport_endpoints,
                MIN_CONFIRMATIONS,
            )
            .unwrap()
    }

    fn get_address(&mut self) -> String {
        self.wlt_mut().get_address().unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn go_online(&mut self, skip_consistency_check: bool, indexer_url: Option<&str>) -> Online {
        self.go_online_result(skip_consistency_check, indexer_url)
            .unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn go_online_result(
        &mut self,
        skip_consistency_check: bool,
        indexer_url: Option<&str>,
    ) -> Result<Online, Error> {
        let mut online_options = test_go_online_options(indexer_url);
        online_options.skip_consistency_check = skip_consistency_check;
        self.wlt_mut().go_online(online_options)
    }

    fn issue_asset_cfa(&mut self, amounts: Option<&[u64]>, file_path: Option<String>) -> AssetCFA {
        self.issue_asset_cfa_result(amounts, file_path).unwrap()
    }

    fn issue_asset_cfa_result(
        &mut self,
        amounts: Option<&[u64]>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, Error> {
        let amounts = if let Some(a) = amounts {
            a.to_vec()
        } else {
            vec![AMOUNT]
        };
        self.wlt_mut().issue_asset_cfa(
            NAME.to_string(),
            Some(DETAILS.to_string()),
            PRECISION,
            amounts,
            file_path,
        )
    }

    fn issue_asset_ifa(
        &mut self,
        amounts: Option<&[u64]>,
        inflation_amounts: Option<&[u64]>,
        reject_list_url: Option<String>,
    ) -> AssetIFA {
        self.issue_asset_ifa_result(amounts, inflation_amounts, reject_list_url)
            .unwrap()
    }

    fn issue_asset_ifa_result(
        &mut self,
        amounts: Option<&[u64]>,
        inflation_amounts: Option<&[u64]>,
        reject_list_url: Option<String>,
    ) -> Result<AssetIFA, Error> {
        let amounts = if let Some(a) = amounts {
            a.to_vec()
        } else {
            vec![AMOUNT]
        };
        let inflation_amounts = if let Some(a) = inflation_amounts {
            a.to_vec()
        } else {
            vec![AMOUNT_INFLATION]
        };
        self.wlt_mut().issue_asset_ifa(
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            amounts,
            inflation_amounts,
            reject_list_url,
        )
    }

    fn issue_asset_nia(&mut self, amounts: Option<&[u64]>) -> AssetNIA {
        self.issue_asset_nia_result(amounts).unwrap()
    }

    fn issue_asset_nia_result(&mut self, amounts: Option<&[u64]>) -> Result<AssetNIA, Error> {
        let amounts = if let Some(a) = amounts {
            a.to_vec()
        } else {
            vec![AMOUNT]
        };
        self.wlt_mut()
            .issue_asset_nia(TICKER.to_string(), NAME.to_string(), PRECISION, amounts)
    }

    fn issue_asset_uda(
        &mut self,
        details: Option<&str>,
        media_file_path: Option<&str>,
        attachments_file_paths: Vec<&str>,
    ) -> AssetUDA {
        self.issue_asset_uda_result(details, media_file_path, attachments_file_paths)
            .unwrap()
    }

    fn issue_asset_uda_result(
        &mut self,
        details: Option<&str>,
        media_file_path: Option<&str>,
        attachments_file_paths: Vec<&str>,
    ) -> Result<AssetUDA, Error> {
        self.wlt_mut().issue_asset_uda(
            TICKER.to_string(),
            NAME.to_string(),
            details.map(|d| d.to_string()),
            PRECISION,
            media_file_path.map(|m| m.to_string()),
            attachments_file_paths
                .iter()
                .map(|a| a.to_string())
                .collect(),
        )
    }

    fn witness_receive(&mut self) -> ReceiveData {
        self.wlt_mut()
            .witness_receive(
                None,
                Assignment::Any,
                Some((now().unix_timestamp() + DURATION_RCV_TRANSFER as i64) as u64),
                TRANSPORT_ENDPOINTS.clone(),
                MIN_CONFIRMATIONS,
            )
            .unwrap()
    }
}

impl SinglesigParty {
    pub(crate) fn get_keys(&self) -> SinglesigKeys {
        self.wallet.get_keys()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn burn(&mut self, asset_id: &str, amount: u64) -> OperationResult {
        self.burn_result(asset_id, amount).unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn burn_begin(&mut self, asset_id: &str, amount: u64) -> String {
        self.burn_begin_result(asset_id, amount).unwrap().psbt
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn burn_begin_result(
        &mut self,
        asset_id: &str,
        amount: u64,
    ) -> Result<BurnBeginResult, Error> {
        self.wallet.burn_begin(
            self.online,
            asset_id.to_string(),
            amount,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            true,
        )
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn burn_result(
        &mut self,
        asset_id: &str,
        amount: u64,
    ) -> Result<OperationResult, Error> {
        self.wallet.burn(
            self.online,
            asset_id.to_string(),
            amount,
            FEE_RATE,
            MIN_CONFIRMATIONS,
        )
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn create_utxos(
        &mut self,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        expected: Option<u8>,
    ) {
        let unspents = self.list_unspents_with_sync(false);
        let colorable_before = unspents.iter().filter(|u| u.utxo.colorable).count();
        let expected = expected.unwrap_or(num.unwrap_or(UTXO_NUM));
        let _ = self
            .wallet
            .create_utxos(self.online, up_to, num, size, fee_rate, false)
            .unwrap();
        let check = || {
            let unspents = self.list_unspents_with_sync(false);
            let colorable = unspents.iter().filter(|u| u.utxo.colorable).count();
            if (colorable - colorable_before) == expected as usize {
                return true;
            }
            false
        };
        if !wait_for_function(check, 10, 500) {
            panic!(
                "created utxo number ({}) didn't match the expected one ({expected})",
                num.unwrap_or(UTXO_NUM)
            );
        }
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn create_utxos_begin_result(
        &mut self,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
    ) -> Result<String, Error> {
        self.wallet
            .create_utxos_begin(self.online, up_to, num, size, fee_rate, false, true)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn create_utxos_default(&mut self) {
        self.create_utxos(false, None, None, FEE_RATE, None);
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn drain_to(&mut self, address: &str) -> String {
        self.wallet
            .drain_to(self.online, address.to_string(), FEE_RATE)
            .unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn inflate_begin(&mut self, asset_id: &str, inflation_amounts: &[u64]) -> String {
        self.inflate_begin_result(asset_id, inflation_amounts)
            .unwrap()
            .psbt
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn inflate_begin_result(
        &mut self,
        asset_id: &str,
        inflation_amounts: &[u64],
    ) -> Result<InflateBeginResult, Error> {
        self.wallet.inflate_begin(
            self.online,
            asset_id.to_string(),
            inflation_amounts.to_vec(),
            FEE_RATE,
            MIN_CONFIRMATIONS,
            true,
        )
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn inflate(&mut self, asset_id: &str, inflation_amounts: &[u64]) -> OperationResult {
        self.inflate_result(asset_id, inflation_amounts).unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn inflate_result(
        &mut self,
        asset_id: &str,
        inflation_amounts: &[u64],
    ) -> Result<OperationResult, Error> {
        self.wallet.inflate(
            self.online,
            asset_id.to_string(),
            inflation_amounts.to_vec(),
            FEE_RATE,
            MIN_CONFIRMATIONS,
        )
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn list_unspents_vanilla(
        &mut self,
        min_confirmations: Option<u8>,
    ) -> Vec<LocalOutput> {
        let min_confirmations = min_confirmations.unwrap_or(MIN_CONFIRMATIONS);
        self.wallet
            .list_unspents_vanilla(self.online, min_confirmations, false)
            .unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn refresh_asset(&mut self, asset_id: &str) -> bool {
        self.refresh_result(Some(asset_id), &[])
            .unwrap()
            .transfers_changed()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn refresh_all(&mut self) -> bool {
        self.refresh_result(None, &[]).unwrap().transfers_changed()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn check_save_new_asset(
        &mut self,
        rcv_party: &mut SinglesigParty,
        asset_id: &str,
        assignment: Assignment,
    ) {
        let receive_data = rcv_party.witness_receive();
        let recipient_map = HashMap::from([(
            asset_id.to_owned(),
            vec![Recipient {
                assignment,
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        )]);
        let txid = self.send_retry(&recipient_map);
        assert!(!txid.is_empty());

        let consignment_path = self.wallet.get_send_consignment_path(asset_id, &txid);
        let consignment = RgbTransfer::load_file(consignment_path).unwrap();

        let contract = consignment.clone().into_contract();
        let asset_schema: AssetSchema = consignment.schema_id().try_into().unwrap();
        let validation_config = ValidationConfig {
            chain_net: rcv_party.wallet.chain_net(),
            trusted_typesystem: asset_schema.types(),
            ..Default::default()
        };
        let mut runtime = rcv_party.wallet.rgb_runtime().unwrap();
        let valid_contract = contract
            .clone()
            .validate(&DumbResolver, &validation_config)
            .unwrap();
        runtime
            .import_contract(valid_contract, rcv_party.wallet.blockchain_resolver())
            .unwrap();
        drop(runtime);

        rcv_party.wallet.save_new_asset(consignment, txid).unwrap();
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn send_retry(&mut self, recipient_map: &HashMap<String, Vec<Recipient>>) -> String {
        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        loop {
            if start.elapsed() > timeout {
                panic!("send failed")
            }
            let result = self.send_result(recipient_map);
            if let Err(e) = result {
                println!("send error: {e}");
                std::thread::sleep(Duration::from_millis(500));
                self.wallet
                    .sync(
                        self.online,
                        SyncOptions {
                            keychain: SyncKeychain::Colored,
                            strategy: SyncStrategy::FastSync,
                        },
                    )
                    .unwrap();
                continue;
            }
            break result.unwrap().txid;
        }
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn send_result(
        &mut self,
        recipient_map: &HashMap<String, Vec<Recipient>>,
    ) -> Result<OperationResult, Error> {
        self.wallet.send(
            self.online,
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            Some((now().unix_timestamp() + DURATION_SEND_TRANSFER as i64) as u64),
        )
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn send_begin_result(
        &mut self,
        recipient_map: &HashMap<String, Vec<Recipient>>,
    ) -> Result<SendBeginResult, Error> {
        self.wallet.send_begin(
            self.online,
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
            false,
        )
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn send_btc(&mut self, address: &str, amount: u64) -> String {
        self.send_btc_result(address, amount).unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn send_btc_result(&mut self, address: &str, amount: u64) -> Result<String, Error> {
        self.wallet
            .send_btc(self.online, address.to_string(), amount, FEE_RATE, false)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn inflate_end_result(
        &mut self,
        signed_psbt: &str,
    ) -> Result<OperationResult, Error> {
        self.wallet
            .inflate_end(self.online, signed_psbt.to_string())
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn drain_wallet(&mut self) {
        let mut rcv_wallet = get_test_wallet(false, None);
        self.drain_to(&rcv_wallet.get_address().unwrap());
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn drain_to_result(&mut self, address: &str) -> Result<String, Error> {
        self.wallet
            .drain_to(self.online, address.to_string(), FEE_RATE)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn send(
        &mut self,
        recipient_map: HashMap<String, Vec<Recipient>>,
        fee_rate: u64,
        expiration_timestamp: Option<u64>,
    ) -> OperationResult {
        self.wallet
            .send(
                self.online,
                recipient_map,
                false,
                fee_rate,
                MIN_CONFIRMATIONS,
                expiration_timestamp,
            )
            .unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn burn_end_result(&mut self, signed_psbt: &str) -> Result<OperationResult, Error> {
        self.wallet.burn_end(self.online, signed_psbt.to_string())
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn drain_to_begin_result(
        &mut self,
        address: &str,
        fee_rate: u64,
    ) -> Result<String, Error> {
        self.wallet
            .drain_to_begin(self.online, address.to_string(), fee_rate, true)
    }
}
