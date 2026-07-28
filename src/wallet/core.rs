//! Core wallet functionality.
//!
//! This module defines abstractions to implement common methods across different wallet types.

use super::*;

// BDK file store of wallets created before the changeset moved into the rgb-lib DB
#[cfg(feature = "bdk_file_store_migration")]
pub(crate) const BDK_DB_NAME: &str = "bdk_db";
// BDK changes that have been applied in memory but whose transaction has not committed yet
pub(crate) const BDK_PENDING_FILE: &str = "bdk_pending.json";

pub(crate) const NUM_KNOWN_SCHEMAS: usize = 4;

pub(crate) const RGB_LIB_DB_NAME: &str = "rgb_lib_db";

pub(crate) const ASSETS_DIR: &str = "assets";
pub(crate) const MEDIA_DIR: &str = "media_files";

pub(crate) const WALLET_MANIFEST_FILE: &str = "wallet_manifest.json";
pub(crate) const WALLET_MANIFEST_VERSION: u8 = 1;

// Only the version field, so an unsupported manifest reports its version instead of failing to
// deserialize.
#[derive(Deserialize)]
struct WalletManifestVersion {
    version: u8,
}

// The non-secret parts of a WalletData and a SinglesigKeys, persisted inside the wallet
// directory so the wallet can be re-opened via Wallet::load without re-supplying them.
//
// The mnemonic must never be stored here: it's the wallet's only secret and the manifest sits in
// plaintext next to the databases.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct WalletManifest {
    pub(crate) version: u8,
    pub(crate) bitcoin_network: BitcoinNetwork,
    pub(crate) database_type: DatabaseType,
    pub(crate) max_allocations_per_utxo: u32,
    pub(crate) supported_schemas: Vec<AssetSchema>,
    pub(crate) account_xpub_vanilla: String,
    pub(crate) account_xpub_colored: String,
    pub(crate) vanilla_keychain: u8,
    pub(crate) master_fingerprint: String,
    pub(crate) witness_version: WitnessVersion,
}

impl WalletManifest {
    pub(crate) fn new(wallet_data: &WalletData, keys: &SinglesigKeys) -> Self {
        Self {
            version: WALLET_MANIFEST_VERSION,
            bitcoin_network: wallet_data.bitcoin_network,
            database_type: wallet_data.database_type.clone(),
            max_allocations_per_utxo: wallet_data.max_allocations_per_utxo,
            supported_schemas: wallet_data.supported_schemas.clone(),
            account_xpub_vanilla: keys.account_xpub_vanilla.clone(),
            account_xpub_colored: keys.account_xpub_colored.clone(),
            vanilla_keychain: keys.vanilla_keychain.unwrap_or(KEYCHAIN_BTC),
            master_fingerprint: keys.master_fingerprint.clone(),
            witness_version: keys.witness_version,
        }
    }

    fn path(wallet_dir: &Path) -> PathBuf {
        wallet_dir.join(WALLET_MANIFEST_FILE)
    }

    pub(crate) fn write(&self, wallet_dir: &Path) -> Result<(), Error> {
        let json = serde_json::to_string_pretty(self).map_err(InternalError::from)?;
        atomic_write(&Self::path(wallet_dir), json.as_bytes())?;
        Ok(())
    }

    pub(crate) fn read(wallet_dir: &Path) -> Result<Self, Error> {
        let manifest_path = Self::path(wallet_dir);
        if !manifest_path.exists() {
            return Err(Error::InexistentWalletManifest {
                path: manifest_path.to_string_lossy().to_string(),
            });
        }
        let json = fs::read_to_string(&manifest_path)?;
        let manifest_version: WalletManifestVersion =
            serde_json::from_str(&json).map_err(InternalError::from)?;
        if manifest_version.version != WALLET_MANIFEST_VERSION {
            return Err(Error::UnsupportedWalletManifestVersion {
                version: manifest_version.version.to_string(),
            });
        }
        serde_json::from_str(&json).map_err(|e| InternalError::from(e).into())
    }

    // Fail if wallet_data or keys disagree with settings fixed at wallet creation. Settings that
    // are allowed to change are not checked on purpose.
    pub(crate) fn check_settings_unchanged(
        wallet_dir: &Path,
        wallet_data: &WalletData,
        keys: &SinglesigKeys,
    ) -> Result<(), Error> {
        if !Self::path(wallet_dir).exists() {
            // skip when no manifest exists (legacy directory or first creation)
            return Ok(());
        }
        let created_with = Self::read(wallet_dir)?;
        let requested = Self::new(wallet_data, keys);

        if created_with.bitcoin_network != requested.bitcoin_network {
            return Err(Error::BitcoinNetworkMismatch);
        }

        macro_rules! check {
            ($($field:ident),+ $(,)?) => {
                $(if created_with.$field != requested.$field {
                    return Err(Error::WalletSettingMismatch {
                        setting: stringify!($field).to_string(),
                        expected: format!("{:?}", created_with.$field),
                        provided: format!("{:?}", requested.$field),
                    });
                })+
            };
        }

        // ordered so the root cause is reported ahead of what it derives: a changed witness
        // version also changes the account xpubs it produces
        check!(
            master_fingerprint,
            witness_version,
            vanilla_keychain,
            account_xpub_colored,
            account_xpub_vanilla,
        );
        Ok(())
    }

