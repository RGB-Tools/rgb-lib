//! Core wallet functionality.
//!
//! This module defines abstractions to implement common methods across different wallet types.

use super::*;

const BDK_DB_NAME: &str = "bdk_db_sqlite";

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
        fs::write(Self::path(wallet_dir), json)?;
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
    pub(crate) bdk_wallet: PersistedWallet<BdkStore>,
    pub(crate) bdk_database: BdkStore,
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
) -> Result<(PersistedWallet<BdkStore>, BdkStore), Error> {
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
    let bdk_db_url = format!("sqlite:{}", adjust_canonicalization(bdk_db_path));
    let mut bdk_database = block_on(BdkStore::new(&bdk_db_url))?;
    // schema migrations are applied automatically when the wallet is loaded
    let bdk_wallet = match block_on(wallet_params.load_wallet_async(&mut bdk_database))? {
        Some(wallet) => wallet,
        None => block_on(
            BdkWallet::create(desc_colored, desc_vanilla)
                .network(bdk_network)
                .create_wallet_async(&mut bdk_database),
        )?,
    };
    Ok((bdk_wallet, bdk_database))
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

    fn bdk_wallet(&self) -> &PersistedWallet<BdkStore> {
        &self.internals().bdk_wallet
    }

    fn bdk_wallet_mut(&mut self) -> &mut PersistedWallet<BdkStore> {
        &mut self.internals_mut().bdk_wallet
    }

    fn bdk_wallet_db_mut(&mut self) -> (&mut PersistedWallet<BdkStore>, &mut BdkStore) {
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
        block_on(bdk_wallet.persist_async(bdk_db))?;

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
