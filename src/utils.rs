//! RGB utilities
//!
//! This module defines some utility methods.

use super::*;

const TIMESTAMP_FORMAT: &[time::format_description::BorrowedFormatItem] = time::macros::format_description!(
    "[year]-[month]-[day]T[hour repr:24]:[minute]:[second].[subsecond digits:3]+00"
);

const RGB_RUNTIME_LOCK_FILE: &str = "rgb_runtime.lock";

pub(crate) const RGB_RUNTIME_DIR: &str = "rgb";

pub(crate) const LOG_FILE: &str = "log";
pub(crate) const PURPOSE: u8 = 86;
pub(crate) const ACCOUNT: u8 = 0;

/// Supported Bitcoin networks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum BitcoinNetwork {
    /// Bitcoin's mainnet
    Mainnet,
    /// Bitcoin's testnet
    Testnet,
    /// Bitcoin's signet
    Signet,
    /// Bitcoin's regtest
    Regtest,
}

impl fmt::Display for BitcoinNetwork {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for BitcoinNetwork {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "mainnet" | "bitcoin" => BitcoinNetwork::Mainnet,
            "testnet" | "testnet3" => BitcoinNetwork::Testnet,
            "regtest" => BitcoinNetwork::Regtest,
            "signet" => BitcoinNetwork::Signet,
            _ => {
                return Err(Error::InvalidBitcoinNetwork {
                    network: s.to_string(),
                })
            }
        })
    }
}

impl From<BdkNetwork> for BitcoinNetwork {
    fn from(x: BdkNetwork) -> BitcoinNetwork {
        match x {
            BdkNetwork::Bitcoin => BitcoinNetwork::Mainnet,
            BdkNetwork::Testnet => BitcoinNetwork::Testnet,
            BdkNetwork::Signet => BitcoinNetwork::Signet,
            BdkNetwork::Regtest => BitcoinNetwork::Regtest,
            _ => unimplemented!("this should not be possible"),
        }
    }
}

impl TryFrom<ChainNet> for BitcoinNetwork {
    type Error = Error;

    fn try_from(x: ChainNet) -> Result<Self, Self::Error> {
        match x {
            ChainNet::BitcoinMainnet => Ok(BitcoinNetwork::Mainnet),
            ChainNet::BitcoinTestnet => Ok(BitcoinNetwork::Testnet),
            ChainNet::BitcoinSignet => Ok(BitcoinNetwork::Signet),
            ChainNet::BitcoinRegtest => Ok(BitcoinNetwork::Regtest),
            _ => Err(Error::UnsupportedLayer1 {
                layer_1: x.layer1().to_string(),
            }),
        }
    }
}

impl From<BitcoinNetwork> for bitcoin::Network {
    fn from(x: BitcoinNetwork) -> bitcoin::Network {
        match x {
            BitcoinNetwork::Mainnet => bitcoin::Network::Bitcoin,
            BitcoinNetwork::Testnet => bitcoin::Network::Testnet,
            BitcoinNetwork::Signet => bitcoin::Network::Signet,
            BitcoinNetwork::Regtest => bitcoin::Network::Regtest,
        }
    }
}

impl From<BitcoinNetwork> for ChainNet {
    fn from(x: BitcoinNetwork) -> ChainNet {
        match x {
            BitcoinNetwork::Mainnet => ChainNet::BitcoinMainnet,
            BitcoinNetwork::Testnet => ChainNet::BitcoinTestnet,
            BitcoinNetwork::Signet => ChainNet::BitcoinSignet,
            BitcoinNetwork::Regtest => ChainNet::BitcoinRegtest,
        }
    }
}