    pub(crate) fn into_parts(
        self,
        data_dir: String,
        mnemonic: Option<String>,
    ) -> (WalletData, SinglesigKeys) {
        (
            WalletData {
                data_dir,
                bitcoin_network: self.bitcoin_network,
                database_type: self.database_type,
                max_allocations_per_utxo: self.max_allocations_per_utxo,
                supported_schemas: self.supported_schemas,
            },
            SinglesigKeys {
                account_xpub_vanilla: self.account_xpub_vanilla,
                account_xpub_colored: self.account_xpub_colored,
                vanilla_keychain: Some(self.vanilla_keychain),
                master_fingerprint: self.master_fingerprint,
                mnemonic,
                witness_version: self.witness_version,
            },
        )
    }
}

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
    pub(crate) bdk_wallet: BdkWallet,
    pub(crate) bdk_pending: Arc<Mutex<ChangeSet>>,
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

/// Import the BDK data of a wallet created before the changeset moved into the rgb-lib DB.
///
/// Such wallets keep their data in `bdk_file_store` files inside the wallet directory. Chain data
/// could be rebuilt by a rescan, but the revealed-address indices could not: a rescan only
/// restores up to the last *used* index, so an address that was revealed and never paid would be
/// handed out again. The signing and watch-only wallets had a store each, so both are imported;
/// `last_revealed` is persisted monotonically, so the higher of the two wins regardless of order.
///
/// A store that was read in full is removed once the import is durably committed, so the stale
/// copy cannot be picked up again by an older rgb-lib. A truncated one is left in place, since the
/// entries past the truncation were never imported. Keeping it costs nothing at load time: this
/// runs only while the stored changeset is empty, and by the end of the first setup it no longer
/// is (either the import filled it or the freshly created wallet wrote its descriptors) so a
/// left-behind store is never read a second time.
///
/// Returns whether anything was imported.
#[cfg(feature = "bdk_file_store_migration")]
fn import_legacy_bdk_store(txn: &DbTxn, wallet_dir: &Path) -> Result<bool, Error> {
    let mut imported = false;
    for name in [BDK_DB_NAME.to_string(), format!("{BDK_DB_NAME}_watch_only")] {
        let path = wallet_dir.join(name);
        if !path.exists() {
            continue;
        }
        // a truncated trailing entry still leaves the earlier ones usable, so take the partial
        // dump rather than failing the whole import
        let (changeset, complete) = match Store::<ChangeSet>::load(BDK_DB_NAME.as_bytes(), &path) {
            Ok((_, changeset)) => (changeset.map(Box::new), true),
            Err(e) => {
                if e.changeset.is_none() {
                    return Err(Error::Internal {
                        details: format!("cannot read legacy BDK store {path:?}: {}", e.error),
                    });
                }
                (e.changeset, false)
            }
        };
        if let Some(changeset) = changeset {
            txn.update_bdk_changeset(&changeset)?;
            imported = true;
        }
        // drop the file only once its contents are durably in the DB
        if complete {
            txn.on_commit(move || {
                let _ = fs::remove_file(path);
            });
        }
    }
    Ok(imported)
}

/// Without the `bdk_file_store_migration` feature there is nothing to import from.
#[cfg(not(feature = "bdk_file_store_migration"))]
fn import_legacy_bdk_store(_txn: &DbTxn, _wallet_dir: &Path) -> Result<bool, Error> {
    Ok(false)
}

