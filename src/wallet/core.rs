//! Core wallet functionality.
//!
//! This module defines abstractions to implement common methods across different wallet types.

use super::*;

const BDK_DB_NAME: &str = "bdk_db";

pub(crate) const NUM_KNOWN_SCHEMAS: usize = 4;

pub(crate) const RGB_LIB_DB_NAME: &str = "rgb_lib_db";

pub(crate) const ASSETS_DIR: &str = "assets";
pub(crate) const MEDIA_DIR: &str = "media_files";

/// Which keychain contributes SPKs to the sync request.
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncKeychain {
    /// Sync the colored keychain
    Colored,
    /// Sync the vanilla keychain
    Vanilla {
        /// Number of addresses preceding the lookback anchor (last used or, if none, last
        /// revealed) to scan
        lookback: u32,
    },
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl SyncKeychain {
    fn keychain(&self) -> KeychainKind {
        match self {
            SyncKeychain::Colored => KeychainKind::External,
            SyncKeychain::Vanilla { .. } => KeychainKind::Internal,
        }
    }
}

/// Strategy used to build the indexer sync request.
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStrategy {
    /// BIP44 stop-gap full scan
    FullScan,
    /// Sync all revealed SPKs
    FullSync,
    /// Sync only SPKs we strictly need to observe:
    /// - colored: SPKs used in pending transfers or unconfirmed transactions
    /// - vanilla: a tail of recently revealed SPKs
    FastSync,
}

/// Options driving a single sync invocation.
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncOptions {
    /// Which keychain to sync
    pub keychain: SyncKeychain,
    /// Sync strategy
    pub strategy: SyncStrategy,
}

pub struct WalletInternals {
    pub(crate) wallet_data: WalletData,
    pub(crate) logger: Logger,
    pub(crate) _logger_guard: AsyncGuard,
    pub(crate) database: Arc<RgbLibDatabase>,
    pub(crate) wallet_dir: PathBuf,
    pub(crate) bdk_wallet: PersistedWallet<Store<ChangeSet>>,
    pub(crate) bdk_database: Store<ChangeSet>,
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) online_data: Option<OnlineData>,
}

pub(crate) fn setup_rgb<P: AsRef<Path>>(
    wallet_dir: P,
    supported_schemas: Vec<AssetSchema>,
    bitcoin_network: BitcoinNetwork,
) -> Result<(), Error> {
    if supported_schemas.is_empty() {
        return Err(Error::NoSupportedSchemas);
    }
    if bitcoin_network == BitcoinNetwork::Mainnet && supported_schemas.contains(&AssetSchema::Ifa) {
        return Err(Error::CannotUseIfaOnMainnet);
    }
    let mut runtime = load_rgb_runtime(wallet_dir)?;
    let known_schemas = runtime.schemata()?;
    if known_schemas.len() < NUM_KNOWN_SCHEMAS {
        let known: HashSet<_> = known_schemas.iter().map(|s| s.id).collect();
        for schema in supported_schemas {
            if !known.contains(&SchemaId::from(schema)) {
                schema.import_kit(&mut runtime)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn setup_db<P: AsRef<Path>>(wallet_dir: P) -> Result<RgbLibDatabase, Error> {
    let db_path = wallet_dir.as_ref().join(RGB_LIB_DB_NAME);
    let display_db_path = adjust_canonicalization(db_path);
    let connection_string = format!("sqlite:{display_db_path}?mode=rwc");
    let mut opt = ConnectOptions::new(connection_string);
    opt.max_connections(1)
        .min_connections(0)
        .connect_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(8))
        .max_lifetime(Duration::from_secs(8));
    let db_cnn = block_on(Database::connect(opt));
    let connection = db_cnn?;
    block_on(Migrator::up(&connection, None))?;
    Ok(RgbLibDatabase::new(connection))
}

pub(crate) fn setup_bdk<P: AsRef<Path>>(
    wallet_data: &WalletData,
    wallet_dir: P,
    desc_colored: String,
    desc_vanilla: String,
    watch_only: bool,
    bdk_network: BdkNetwork,
) -> Result<(PersistedWallet<Store<ChangeSet>>, Store<ChangeSet>), Error> {
    let chain_net: ChainNet = wallet_data.bitcoin_network.into();
    let mut wallet_params = BdkWallet::load()
        .descriptor(KeychainKind::External, Some(desc_colored.clone()))
        .descriptor(KeychainKind::Internal, Some(desc_vanilla.clone()))
        .check_genesis_hash(BlockHash::from_byte_array(
            chain_net.chain_hash().to_bytes(),
        ));
    let bdk_db_name = if watch_only {
        format!("{BDK_DB_NAME}_watch_only")
    } else {
        wallet_params = wallet_params.extract_keys();
        BDK_DB_NAME.to_string()
    };
    let bdk_db_path = wallet_dir.as_ref().join(bdk_db_name);
    let (mut bdk_database, _) =
        Store::<ChangeSet>::load_or_create(BDK_DB_NAME.as_bytes(), bdk_db_path)?;
    let bdk_wallet = match wallet_params.load_wallet(&mut bdk_database)? {
        Some(wallet) => wallet,
        None => BdkWallet::create(desc_colored, desc_vanilla)
            .network(bdk_network)
            .create_wallet(&mut bdk_database)?,
    };
    Ok((bdk_wallet, bdk_database))
}

pub(crate) fn setup_new_wallet(
    wallet_data: &WalletData,
    fingerprint: &str,
) -> Result<(PathBuf, Logger, AsyncGuard), Error> {
    let data_dir_path = Path::new(&wallet_data.data_dir);
    if !data_dir_path.exists() {
        return Err(Error::InexistentDataDir);
    }
    let data_dir_path = fs::canonicalize(data_dir_path)?;
    let wallet_dir = data_dir_path.join(fingerprint);
    if !wallet_dir.exists() {
        fs::create_dir(&wallet_dir)?;
        fs::create_dir(wallet_dir.join(ASSETS_DIR))?;
        fs::create_dir(wallet_dir.join(MEDIA_DIR))?;
    }
    let (logger, logger_guard) = setup_logger(&wallet_dir, None)?;
    info!(logger.clone(), "New wallet in '{:?}'", wallet_dir);
    let panic_logger = logger.clone();
    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        error!(panic_logger.clone(), "PANIC: {:?}", info);
        prev_hook(info);
    }));
    Ok((wallet_dir, logger, logger_guard))
}