impl From<BitcoinNetwork> for RgbNetwork {
    fn from(x: BitcoinNetwork) -> RgbNetwork {
        match x {
            BitcoinNetwork::Mainnet => RgbNetwork::Mainnet,
            BitcoinNetwork::Testnet => RgbNetwork::Testnet3,
            BitcoinNetwork::Signet => RgbNetwork::Signet,
            BitcoinNetwork::Regtest => RgbNetwork::Regtest,
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn adjust_canonicalization<P: AsRef<Path>>(p: P) -> String {
    p.as_ref().display().to_string()
}

#[cfg(target_os = "windows")]
pub(crate) fn adjust_canonicalization<P: AsRef<Path>>(p: P) -> String {
    const VERBATIM_PREFIX: &str = r#"\\?\"#;
    let p = p.as_ref().display().to_string();
    if p.starts_with(VERBATIM_PREFIX) {
        p[VERBATIM_PREFIX.len()..].to_string()
    } else {
        p
    }
}

#[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
pub(crate) fn get_genesis_hash(bitcoin_network: &BitcoinNetwork) -> &str {
    match bitcoin_network {
        BitcoinNetwork::Mainnet => {
            "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"
        }
        BitcoinNetwork::Testnet => {
            "000000000933ea01ad0ee984209779baaec3ced90fa3f408719526f8d77f4943"
        }
        BitcoinNetwork::Signet => {
            "00000008819873e925422c1ff0f99f7cc9bbb232af63a077a480a3633bee1ef6"
        }
        BitcoinNetwork::Regtest => {
            "0f9188f13cb7b2c71f2a335e3a4fc328bf5beb436012afca590b1a11466e2206"
        }
    }
}

#[cfg_attr(not(feature = "electrum"), allow(dead_code))]
pub(crate) fn get_valid_txid_for_network(bitcoin_network: &BitcoinNetwork) -> String {
    match bitcoin_network {
        BitcoinNetwork::Mainnet => {
            "33e794d097969002ee05d336686fc03c9e15a597c1b9827669460fac98799036"
        }
        BitcoinNetwork::Testnet => {
            "5e6560fd518aadbed67ee4a55bdc09f19e619544f5511e9343ebba66d2f62653"
        }
        BitcoinNetwork::Signet => {
            "8153034f45e695453250a8fb7225a5e545144071d8ed7b0d3211efa1f3c92ad8"
        }
        BitcoinNetwork::Regtest => {
            "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"
        }
    }
    .to_string()
}

fn get_coin_type(bitcoin_network: BitcoinNetwork) -> u32 {
    u32::from(bitcoin_network != BitcoinNetwork::Mainnet)
}

pub(crate) fn derive_account_xprv_from_mnemonic(
    bitcoin_network: BitcoinNetwork,
    mnemonic: &str,
) -> Result<ExtendedPrivKey, Error> {
    let coin_type = get_coin_type(bitcoin_network);
    let account_derivation_path = vec![
        ChildNumber::from_hardened_idx(PURPOSE as u32).unwrap(),
        ChildNumber::from_hardened_idx(coin_type).unwrap(),
        ChildNumber::from_hardened_idx(ACCOUNT as u32).unwrap(),
    ];
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic.to_string())?;
    let master_xprv =
        ExtendedPrivKey::new_master(bitcoin_network.into(), &mnemonic.to_seed("")).unwrap();
    Ok(master_xprv.derive_priv(&Secp256k1::new(), &account_derivation_path)?)
}

pub(crate) fn get_xpub_from_xprv(xprv: &ExtendedPrivKey) -> ExtendedPubKey {
    ExtendedPubKey::from_priv(&Secp256k1::new(), xprv)
}

/// Get account-level xPub for the given mnemonic and Bitcoin network
pub fn get_account_xpub(
    bitcoin_network: BitcoinNetwork,
    mnemonic: &str,
) -> Result<ExtendedPubKey, Error> {
    let account_xprv = derive_account_xprv_from_mnemonic(bitcoin_network, mnemonic)?;
    Ok(get_xpub_from_xprv(&account_xprv))
}

fn get_derivation_path(keychain: u8) -> DerivationPath {
    let derivation_path = vec![ChildNumber::from_normal_idx(keychain as u32).unwrap()];
    DerivationPath::from_iter(derivation_path.clone())
}

fn get_descriptor_priv_key(
    xprv: ExtendedPrivKey,
    keychain: u8,
) -> Result<DescriptorSecretKey, Error> {
    let path = get_derivation_path(keychain);
    let der_xprv = &xprv
        .derive_priv(&Secp256k1::new(), &path)
        .expect("provided path should be derivable in an xprv");
    let origin_prv: KeySource = (xprv.fingerprint(&Secp256k1::new()), path);
    let der_xprv_desc_key: DescriptorKey<Segwitv0> = der_xprv
        .into_descriptor_key(Some(origin_prv), DerivationPath::default())
        .expect("should be able to convert xprv in a descriptor key");
    if let Secret(key, _, _) = der_xprv_desc_key {
        Ok(key)
    } else {
        Err(InternalError::Unexpected)?
    }
}

fn get_descriptor_pub_key(
    xpub: ExtendedPubKey,
    keychain: u8,
) -> Result<DescriptorPublicKey, Error> {
    let path = get_derivation_path(keychain);
    let der_xpub = &xpub
        .derive_pub(&Secp256k1::new(), &path)
        .expect("provided path should be derivable in an xpub");
    let origin_pub: KeySource = (xpub.fingerprint(), path);
    let der_xpub_desc_key: DescriptorKey<Segwitv0> = der_xpub
        .into_descriptor_key(Some(origin_pub), DerivationPath::default())
        .expect("should be able to convert xpub in a descriptor key");
    if let Public(key, _, _) = der_xpub_desc_key {
        Ok(key)
    } else {
        Err(InternalError::Unexpected)?
    }
}

pub(crate) fn calculate_descriptor_from_xprv(
    xprv: ExtendedPrivKey,
    keychain: u8,
) -> Result<String, Error> {
    let key = get_descriptor_priv_key(xprv, keychain)?;
    Ok(format!("tr({key})"))
}

pub(crate) fn calculate_descriptor_from_xpub(
    xpub: ExtendedPubKey,
    keychain: u8,
) -> Result<String, Error> {
    let key = get_descriptor_pub_key(xpub, keychain)?;
    Ok(format!("tr({key})"))
}

fn convert_time_fmt_error(cause: time::error::Format) -> io::Error {
    io::Error::new(ErrorKind::Other, cause)
}

fn log_timestamp(io: &mut dyn io::Write) -> io::Result<()> {
    let now: time::OffsetDateTime = now();
    write!(
        io,
        "{}",
        now.format(TIMESTAMP_FORMAT)
            .map_err(convert_time_fmt_error)?
    )
}

pub(crate) fn setup_logger<P: AsRef<Path>>(
    log_path: P,
    log_name: Option<&str>,
) -> Result<Logger, Error> {
    let log_file = log_name.unwrap_or(LOG_FILE);
    let log_filepath = log_path.as_ref().join(log_file);
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_filepath)?;

    let decorator = PlainDecorator::new(file);
    let drain = FullFormat::new(decorator)
        .use_custom_timestamp(log_timestamp)
        .use_file_location();
    let drain = slog_async::Async::new(drain.build().fuse()).build().fuse();

    Ok(Logger::root(drain, o!()))
}

pub(crate) fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// Wrapper for the RGB stock and its lockfile.
pub struct RgbRuntime {
    /// Path to the RGB stock
    pub stock_path: PathBuf,
    /// The RGB stock
    pub stock: Stock,
    /// The wallet directory, where the lockfile for the runtime is to be held
    pub wallet_dir: PathBuf,
}

impl RgbRuntime {
    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn accept_transfer<R: ResolveHeight>(
        &mut self,
        transfer: RgbTransfer,
        resolver: &mut R,
        force: bool,
    ) -> Result<Status, InternalError>
    where
        R::Error: 'static,
    {
        self.stock
            .accept_transfer(transfer, resolver, force)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn blank_builder(
        &mut self,
        contract_id: ContractId,
        iface: impl Into<TypeName>,
    ) -> Result<TransitionBuilder, InternalError> {
        self.stock
            .blank_builder(contract_id, iface)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn consume(&mut self, fascia: Fascia) -> Result<(), InternalError> {
        self.stock.consume(fascia).map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn contract_ids(&self) -> Result<BTreeSet<ContractId>, InternalError> {
        self.stock.contract_ids().map_err(InternalError::from)
    }

    pub(crate) fn contract_iface_id(
        &mut self,
        contract_id: ContractId,
        iface_id: IfaceId,
    ) -> Result<ContractIface, InternalError> {
        self.stock
            .contract_iface_id(contract_id, iface_id)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn contracts_by_outputs(
        &mut self,
        outputs: impl IntoIterator<Item = impl Into<XOutputSeal>>,
    ) -> Result<BTreeSet<ContractId>, InternalError> {
        self.stock
            .contracts_by_outputs(outputs)
            .map_err(InternalError::from)
    }

    pub(crate) fn export_contract(
        &self,
        contract_id: ContractId,
    ) -> Result<Contract, InternalError> {
        self.stock
            .export_contract(contract_id)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn genesis(&self, contract_id: ContractId) -> Result<&Genesis, InternalError> {
        self.stock.genesis(contract_id).map_err(InternalError::from)
    }

    pub(crate) fn iface_by_name(&self, name: &TypeName) -> Result<&Iface, InternalError> {
        self.stock.iface_by_name(name).map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn import_contract<R: ResolveHeight>(
        &mut self,
        contract: Contract,
        resolver: &mut R,
    ) -> Result<Status, InternalError>
    where
        R::Error: 'static,
    {
        self.stock
            .import_contract(contract, resolver)
            .map_err(InternalError::from)
    }

    pub(crate) fn import_iface(&mut self, iface: Iface) -> Result<Status, InternalError> {
        self.stock.import_iface(iface).map_err(InternalError::from)
    }

    pub(crate) fn import_iface_impl(&mut self, iimpl: IfaceImpl) -> Result<Status, InternalError> {
        self.stock
            .import_iface_impl(iimpl)
            .map_err(InternalError::from)
    }

    pub(crate) fn import_schema(&mut self, schema: SubSchema) -> Result<Status, InternalError> {
        self.stock
            .import_schema(schema)
            .map_err(InternalError::from)
    }

    pub(crate) fn schema_ids(&self) -> Result<BTreeSet<SchemaId>, InternalError> {
        self.stock.schema_ids().map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn state_for_outpoints(
        &mut self,
        contract_id: ContractId,
        outpoints: impl IntoIterator<Item = impl Into<XOutpoint>>,
    ) -> Result<BTreeMap<(Opout, XOutputSeal), PersistedState>, InternalError> {
        self.stock
            .state_for_outpoints(contract_id, outpoints)
            .map_err(InternalError::from)
    }

    pub(crate) fn store_seal_secret(
        &mut self,
        seal: XChain<GraphSeal>,
    ) -> Result<(), InternalError> {
        self.stock
            .store_seal_secret(seal)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn transfer(
        &mut self,
        contract_id: ContractId,
        outputs: impl AsRef<[XOutputSeal]>,
        secret_seals: impl AsRef<[XChain<SecretSeal>]>,
    ) -> Result<RgbTransfer, InternalError> {
        self.stock
            .transfer(contract_id, outputs, secret_seals)
            .map_err(|_| InternalError::Unexpected)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn transition_builder(
        &mut self,
        contract_id: ContractId,
        iface: impl Into<TypeName>,
        transition_name: Option<impl Into<FieldName>>,
    ) -> Result<TransitionBuilder, InternalError> {
        self.stock
            .transition_builder(contract_id, iface, transition_name)
            .map_err(InternalError::from)
    }
}

impl Drop for RgbRuntime {
    fn drop(&mut self) {
        self.stock
            .store(&self.stock_path)
            .expect("unable to save stock");
        fs::remove_file(self.wallet_dir.join(RGB_RUNTIME_LOCK_FILE))
            .expect("should be able to drop lockfile")
    }
}

fn _write_rgb_runtime_lockfile(wallet_dir: &Path) {
    let lock_file_path = wallet_dir.join(RGB_RUNTIME_LOCK_FILE);
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_file_path.clone())
        {
            Ok(_) => break,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(400)),
        }
    }
}

/// Write the lock file for write access in the provided `wallet_dir`, load the RGB runtime for the
/// provided `bitcoin_network` and return the loaded runtime.
///
/// <div class="warning">This method is meant for special usage and is normally not needed, use
/// it only if you know what you're doing</div>
pub fn load_rgb_runtime(wallet_dir: PathBuf) -> Result<RgbRuntime, Error> {
    _write_rgb_runtime_lockfile(&wallet_dir);

    let rgb_dir = wallet_dir.join(RGB_RUNTIME_DIR);
    if !rgb_dir.exists() {
        fs::create_dir_all(&rgb_dir)?;
    }
    let stock_path = rgb_dir.join("stock.dat");
    let stock = Stock::load(&stock_path).or_else(|err| {
        if matches!(err, DeserializeError::Decode(DecodeError::Io(ref err)) if err.kind() == ErrorKind::NotFound) {
            let stock = Stock::default();
            stock.store(&stock_path).expect("unable to save stock");
            return Ok(stock)
        }
        Err(Error::IO { details: err.to_string() })
    })?;

    Ok(RgbRuntime {
        stock_path,
        stock,
        wallet_dir,
    })
}

fn convert_prop_key(prop_key: PropKey) -> ProprietaryKey {
    ProprietaryKey {
        prefix: prop_key.identifier.into(),
        subtype: prop_key.subtype as u8,
        key: prop_key.data.to_vec(),
    }
}

trait RgbPropKey {
    fn opret_host() -> ProprietaryKey {
        convert_prop_key(PropKey::opret_host())
    }

    fn rgb_transition(opid: OpId) -> ProprietaryKey {
        convert_prop_key(PropKey::rgb_transition(opid))
    }

    fn rgb_closing_methods(opid: OpId) -> ProprietaryKey {
        convert_prop_key(PropKey::rgb_closing_methods(opid))
    }

    fn rgb_in_consumed_by(contract_id: ContractId) -> ProprietaryKey {
        convert_prop_key(PropKey::rgb_in_consumed_by(contract_id))
    }

    fn mpc_entropy() -> ProprietaryKey {
        convert_prop_key(PropKey::mpc_entropy())
    }
}

impl RgbPropKey for ProprietaryKey {}

/// Methods adding RGB functionality to rust-bitcoin Input
pub trait RgbInExt {
    /// See upstream method for details
    fn rgb_consumer(&self, contract_id: ContractId) -> Result<Option<OpId>, FromSliceError>;
    /// See upstream method for details
    fn set_rgb_consumer(&mut self, contract_id: ContractId, opid: OpId) -> Result<bool, Error>;
}

impl RgbInExt for Input {
    fn rgb_consumer(&self, contract_id: ContractId) -> Result<Option<OpId>, FromSliceError> {
        let Some(data) = self
            .proprietary
            .get(&ProprietaryKey::rgb_in_consumed_by(contract_id))
        else {
            return Ok(None);
        };
        Ok(Some(OpId::copy_from_slice(data)?))
    }

    fn set_rgb_consumer(&mut self, contract_id: ContractId, opid: OpId) -> Result<bool, Error> {
        let key = ProprietaryKey::rgb_in_consumed_by(contract_id);
        match self.rgb_consumer(contract_id) {
            Ok(None) | Err(_) => {
                let _ = self.proprietary.insert(key, opid.to_vec());
                Ok(true)
            }
            Ok(Some(id)) if id == opid => Ok(false),
            Ok(Some(_)) => Err(Error::Internal {
                details: s!("proprietary key is already present"),
            }),
        }
    }
}

/// Methods adding RGB functionality to rust-bitcoin Output
pub trait RgbOutExt {
    /// See upstream method for details
    fn set_opret_host(&mut self);

    /// See upstream method for details
    fn set_mpc_entropy(&mut self, entropy: u64);
}

impl RgbOutExt for Output {
    fn set_opret_host(&mut self) {
        self.proprietary
            .insert(ProprietaryKey::opret_host(), vec![]);
    }

    fn set_mpc_entropy(&mut self, entropy: u64) {
        let val = entropy.to_le_bytes().to_vec();
        self.proprietary.insert(ProprietaryKey::mpc_entropy(), val);
    }
}

/// Methods adding RGB functionality to rust-bitcoin Psbt
pub trait RgbPsbtExt {
    /// See upstream method for details
    fn rgb_transition(&self, opid: OpId) -> Result<Option<Transition>, InternalError>;

    /// See upstream method for details
    fn rgb_close_methods(&self, opid: OpId) -> Result<Option<CloseMethodSet>, Error>;

    /// See upstream method for details
    fn push_rgb_transition(
        &mut self,
        transition: Transition,
        methods: CloseMethodSet,
    ) -> Result<bool, Error>;
}

impl RgbPsbtExt for PartiallySignedTransaction {
    fn rgb_transition(&self, opid: OpId) -> Result<Option<Transition>, InternalError> {
        let Some(data) = self.proprietary.get(&ProprietaryKey::rgb_transition(opid)) else {
            return Ok(None);
        };
        let data = Confined::try_from_iter(data.iter().copied())?;
        let transition = Transition::from_strict_serialized::<U24>(data).unwrap();
        Ok(Some(transition))
    }

    fn rgb_close_methods(&self, opid: OpId) -> Result<Option<CloseMethodSet>, Error> {
        let Some(m) = self
            .proprietary
            .get(&ProprietaryKey::rgb_closing_methods(opid))
        else {
            return Ok(None);
        };
        if m.len() == 1 {
            if let Ok(method) = CloseMethodSet::try_from(m[0]) {
                return Ok(Some(method));
            }
        }
        Err(Error::Internal {
            details: s!("invalid close method"),
        })
    }

    fn push_rgb_transition(
        &mut self,
        mut transition: Transition,
        mut methods: CloseMethodSet,
    ) -> Result<bool, Error> {
        let opid = transition.id();
        let prev_methods = self.rgb_close_methods(opid)?;
        let prev_transition = self.rgb_transition(opid)?;
        if let Some(ref prev_transition) = prev_transition {
            transition = transition
                .merge_reveal(prev_transition.clone())
                .map_err(|e| Error::Internal {
                    details: e.to_string(),
                })?;
        }
        let serialized_transition =
            transition
                .to_strict_serialized::<U24>()
                .map_err(|e| Error::Internal {
                    details: e.to_string(),
                })?;
        let _ = self.proprietary.insert(
            ProprietaryKey::rgb_transition(opid),
            serialized_transition.into_inner(),
        );
        methods |= prev_methods;
        let _ = self.proprietary.insert(
            ProprietaryKey::rgb_closing_methods(opid),
            vec![methods as u8],
        );
        Ok(prev_transition.is_none())
    }
}