pub(crate) fn setup_bdk<P: AsRef<Path>>(
    txn: &DbTxn,
    wallet_dir: P,
    desc_colored: String,
    desc_vanilla: String,
    watch_only: bool,
    bitcoin_network: BitcoinNetwork,
) -> Result<BdkWallet, Error> {
    let chain_net: ChainNet = bitcoin_network.into();
    let mut wallet_params = BdkWallet::load()
        .descriptor(KeychainKind::External, Some(desc_colored.clone()))
        .descriptor(KeychainKind::Internal, Some(desc_vanilla.clone()))
        .use_spk_cache(false)
        .check_genesis_hash(BlockHash::from_byte_array(
            chain_net.chain_hash().to_bytes(),
        ));
    if !watch_only {
        wallet_params = wallet_params.extract_keys();
    }
    let mut changeset = txn.get_bdk_changeset()?;
    // an empty changeset means either a brand-new wallet or one whose data still lives in the
    // legacy file store
    let mut reload = changeset.is_empty() && import_legacy_bdk_store(txn, wallet_dir.as_ref())?;
    // a crash between the temporary write and the rename leaves the newer changeset in the .tmp
    // file, so fold both in; the pending buffer only ever grows, so applying them in this order
    // ends on the newest state
    let pending_path = wallet_dir.as_ref().join(BDK_PENDING_FILE);
    let pending_tmp_path = atomic_tmp_path(&pending_path)?;
    for pending_file in [pending_path, pending_tmp_path] {
        if !pending_file.exists() {
            continue;
        }
        // a file that fails to parse was still being written when the crash happened, so its
        // contents were never complete: drop it rather than refusing to open the wallet
        if let Ok(pending) = serde_json::from_slice::<ChangeSet>(&fs::read(&pending_file)?) {
            txn.update_bdk_changeset(&pending)?;
            reload = true;
        }
        // drop the file only once its contents are durably in the DB
        txn.on_commit(move || {
            let _ = fs::remove_file(pending_file);
        });
    }
    if reload {
        changeset = txn.get_bdk_changeset()?;
    }
    let bdk_wallet = match wallet_params.load_wallet_no_persist(changeset)? {
        Some(wallet) => wallet,
        None => {
            let mut wallet = BdkWallet::create(desc_colored, desc_vanilla)
                .network(BdkNetwork::from(bitcoin_network))
                .use_spk_cache(false)
                .create_wallet_no_persist()
                .map_err(InternalError::from)?;
            if let Some(changeset) = wallet.take_staged() {
                txn.update_bdk_changeset(&changeset)?;
            }
            wallet
        }
    };
    Ok(bdk_wallet)
}

pub(crate) fn setup_new_wallet(
    wallet_data: &WalletData,
    fingerprint: &str,
) -> Result<(PathBuf, Logger, AsyncGuard), Error> {
    if wallet_data.max_allocations_per_utxo == 0 {
        return Err(Error::NoMaxAllocationsPerUtxo);
    }
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

    fn bdk_wallet(&self) -> &BdkWallet {
        &self.internals().bdk_wallet
    }

    fn bdk_wallet_mut(&mut self) -> &mut BdkWallet {
        &mut self.internals_mut().bdk_wallet
    }

    /// Persist the BDK wallet's staged changes through the given rgb-lib transaction.
    ///
    /// The changes are moved out of BDK's stage into `bdk_pending` and only dropped once `txn`
    /// commits, so neither a failed write nor a rollback can lose them: the next `persist_bdk`
    /// writes them again (the mapping is made of upserts, so re-writing is idempotent). This
    /// mirrors [`PersistedWallet::persist`](bdk_wallet::PersistedWallet), where the persister
    /// write is itself the commit.
    fn persist_bdk(&mut self, txn: &DbTxn) -> Result<(), Error> {
        let staged = self.bdk_wallet_mut().take_staged();
        let pending = Arc::clone(&self.internals().bdk_pending);
        {
            let mut guard = pending.lock().expect("bdk_pending is never poisoned");
            if let Some(staged) = staged {
                guard.merge(staged);
            }
            if guard.is_empty() {
                return Ok(());
            }
            txn.update_bdk_changeset(&guard)?;
        }
        let pending_file = self.wallet_dir().join(BDK_PENDING_FILE);
        txn.on_commit(move || {
            pending
                .lock()
                .expect("bdk_pending is never poisoned")
                .take();
            // the changes are in the DB now, so the crash-recovery copy is no longer needed
            let _ = fs::remove_file(pending_file);
        });
        Ok(())
    }

    /// Write the pending BDK changes to disk, outside the rgb-lib transaction.
    ///
    /// The file is read back by [`setup_bdk`] and removed once its contents reach the DB. So is
    /// the temporary file, in case a crash landed between the write and the rename.
    fn flush_bdk_pending(&mut self) -> Result<(), Error> {
        let staged = self.bdk_wallet_mut().take_staged();
        let pending = Arc::clone(&self.internals().bdk_pending);
        let serialized = {
            let mut guard = pending.lock().expect("bdk_pending is never poisoned");
            if let Some(staged) = staged {
                guard.merge(staged);
            }
            if guard.is_empty() {
                return Ok(());
            }
            serde_json::to_vec(&*guard).map_err(InternalError::from)?
        };
        // a crash mid-write cannot leave a half-written file, which would fail to parse on reload
        // and leave the wallet unopenable
        atomic_write(&self.wallet_dir().join(BDK_PENDING_FILE), &serialized)
    }

    /// Persist any pending BDK changes and commit the transaction.
    ///
    /// This is an operation's single persist point: BDK changes accumulate in memory while the
    /// operation runs and reach the DB once, in the same commit as the rgb-lib changes they belong
    /// to. Operations that cannot touch BDK take `&self` and commit the transaction directly.
    fn persist_and_commit(&mut self, txn: DbTxn) -> Result<(), Error> {
        self.persist_bdk(&txn)?;
        txn.commit()
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
        self.bdk_wallet_mut()
            .apply_update(update)
            .map_err(|e| Error::FailedBdkSync {
                details: e.to_string(),
            })?;

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