pub trait WalletCore {
    fn internals(&self) -> &WalletInternals;

    fn internals_mut(&mut self) -> &mut WalletInternals;

    fn bdk_wallet(&self) -> &PersistedWallet<Store<ChangeSet>> {
        &self.internals().bdk_wallet
    }

    fn bdk_wallet_mut(&mut self) -> &mut PersistedWallet<Store<ChangeSet>> {
        &mut self.internals_mut().bdk_wallet
    }

    fn bdk_wallet_db_mut(
        &mut self,
    ) -> (
        &mut PersistedWallet<Store<ChangeSet>>,
        &mut Store<ChangeSet>,
    ) {
        let internals_mut = self.internals_mut();
        (
            &mut internals_mut.bdk_wallet,
            &mut internals_mut.bdk_database,
        )
    }

    fn database(&self) -> &RgbLibDatabase {
        &self.internals().database
    }

    fn logger(&self) -> &Logger {
        &self.internals().logger
    }

    fn wallet_data(&self) -> &WalletData {
        &self.internals().wallet_data
    }

    fn wallet_dir(&self) -> &PathBuf {
        &self.internals().wallet_dir
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn online_data(&self) -> &Option<OnlineData> {
        &self.internals().online_data
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn online_data_mut(&mut self) -> &mut Option<OnlineData> {
        &mut self.internals_mut().online_data
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn indexer(&self) -> &Indexer {
        &self.online_data().as_ref().unwrap().indexer
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn vanilla_sync_lookback(&self) -> u32 {
        self.online_data().as_ref().unwrap().vanilla_sync_lookback
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn check_online(&self, online: Online) -> Result<(), Error> {
        if let Some(online_data) = &self.online_data() {
            if online_data.id != online.id {
                error!(self.logger(), "Cannot change online object");
                return Err(Error::CannotChangeOnline);
            }
        } else {
            error!(self.logger(), "Wallet is offline");
            return Err(Error::Offline);
        }
        Ok(())
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn fast_sync_colored_spks(&self, txn: &DbTxn) -> Result<HashSet<ScriptBuf>, Error> {
        let mut spks: HashSet<ScriptBuf> = HashSet::new();
        for pws in txn.iter_pending_witness_scripts()? {
            spks.insert(ScriptBuf::from_hex(&pws.script).expect("valid script"));
        }
        Ok(spks)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn fast_sync_vanilla_spks(&self, lookback: u32) -> HashSet<ScriptBuf> {
        let spk_index = self.bdk_wallet().spk_index();
        let Some(last_revealed) = spk_index.last_revealed_index(KeychainKind::Internal) else {
            return HashSet::new();
        };
        let lookback_anchor = spk_index
            .last_used_index(KeychainKind::Internal)
            .unwrap_or(last_revealed);
        let start = lookback_anchor.saturating_sub(lookback);
        spk_index
            .revealed_keychain_spks(KeychainKind::Internal)
            .filter(|(i, _)| *i >= start && *i <= last_revealed)
            .map(|(_, spk)| spk)
            .collect()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn unconfirmed_colored_spks(&self) -> HashSet<ScriptBuf> {
        let spk_index = self.bdk_wallet().spk_index();
        let mut spks: HashSet<ScriptBuf> = HashSet::new();
        for tx in self
            .bdk_wallet()
            .transactions()
            .filter(|tx| matches!(tx.chain_position, ChainPosition::Unconfirmed { .. }))
        {
            // first input is enough for the indexer's to return the TX info
            for input in tx.tx_node.tx.input.iter() {
                if let Some(((kc, _), txout)) = spk_index.txout(input.previous_output)
                    && kc == KeychainKind::External
                {
                    spks.insert(txout.script_pubkey.clone());
                    break;
                }
            }
        }
        spks
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn sync_bdk_and_db_txos(
        &mut self,
        txn: &DbTxn,
        options: SyncOptions,
        include_spent: bool,
    ) -> Result<(), Error> {
        debug!(self.logger(), "Syncing {:?}...", options);

        let kc = options.keychain.keychain();
        let latest_checkpoint = self.bdk_wallet().latest_checkpoint();
        let update: Update = match options.strategy {
            SyncStrategy::FullScan => {
                let mut iters = self.bdk_wallet().spk_index().all_unbounded_spk_iters();
                let iter = iters.remove(&kc).expect("keychain must exist");
                let request = FullScanRequest::builder()
                    .chain_tip(latest_checkpoint)
                    .spks_for_keychain(kc, iter);
                self.indexer().full_scan(request)?.into()
            }
            SyncStrategy::FullSync => {
                let spks: Vec<ScriptBuf> = self
                    .bdk_wallet()
                    .spk_index()
                    .revealed_keychain_spks(kc)
                    .map(|(_, spk)| spk)
                    .collect();
                let request = SyncRequest::builder()
                    .chain_tip(latest_checkpoint)
                    .spks(spks);
                self.indexer().sync(request)?.into()
            }
            SyncStrategy::FastSync => {
                let mut spks: HashSet<ScriptBuf> = HashSet::new();
                match options.keychain {
                    SyncKeychain::Colored => {
                        spks.extend(self.fast_sync_colored_spks(txn)?);
                        spks.extend(self.unconfirmed_colored_spks());
                    }
                    SyncKeychain::Vanilla { lookback } => {
                        spks.extend(self.fast_sync_vanilla_spks(lookback));
                    }
                }
                let request = SyncRequest::builder()
                    .chain_tip(latest_checkpoint)
                    .spks(spks);
                self.indexer().sync(request)?.into()
            }
        };
        let (bdk_wallet, bdk_db) = self.bdk_wallet_db_mut();
        bdk_wallet
            .apply_update(update)
            .map_err(|e| Error::FailedBdkSync {
                details: e.to_string(),
            })?;
        bdk_wallet.persist(bdk_db)?;

        if matches!(options.keychain, SyncKeychain::Colored) {
            self.update_db_colored_txos_from_bdk(txn, include_spent)?;
        }

        debug!(self.logger(), "Synced");
        Ok(())
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn update_db_colored_txos_from_bdk(
        &mut self,
        txn: &DbTxn,
        include_spent: bool,
    ) -> Result<(), Error> {
        let db_txos = txn.iter_txos()?;

        let db_outpoints: HashSet<String> = db_txos
            .into_iter()
            .filter(|t| t.exists && (include_spent || !t.spent))
            .map(|u| u.outpoint().to_string())
            .collect();

        let pending_witness_scripts: Vec<String> = txn
            .iter_pending_witness_scripts()?
            .into_iter()
            .map(|s| s.script)
            .collect();

        let iter: Box<dyn Iterator<Item = LocalOutput>> = if include_spent {
            Box::new(self.bdk_wallet().list_output())
        } else {
            Box::new(self.bdk_wallet().list_unspent())
        };

        for new_utxo in iter
            .filter(|u| u.keychain == KeychainKind::External)
            .filter(|u| !db_outpoints.contains(&u.outpoint.to_string()))
        {
            let mut new_db_utxo: DbTxoActMod = new_utxo.clone().into();
            if !pending_witness_scripts.is_empty() {
                let pending_witness_script = new_utxo.txout.script_pubkey.to_hex_string();
                if pending_witness_scripts.contains(&pending_witness_script) {
                    new_db_utxo.pending_witness = ActiveValue::Set(true);
                    txn.del_pending_witness_script(pending_witness_script)?;
                }
            }
            txn.set_txo(new_db_utxo.clone())?;
        }

        Ok(())
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn sync_wallet(
        &mut self,
        txn: &DbTxn,
        options: SyncOptions,
        include_spent: bool,
    ) -> Result<(), Error> {
        self.sync_bdk_and_db_txos(txn, options, include_spent)
    }
}
