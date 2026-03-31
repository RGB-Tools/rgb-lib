//! Utilities.
//!
//! This module defines some utility methods and structures.

use super::*;

const TIMESTAMP_FORMAT: &[time::format_description::BorrowedFormatItem] = time::macros::format_description!(
    "[year]-[month]-[day]T[hour repr:24]:[minute]:[second].[subsecond digits:3]+00"
);

#[cfg(feature = "rgb_fs_store_migration")]
pub(crate) const RGB_RUNTIME_DIR: &str = "rgb";

/// Where the legacy filesystem RGB store is moved to be deleted, once migrated into the DB, so
/// that retiring it is atomic. Only ever on disk between that rename and the delete: a crash in
/// between leaves this dir, which the next wallet open sweeps up and every backup skips.
pub(crate) const RGB_RUNTIME_DELETING_DIR: &str = "rgb.deleting";
pub(crate) const LOG_FILE: &str = "log";

/// Size cap for a strict-encoded fascia file, matching what rgb-ops uses for its own containers.
const FASCIA_MAX_LEN: usize = amplify::confinement::U32;

pub(crate) const PURPOSE: u8 = 86;
pub(crate) const COIN_RGB_MAINNET: u32 = 827166;
pub(crate) const COIN_RGB_TESTNET: u32 = 827167;
pub(crate) const ACCOUNT: u8 = 0;
pub(crate) const KEYCHAIN_RGB: u8 = 0;
pub(crate) const KEYCHAIN_BTC: u8 = 0;

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) const INDEXER_STOP_GAP: usize = 20;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) const INDEXER_TIMEOUT: u64 = 10;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) const INDEXER_RETRIES: u8 = 3;
#[cfg(feature = "electrum")]
pub(crate) const INDEXER_BATCH_SIZE: usize = 5;
#[cfg(feature = "esplora")]
pub(crate) const INDEXER_PARALLEL_REQUESTS: usize = 5;

#[cfg(feature = "_online")]
const PROXY_PROTOCOL_VERSION: &str = "1.0";

// sea-orm with runtime-tokio-rustls needs a tokio runtime for connection pool management
static TOKIO_RUNTIME: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| tokio::runtime::Runtime::new().expect("failed to create the runtime"));

/// Block on a future, spawning a new thread if already inside a Tokio runtime.
pub fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future + Send,
    F::Output: Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        // avoid blocking the Tokio runtime thread; spawn a new thread
        std::thread::scope(|s| {
            s.spawn(|| TOKIO_RUNTIME.block_on(future))
                .join()
                .expect("rgb-lib block_on thread panicked")
        })
    } else {
        TOKIO_RUNTIME.block_on(future)
    }
}

/// Supported Bitcoin networks.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum BitcoinNetwork {
    /// Bitcoin's mainnet
    Mainnet,
    /// Bitcoin's testnet3
    Testnet,
    /// Bitcoin's testnet4
    Testnet4,
    /// Bitcoin's default signet
    Signet,
    /// Bitcoin's regtest
    Regtest,
    /// Bitcoin's custom signet
    SignetCustom,
}

impl BitcoinNetwork {
    pub(crate) fn network_kind(&self) -> NetworkKind {
        match self {
            BitcoinNetwork::Mainnet => NetworkKind::Main,
            _ => NetworkKind::Test,
        }
    }
}

impl fmt::Display for BitcoinNetwork {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl FromStr for BitcoinNetwork {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "mainnet" | "bitcoin" => BitcoinNetwork::Mainnet,
            "testnet" | "testnet3" => BitcoinNetwork::Testnet,
            "testnet4" => BitcoinNetwork::Testnet4,
            "regtest" => BitcoinNetwork::Regtest,
            "signet" => BitcoinNetwork::Signet,
            "signetcustom" => BitcoinNetwork::SignetCustom,
            _ => {
                return Err(Error::InvalidBitcoinNetwork {
                    network: s.to_string(),
                });
            }
        })
    }
}

impl TryFrom<ChainNet> for BitcoinNetwork {
    type Error = Error;

    fn try_from(x: ChainNet) -> Result<Self, Self::Error> {
        match x {
            ChainNet::BitcoinMainnet => Ok(BitcoinNetwork::Mainnet),
            ChainNet::BitcoinTestnet3 => Ok(BitcoinNetwork::Testnet),
            ChainNet::BitcoinTestnet4 => Ok(BitcoinNetwork::Testnet4),
            ChainNet::BitcoinSignet => Ok(BitcoinNetwork::Signet),
            ChainNet::BitcoinRegtest => Ok(BitcoinNetwork::Regtest),
            ChainNet::BitcoinSignetCustom => Ok(BitcoinNetwork::SignetCustom),
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
            BitcoinNetwork::Testnet4 => bitcoin::Network::Testnet4,
            BitcoinNetwork::Signet => bitcoin::Network::Signet,
            BitcoinNetwork::Regtest => bitcoin::Network::Regtest,
            BitcoinNetwork::SignetCustom => bitcoin::Network::Signet,
        }
    }
}

impl From<BitcoinNetwork> for NetworkKind {
    fn from(x: BitcoinNetwork) -> Self {
        match x {
            BitcoinNetwork::Mainnet => Self::Main,
            _ => Self::Test,
        }
    }
}

impl From<BitcoinNetwork> for ChainNet {
    fn from(x: BitcoinNetwork) -> ChainNet {
        match x {
            BitcoinNetwork::Mainnet => ChainNet::BitcoinMainnet,
            BitcoinNetwork::Testnet => ChainNet::BitcoinTestnet3,
            BitcoinNetwork::Testnet4 => ChainNet::BitcoinTestnet4,
            BitcoinNetwork::Signet => ChainNet::BitcoinSignet,
            BitcoinNetwork::Regtest => ChainNet::BitcoinRegtest,
            BitcoinNetwork::SignetCustom => ChainNet::BitcoinSignetCustom,
        }
    }
}

/// Make a newly created file's directory entry durable.
///
/// [`File::sync_all`](fs::File::sync_all) covers a file's own data and metadata but not the entry
/// that names it, so without this a freshly created file can still vanish on power loss.
#[cfg(not(target_os = "windows"))]
pub(crate) fn sync_dir<P: AsRef<Path>>(dir: P) -> Result<(), io::Error> {
    fs::File::open(dir)?.sync_all()
}

/// Windows cannot open a directory as a file and offers no portable equivalent, so the directory
/// entry is left to the filesystem's own ordering there.
#[cfg(target_os = "windows")]
pub(crate) fn sync_dir<P: AsRef<Path>>(_dir: P) -> Result<(), io::Error> {
    Ok(())
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

fn deserialize_str_or_number<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr + Copy,
    T::Err: fmt::Display,
{
    struct StringOrNumberVisitor<T>(std::marker::PhantomData<T>);

    impl<T> Visitor<'_> for StringOrNumberVisitor<T>
    where
        T: FromStr + Copy,
        T::Err: fmt::Display,
    {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string, a number, or null")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            T::from_str(&value.to_string())
                .map(Some)
                .map_err(de::Error::custom)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            T::from_str(&value.to_string())
                .map(Some)
                .map_err(de::Error::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse::<T>().map(Some).map_err(|e| {
                de::Error::invalid_value(Unexpected::Str(value), &e.to_string().as_str())
            })
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(StringOrNumberVisitor(std::marker::PhantomData))
}

pub(crate) fn from_str_or_number_mandatory<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr + Copy,
    T::Err: fmt::Display,
{
    match deserialize_str_or_number(deserializer)? {
        Some(val) => Ok(val),
        None => Err(de::Error::custom("expected a number but got null")),
    }
}

pub(crate) fn from_str_or_number_optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr + Copy,
    T::Err: fmt::Display,
{
    deserialize_str_or_number(deserializer)
}

pub(crate) fn str_to_xpub(xpub: &str, network_kind: &NetworkKind) -> Result<Xpub, Error> {
    let pubkey_btc = Xpub::from_str(xpub)?;
    let extended_key_btc: ExtendedKey = ExtendedKey::from(pubkey_btc);
    Ok(extended_key_btc.into_xpub(*network_kind, &Secp256k1::new()))
}

pub(crate) fn get_coin_type(bitcoin_network: &BitcoinNetwork, rgb: bool) -> u32 {
    match (bitcoin_network, rgb) {
        (BitcoinNetwork::Mainnet, true) => COIN_RGB_MAINNET,
        (_, true) => COIN_RGB_TESTNET,
        (_, false) => u32::from(*bitcoin_network != BitcoinNetwork::Mainnet),
    }
}

pub(crate) fn get_account_derivation_children(
    witness_version: WitnessVersion,
    coin_type: u32,
) -> Vec<ChildNumber> {
    vec![
        ChildNumber::from_hardened_idx(witness_version.purpose()).unwrap(),
        ChildNumber::from_hardened_idx(coin_type).unwrap(),
        ChildNumber::from_hardened_idx(ACCOUNT as u32).unwrap(),
    ]
}

fn derive_account_xprv_from_mnemonic(
    bitcoin_network: &BitcoinNetwork,
    mnemonic: &str,
    rgb: bool,
    witness_version: WitnessVersion,
) -> Result<(Xpriv, Fingerprint), Error> {
    let coin_type = get_coin_type(bitcoin_network, rgb);
    let account_derivation_children = get_account_derivation_children(witness_version, coin_type);
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic.to_string())?;
    let master_xprv = Xpriv::new_master(*bitcoin_network, &mnemonic.to_seed("")).unwrap();
    let master_xpub = Xpub::from_priv(&Secp256k1::new(), &master_xprv);
    let master_fingerprint = master_xpub.fingerprint();
    let account_xprv = master_xprv.derive_priv(&Secp256k1::new(), &account_derivation_children)?;
    Ok((account_xprv, master_fingerprint))
}

fn get_xpub_from_xprv(xprv: &Xpriv) -> Xpub {
    Xpub::from_priv(&Secp256k1::new(), xprv)
}

/// Get the account-level xPriv and xPub for the given mnemonic, Bitcoin network and witness
/// version, based on the requested wallet side (colored or vanilla).
pub fn get_account_data(
    bitcoin_network: &BitcoinNetwork,
    mnemonic: &str,
    rgb: bool,
    witness_version: WitnessVersion,
) -> Result<(Xpriv, Xpub, Fingerprint), Error> {
    let (account_xprv, master_fingerprint) =
        derive_account_xprv_from_mnemonic(bitcoin_network, mnemonic, rgb, witness_version)?;
    let account_xpub = get_xpub_from_xprv(&account_xprv);
    Ok((account_xprv, account_xpub, master_fingerprint))
}

pub(crate) fn get_account_xpubs(
    bitcoin_network: &BitcoinNetwork,
    mnemonic: &str,
    witness_version: WitnessVersion,
) -> Result<(Xpub, Xpub), Error> {
    let (_, account_xpub_vanilla, _) =
        get_account_data(bitcoin_network, mnemonic, false, witness_version)?;
    let (_, account_xpub_colored, _) =
        get_account_data(bitcoin_network, mnemonic, true, witness_version)?;
    Ok((account_xpub_vanilla, account_xpub_colored))
}

fn derive_descriptor(
    bitcoin_network: &BitcoinNetwork,
    mnemonic: &str,
    rgb: bool,
    keychain: u8,
    expected_xpub: &Xpub,
    witness_version: WitnessVersion,
) -> Result<String, Error> {
    let (account_xprv, account_xpub, master_fingerprint) =
        get_account_data(bitcoin_network, mnemonic, rgb, witness_version)?;
    if account_xpub != *expected_xpub {
        return Err(Error::InvalidBitcoinKeys);
    }
    let coin_type = get_coin_type(bitcoin_network, rgb);
    calculate_descriptor_from_xprv(
        &master_fingerprint,
        coin_type,
        account_xprv,
        keychain,
        witness_version,
    )
}

pub(crate) fn get_descriptors(
    bitcoin_network: &BitcoinNetwork,
    mnemonic: &str,
    vanilla_keychain: Option<u8>,
    expected_xpub_btc: &Xpub,
    expected_xpub_rgb: &Xpub,
    witness_version: WitnessVersion,
) -> Result<WalletDescriptors, Error> {
    let colored = derive_descriptor(
        bitcoin_network,
        mnemonic,
        true,
        KEYCHAIN_RGB,
        expected_xpub_rgb,
        witness_version,
    )?;
    let vanilla = derive_descriptor(
        bitcoin_network,
        mnemonic,
        false,
        vanilla_keychain.unwrap_or(KEYCHAIN_BTC),
        expected_xpub_btc,
        witness_version,
    )?;
    Ok(WalletDescriptors { colored, vanilla })
}

pub(crate) fn get_descriptors_from_xpubs(
    bitcoin_network: &BitcoinNetwork,
    master_fingerprint: &str,
    xpub_rgb: &Xpub,
    xpub_btc: &Xpub,
    vanilla_keychain: Option<u8>,
    witness_version: WitnessVersion,
) -> Result<WalletDescriptors, Error> {
    let master_fingerprint =
        Fingerprint::from_str(master_fingerprint).map_err(|_| Error::InvalidFingerprint)?;
    let colored = calculate_descriptor_from_xpub(
        &master_fingerprint,
        get_coin_type(bitcoin_network, true),
        xpub_rgb,
        KEYCHAIN_RGB,
        witness_version,
    )?;
    let vanilla = calculate_descriptor_from_xpub(
        &master_fingerprint,
        get_coin_type(bitcoin_network, false),
        xpub_btc,
        vanilla_keychain.unwrap_or(KEYCHAIN_BTC),
        witness_version,
    )?;
    Ok(WalletDescriptors { colored, vanilla })
}

pub(crate) fn parse_address_str(
    address: &str,
    bitcoin_network: BitcoinNetwork,
) -> Result<BdkAddress, Error> {
    BdkAddress::from_str(address)
        .map_err(|e| Error::InvalidAddress {
            details: e.to_string(),
        })?
        .require_network(bitcoin_network.into())
        .map_err(|_| Error::InvalidAddress {
            details: s!("belongs to another network"),
        })
}

/// Extract the witness script if recipient is a Witness one
pub fn script_buf_from_recipient_id(recipient_id: String) -> Result<Option<ScriptBuf>, Error> {
    let xchainnet_beneficiary =
        XChainNet::<Beneficiary>::from_str(&recipient_id).map_err(|_| Error::InvalidRecipientID)?;
    match xchainnet_beneficiary.into_inner() {
        Beneficiary::WitnessVout(pay_2_vout, _) => {
            let script_buf = pay_2_vout.to_script();
            Ok(Some(script_buf))
        }
        Beneficiary::BlindedSeal(_) => Ok(None),
    }
}

/// The URL of a BFA reject list, if the contract declares one served at a URL.
///
/// This is only the URL recorded in the DB and reported in the asset metadata: a reject list kept
/// by an EVM contract has no URL, so it gives `None` here, but it is still enforced, as the reject
/// list of a BFA asset is read off its contract when needed (see `asset_reject_list_source`).
pub(crate) fn bfa_reject_list_url(location: Option<RejectListLocation>) -> Option<String> {
    match location? {
        RejectListLocation::Url(url) => Some(url.to_string()),
        RejectListLocation::Evm(_) => None,
    }
}

pub(crate) fn beneficiary_from_script_buf(script_buf: ScriptBuf) -> Beneficiary {
    let address_payload = AddressPayload::from_script(&script_buf).unwrap();
    Beneficiary::WitnessVout(Pay2Vout::new(address_payload), None)
}

/// Return the recipient ID for a specific script buf
pub fn recipient_id_from_script_buf(
    script_buf: ScriptBuf,
    bitcoin_network: BitcoinNetwork,
) -> String {
    let beneficiary = beneficiary_from_script_buf(script_buf);
    XChainNet::with(bitcoin_network.into(), beneficiary).to_string()
}

fn get_derivation_path(keychain: u8) -> DerivationPath {
    let derivation_path = vec![ChildNumber::from_normal_idx(keychain as u32).unwrap()];
    DerivationPath::from_iter(derivation_path.clone())
}

pub(crate) fn get_extended_derivation_path(
    mut account_derivation_children: Vec<ChildNumber>,
    keychain: u8,
) -> DerivationPath {
    let keychain_child = ChildNumber::from_normal_idx(keychain as u32).unwrap();
    account_derivation_children.push(keychain_child);
    DerivationPath::from_iter(account_derivation_children.clone())
}

pub(crate) fn calculate_descriptor_from_xprv(
    master_fingerprint: &Fingerprint,
    coin_type: u32,
    xprv: Xpriv,
    keychain: u8,
    witness_version: WitnessVersion,
) -> Result<String, Error> {
    // derive final xpub from account-level xpub
    let path = get_derivation_path(keychain);
    let der_xprv = &xprv
        .derive_priv(&Secp256k1::new(), &path)
        .expect("provided path should be derivable in an xprv");
    // derive descriptor with master fingerprint and full derivation path
    let account_derivation_children = get_account_derivation_children(witness_version, coin_type);
    let full_path = get_extended_derivation_path(account_derivation_children, keychain);
    let origin_prv: KeySource = (*master_fingerprint, full_path.clone());
    let der_xprv_desc_key: DescriptorKey<Segwitv0> = der_xprv
        .into_descriptor_key(Some(origin_prv), DerivationPath::default())
        .expect("should be able to convert xprv in a descriptor key");
    let Secret(key, _, _) = der_xprv_desc_key else {
        unreachable!("into_descriptor_key on an Xpriv always yields a Secret variant")
    };
    Ok(format!("{}({key})", witness_version.descriptor_fn()))
}

pub(crate) fn calculate_descriptor_from_xpub(
    master_fingerprint: &Fingerprint,
    coin_type: u32,
    xpub: &Xpub,
    keychain: u8,
    witness_version: WitnessVersion,
) -> Result<String, Error> {
    // derive final xpub from account-level xpub
    let path = get_derivation_path(keychain);
    let der_xpub = xpub
        .derive_pub(&Secp256k1::new(), &path)
        .expect("provided path should be derivable in an xpub");
    // derive descriptor with master fingerprint and full derivation path
    let account_derivation_children = get_account_derivation_children(witness_version, coin_type);
    let full_path = get_extended_derivation_path(account_derivation_children, keychain);
    let origin_pub: KeySource = (*master_fingerprint, full_path);
    let der_xpub_desc_key: DescriptorKey<Segwitv0> = der_xpub
        .into_descriptor_key(Some(origin_pub), DerivationPath::default())
        .expect("should be able to convert xpub in a descriptor key");
    let Public(key, _, _) = der_xpub_desc_key else {
        unreachable!("into_descriptor_key on an Xpub always yields a Public variant")
    };
    Ok(format!("{}({key})", witness_version.descriptor_fn()))
}

/// The retention grace advertised by a proxy (`retention_grace_secs`), i.e. how long after an
/// expiration it keeps the data awaiting retrieval. A proxy not advertising it is considered to
/// grant none.
#[cfg(feature = "_online")]
pub(crate) fn proxy_retention_grace(info: &ServerInfoResponse) -> u32 {
    #[cfg(test)]
    if let Some(grace) = mock_retention_grace() {
        return grace;
    }
    info.retention_grace_secs
        .as_ref()
        .and_then(|grace| grace.parse::<u32>().ok())
        .unwrap_or(0)
}

/// The max number of media per transfer advertised by a proxy (`max_media`), `None` if it doesn't
/// advertise a valid one.
#[cfg(feature = "_online")]
pub(crate) fn proxy_max_media(info: &ServerInfoResponse) -> Option<usize> {
    info.max_media.as_ref().and_then(|max| max.parse().ok())
}

/// Check that the proxy at `proxy_url` runs the supported protocol version, returning its info.
#[cfg(feature = "_online")]
pub(crate) fn check_proxy(proxy_url: &str) -> Result<ServerInfoResponse, Error> {
    let proxy_client = ProxyClient::new(proxy_url)?;
    let server_info = proxy_client.get_info()?;
    if let Some(info) = server_info.result {
        if info.protocol_version == *PROXY_PROTOCOL_VERSION {
            return Ok(info);
        } else {
            return Err(Error::InvalidProxyProtocol {
                version: info.protocol_version,
            });
        }
    }
    let details = match server_info.error {
        Some(err) => err.message,
        None => s!("proxy response has neither result nor error"),
    };
    Err(Error::Proxy { details })
}

#[cfg(feature = "_online")]
pub(crate) fn get_indexer_and_resolver(
    indexer_config: &IndexerConfig,
    bitcoin_network: BitcoinNetwork,
) -> Result<(Indexer, AnyResolver), Error> {
    let invalid = |details: String| Error::InvalidIndexer { details };

    let mut indexer = build_indexer(indexer_config)?;
    // this also checks connectivity (and credentials for bitcoind)
    indexer
        .block_hash(0)
        .map_err(|e| invalid(unreachable_indexer_details(indexer_config, &e)))?;

    let resolver = match &mut indexer {
        #[cfg(feature = "bitcoind")]
        Indexer::Bitcoind(client) => {
            // RGB witnesses are looked up by TX ID alone, which needs a synced -txindex
            let indexes = client.indexes()?;
            if !indexes.txindex {
                return Err(invalid(s!(
                    "bitcoind has no synced transaction index (-txindex)"
                )));
            }
            client.block_filters = indexes.block_filters;
            let IndexerConfig::Bitcoind { url, auth } = indexer_config else {
                unreachable!("build_indexer only returns a bitcoind indexer for a bitcoind config")
            };
            AnyResolver::bitcoind_blocking(url, auth.into()).map_err(invalid)?
        }
        #[cfg(feature = "electrum")]
        Indexer::Electrum(_) => {
            // the resolver opens its own connection, which can fail even if the indexer one
            // succeeded
            AnyResolver::electrum_blocking(indexer_config.url(), Some(electrum_config()))
                .map_err(|e| invalid(e.to_string()))?
        }
        #[cfg(feature = "esplora")]
        Indexer::Esplora(_) => AnyResolver::esplora_blocking(esplora_config(indexer_config.url()))
            .expect("esplora_blocking wraps an infallible builder and always returns Ok"),
    };

    resolver
        .check_chain_net(bitcoin_network.into())
        .map_err(|e| invalid(e.to_string()))?;

    Ok((indexer, resolver))
}

/// Details for the [`Error::InvalidIndexer`] raised when the service at `indexer_config` cannot be
/// reached or doesn't speak the configured protocol.
#[cfg(feature = "_online")]
fn unreachable_indexer_details(indexer_config: &IndexerConfig, err: &impl fmt::Display) -> String {
    match indexer_config {
        IndexerConfig::Bitcoind { .. } => format!("cannot connect to bitcoind: {err}"),
        IndexerConfig::Electrum { .. } => s!("not a valid electrum server"),
        IndexerConfig::Esplora { .. } => s!("not a valid esplora server"),
    }
}

#[cfg(feature = "bitcoind")]
impl From<&BitcoindAuth> for BitcoindRpcAuth {
    fn from(auth: &BitcoindAuth) -> Self {
        match auth {
            BitcoindAuth::UserPass { username, password } => {
                BitcoindRpcAuth::UserPass(username.clone(), password.clone())
            }
            BitcoindAuth::CookieFile { path } => BitcoindRpcAuth::CookieFile(PathBuf::from(path)),
        }
    }
}

#[cfg(feature = "electrum")]
pub(crate) fn electrum_config() -> ElectrumConfig {
    ConfigBuilder::new()
        .retry(INDEXER_RETRIES)
        .timeout(Some(Duration::from_secs(INDEXER_TIMEOUT)))
        .build()
}

#[cfg(feature = "esplora")]
pub(crate) fn esplora_config(indexer_url: &str) -> EsploraBuilder {
    EsploraBuilder::new(indexer_url)
        .max_retries(INDEXER_RETRIES.into())
        .timeout(INDEXER_TIMEOUT)
}

/// Build the indexer client described by `indexer_config`.
///
/// Fails if support for the configured indexer is not compiled in. The returned client has not
/// contacted the service yet, except for electrum, whose client connects on creation.
#[cfg(feature = "_online")]
pub(crate) fn build_indexer(indexer_config: &IndexerConfig) -> Result<Indexer, Error> {
    match indexer_config {
        #[cfg(feature = "bitcoind")]
        IndexerConfig::Bitcoind { url, auth } => {
            let client =
                BitcoindRpcClient::new(url, auth.into()).map_err(|e| Error::InvalidIndexer {
                    details: unreachable_indexer_details(indexer_config, &e),
                })?;
            Ok(Indexer::Bitcoind(Box::new(BitcoindClient {
                inner: client,
                block_filters: false,
            })))
        }
        #[cfg(feature = "electrum")]
        IndexerConfig::Electrum { url } => {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
            let client = ElectrumClient::from_config(url, electrum_config()).map_err(|e| {
                Error::InvalidIndexer {
                    details: unreachable_indexer_details(indexer_config, &e),
                }
            })?;
            Ok(Indexer::Electrum(Box::new(BdkElectrumClient::new(client))))
        }
        #[cfg(feature = "esplora")]
        IndexerConfig::Esplora { url } => Ok(Indexer::Esplora(Box::new(
            EsploraClient::from_builder(esplora_config(url)),
        ))),
        #[cfg(not(all(feature = "bitcoind", feature = "electrum", feature = "esplora")))]
        _ => {
            let feature = match indexer_config {
                IndexerConfig::Bitcoind { .. } => "bitcoind",
                IndexerConfig::Electrum { .. } => "electrum",
                IndexerConfig::Esplora { .. } => "esplora",
            };
            Err(Error::InvalidIndexer {
                details: format!(
                    "{feature} support is not enabled (requires the `{feature}` feature)"
                ),
            })
        }
    }
}

/// Detect which indexer is served at `indexer_url`: a Bitcoin Core node when `bitcoind_auth` is
/// provided, otherwise an electrum or esplora server (detected by trying electrum first).
///
/// Returns `None` if no supported indexer can be detected.
#[cfg(feature = "_online")]
pub(crate) fn detect_indexer(
    indexer_url: &str,
    bitcoind_auth: Option<&BitcoindAuth>,
) -> Option<IndexerConfig> {
    if let Some(auth) = bitcoind_auth {
        return cfg!(feature = "bitcoind").then(|| IndexerConfig::Bitcoind {
            url: indexer_url.to_string(),
            auth: auth.clone(),
        });
    }
    let electrum = IndexerConfig::Electrum {
        url: indexer_url.to_string(),
    };
    if build_indexer(&electrum).is_ok() {
        return Some(electrum);
    }
    cfg!(feature = "esplora").then(|| IndexerConfig::Esplora {
        url: indexer_url.to_string(),
    })
}

pub(crate) fn hash_bytes(data: &[u8]) -> Vec<u8> {
    sha256_digest(data).to_vec()
}

pub(crate) fn hash_bytes_hex(data: &[u8]) -> String {
    hex::encode(hash_bytes(data))
}

/// Atomically materialize a file at `path`.
///
/// The `write` closure writes to a temporary file in the same directory, which
/// is then fsynced and renamed into place. Since `rename` is atomic on a
/// single filesystem, a reader (or a crash) never observes a partially-written
/// file. The fsync (plus a best-effort directory fsync) makes the result
/// durable across a power loss.
pub(crate) fn atomic_write_with<F>(path: &Path, write: F) -> Result<(), Error>
where
    F: FnOnce(&Path) -> Result<(), Error>,
{
    let parent = path.parent().ok_or_else(|| Error::Internal {
        details: format!("cannot atomically write {path:?}: no parent directory"),
    })?;
    fs::create_dir_all(parent)?;
    let tmp = atomic_tmp_path(path)?;
    // drop any temp file leaked by an earlier crash
    let _ = fs::remove_file(&tmp);
    if let Err(e) = write(&tmp) {
        // don't leave a partial temp file behind (best-effort)
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    // fsync the temp file contents before making them visible
    fs::File::open(&tmp)?.sync_all()?;
    fs::rename(&tmp, path)?;
    // fsync the parent directory so the rename entry itself is durable (a no-op on Windows, where
    // a directory can't be opened as a file: the data is already fsynced and only the durability
    // of the rename across a power loss is left to the filesystem)
    sync_dir(parent)?;
    Ok(())
}

/// The temporary file `atomic_write_with` stages `path` in before renaming it into place.
///
/// Exposed so a loader can recover a temp file left behind by a crash between the write and the
/// rename.
pub(crate) fn atomic_tmp_path(path: &Path) -> Result<PathBuf, Error> {
    let parent = path.parent().ok_or_else(|| Error::Internal {
        details: format!("cannot atomically write {path:?}: no parent directory"),
    })?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Internal {
            details: format!("cannot atomically write {path:?}: invalid file name"),
        })?;
    Ok(parent.join(format!(".{file_name}.tmp")))
}

/// Atomically write `bytes` to `path`.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    atomic_write_with(path, |tmp| Ok(fs::write(tmp, bytes)?))
}

/// Read a fascia file, which is strict-encoded like every other RGB container.
pub(crate) fn read_fascia(path: &Path) -> Result<Fascia, Error> {
    Fascia::strict_deserialize_from_file::<FASCIA_MAX_LEN>(path).map_err(|e| Error::Internal {
        details: format!("cannot read fascia {}: {e}", path.display()),
    })
}

/// Write a fascia file atomically, so a crash mid-write cannot leave a torn one behind.
#[cfg(any(feature = "_online", feature = "rgb_fs_store_migration"))]
pub(crate) fn write_fascia(path: &Path, fascia: &Fascia) -> Result<(), Error> {
    let bytes = fascia
        .to_strict_serialized::<FASCIA_MAX_LEN>()
        .map_err(|e| Error::Internal {
            details: format!("cannot serialize fascia {}: {e}", path.display()),
        })?;
    atomic_write(path, &bytes)
}

#[cfg(feature = "_online")]
pub(crate) fn hash_file(path: &Path) -> Result<String, Error> {
    let mut file = fs::File::open(path)?;
    let mut engine = sha256::HashEngine::default();
    let mut buffer = [0u8; 8192];
    while let Ok(n) = file.read(&mut buffer) {
        if n == 0 {
            break;
        }
        engine.input(&buffer[..n]);
    }
    Ok(sha256::Hash::from_engine(engine).to_string())
}

fn log_timestamp(io: &mut dyn io::Write) -> io::Result<()> {
    let now: time::OffsetDateTime = now();
    write!(
        io,
        "{}",
        now.format(TIMESTAMP_FORMAT)
            .expect("OffsetDateTime::format with a static format description is infallible")
    )
}

pub(crate) fn setup_logger<P: AsRef<Path>>(
    log_path: P,
    log_name: Option<&str>,
) -> Result<(Logger, AsyncGuard), Error> {
    let log_file = log_name.unwrap_or(LOG_FILE);
    let log_filepath = log_path.as_ref().join(log_file);
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_filepath)?;

    let decorator = PlainDecorator::new(file);
    let drain = FullFormat::new(decorator)
        .use_custom_timestamp(log_timestamp)
        .use_file_location();
    let (drain, async_guard) = slog_async::Async::new(drain.build().fuse()).build_with_guard();
    let logger = Logger::root(drain.fuse(), o!());

    Ok((logger, async_guard))
}

pub(crate) fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

pub(crate) struct DumbResolver;

impl ResolveWitness for DumbResolver {
    fn resolve_witness(&self, _: RgbTxid) -> Result<WitnessStatus, WitnessResolverError> {
        unreachable!()
    }

    fn check_chain_net(&self, _: ChainNet) -> Result<(), WitnessResolverError> {
        Ok(())
    }
}

/// Wrapper for the RGB stock.
///
/// Every read and write goes through the borrowed database transaction, so concurrent access is
/// serialized by the database itself (a single connection per process, SQLite locking across
/// processes) and no separate lock is needed.
// `pub` only because it appears in the signatures of `WalletOffline`/`WalletOnline` methods,
// supertraits of the public `RgbWalletOps*` traits; not part of the public API
#[doc(hidden)]
#[derive(Debug)]
pub struct RgbStock<'txn> {
    /// The RGB stock, writing through the shared database transaction borrowed for `'txn`
    stock: Stock<&'txn DbTxn>,
}

impl<'txn> RgbStock<'txn> {
    /// Build an [`RgbStock`] whose `Stock` reads and writes through the shared rgb-lib
    /// transaction. The store borrows `db_txn` for `'txn`, so the stock cannot outlive the
    /// transaction's commit.
    pub(crate) fn new(db_txn: &'txn DbTxn) -> Self {
        Self {
            stock: Stock::with(db_txn),
        }
    }

    #[cfg(feature = "_online")]
    pub(crate) fn accept_transfer<R: ResolveWitness>(
        &mut self,
        contract: ValidTransfer,
        resolver: &R,
    ) -> Result<(), Error> {
        self.stock
            .accept_transfer(contract, resolver)
            .map_err(Error::from)
    }

    pub(crate) fn consume_fascia(
        &mut self,
        fascia: Fascia,
        witness_ord: Option<WitnessOrd>,
    ) -> Result<(), Error> {
        struct FasciaResolver {
            witness_id: RgbTxid,
            witness_ord: WitnessOrd,
        }
        impl WitnessOrdProvider for FasciaResolver {
            fn witness_ord(&self, witness_id: RgbTxid) -> Result<WitnessOrd, WitnessResolverError> {
                debug_assert_eq!(witness_id, self.witness_id);
                Ok(self.witness_ord)
            }
        }

        let resolver = FasciaResolver {
            witness_id: fascia.witness_id(),
            witness_ord: witness_ord.unwrap_or(WitnessOrd::Tentative),
        };

        self.stock
            .consume_fascia(fascia, resolver)
            .map_err(Error::from)
    }

    pub(crate) fn contract_wrapper<C: IssuerWrapper>(
        &self,
        contract_id: ContractId,
    ) -> Result<C::Wrapper<ContractStateSnapshot>, Error> {
        self.stock
            .contract_wrapper::<C>(contract_id)
            .map_err(Error::from)
    }

    /// The assignments on the given outpoints of every contract assigning state there, in a
    /// single store query
    #[cfg(feature = "_online")]
    pub(crate) fn assignments_by_contract(
        &self,
        outpoints: impl IntoIterator<Item = impl Into<OutPoint>>,
    ) -> Result<BTreeMap<ContractId, ContractAssignments>, Error> {
        self.stock
            .assignments_by_contract(outpoints)
            .map_err(Error::from)
    }

    pub(crate) fn genesis(&self, contract_id: ContractId) -> Result<Genesis, Error> {
        self.stock
            .as_store()
            .genesis(contract_id)?
            .ok_or_else(|| Error::Internal {
                details: format!("genesis {contract_id} not found"),
            })
    }

    pub(crate) fn import_contract<R: ResolveWitness>(
        &mut self,
        contract: ValidContract,
        resolver: &R,
    ) -> Result<(), Error> {
        self.stock
            .import_contract(contract, resolver)
            .map_err(Error::from)
    }

    pub(crate) fn import_schema_definition(
        &mut self,
        schema_def: SchemaDefinition,
    ) -> Result<(), Error> {
        self.stock
            .import_schema_definition(schema_def)
            .map_err(Error::from)
    }

    pub(crate) fn consignment_data<const TRANSFER: bool>(
        &self,
        consignment: &ValidConsignment<TRANSFER>,
    ) -> Result<ContractData<FilteredContractState>, Error> {
        self.stock
            .consignment_data(consignment)
            .map_err(Error::from)
    }

    pub(crate) fn contract_assignments_for(
        &self,
        contract_id: ContractId,
        outpoints: impl IntoIterator<Item = impl Into<OutPoint>>,
    ) -> Result<ContractAssignments, Error> {
        self.stock
            .contract_assignments_for(contract_id, outpoints)
            .map_err(Error::from)
    }

    #[cfg(feature = "_online")]
    pub(crate) fn contract_schema(&self, contract_id: ContractId) -> Result<Schema, Error> {
        self.stock
            .as_store()
            .contract_schema(contract_id)?
            .ok_or_else(|| Error::Internal {
                details: format!("schema for contract {contract_id} not found"),
            })
    }

    pub(crate) fn schemata(&self) -> Result<Vec<SchemaInfo>, Error> {
        self.stock
            .schemata()
            .map(|r| r.map_err(Error::from))
            .collect()
    }

    pub(crate) fn seal_of_secret(
        &mut self,
        secret: SecretSeal,
    ) -> Result<Option<GraphSeal>, Error> {
        self.stock.as_store().seal_of_secret(secret)
    }

    pub(crate) fn store_secret_seal(&mut self, seal: GraphSeal) -> Result<(), Error> {
        self.stock.store_secret_seal(seal).map_err(Error::from)
    }

    pub(crate) fn store_spv_proofs(&mut self, proofs: RetrievedSpvProofs) -> Result<(), Error> {
        if !proofs.is_empty() {
            self.stock.store_spv_proofs(proofs)?;
        }
        Ok(())
    }

    pub(crate) fn transfer(
        &self,
        contract_id: ContractId,
        outputs: impl AsRef<[OutputSeal]>,
        secret_seals: impl AsRef<[SecretSeal]>,
        witness_id: Option<RgbTxid>,
        spv_resolver: Option<&dyn ResolveSpvProof>,
    ) -> Result<(RgbTransfer, RetrievedSpvProofs), Error> {
        self.stock
            .transfer(
                contract_id,
                outputs,
                secret_seals,
                [],
                witness_id,
                spv_resolver,
            )
            .map_err(Error::from)
    }

    pub(crate) fn transfer_from_fascia(
        &self,
        contract_id: ContractId,
        outputs: impl AsRef<[OutputSeal]>,
        secret_seals: impl AsRef<[SecretSeal]>,
        opids: impl IntoIterator<Item = OpId>,
        fascia: &Fascia,
        spv_resolver: Option<&dyn ResolveSpvProof>,
    ) -> Result<(RgbTransfer, RetrievedSpvProofs), Error> {
        self.stock
            .transfer_from_fascia(
                contract_id,
                outputs,
                secret_seals,
                opids,
                fascia,
                spv_resolver,
            )
            .map_err(Error::from)
    }

    #[cfg(feature = "_online")]
    pub(crate) fn transfer_from_fascia_with_dag(
        &self,
        contract_id: ContractId,
        outputs: impl AsRef<[OutputSeal]>,
        secret_seals: impl AsRef<[SecretSeal]>,
        opids: impl IntoIterator<Item = OpId>,
        fascia: &Fascia,
    ) -> Result<(RgbTransfer, OpoutsDagData, RetrievedSpvProofs), Error> {
        self.stock
            .transfer_from_fascia_with_dag(contract_id, outputs, secret_seals, opids, fascia, None)
            .map_err(Error::from)
    }

    pub(crate) fn transition_builder(
        &self,
        contract_id: ContractId,
        transition_name: impl Into<FieldName>,
    ) -> Result<TransitionBuilder, Error> {
        self.stock
            .transition_builder(contract_id, transition_name)
            .map_err(Error::from)
    }

    #[cfg(feature = "_online")]
    pub(crate) fn transition_builder_raw(
        &self,
        contract_id: ContractId,
        transition_type: TransitionType,
    ) -> Result<TransitionBuilder, Error> {
        self.stock
            .transition_builder_raw(contract_id, transition_type)
            .map_err(Error::from)
    }

    #[cfg(feature = "_online")]
    pub(crate) fn update_witnesses<R: ResolveWitness>(
        &mut self,
        resolver: &R,
        after_height: u32,
        force_witnesses: Vec<RgbTxid>,
    ) -> Result<UpdateRes, Error> {
        self.stock
            .update_witnesses(resolver, after_height, force_witnesses)
            .map_err(Error::from)
    }

    pub(crate) fn upsert_witness(
        &mut self,
        witness_id: RgbTxid,
        witness_ord: WitnessOrd,
    ) -> Result<(), Error> {
        self.stock.upsert_witness(witness_id, witness_ord)?;
        Ok(())
    }

    #[cfg(all(test, feature = "electrum"))]
    pub(crate) fn witness_spv_proof(&self, witness_id: RgbTxid) -> Result<Option<SpvProof>, Error> {
        self.stock.as_store().witness_spv_proof(witness_id)
    }
}

/// The wallet's on-chain witness resolver (the indexer-backed [`AnyResolver`]).
#[doc(hidden)]
#[cfg(feature = "_online")]
pub struct ChainResolver<'a> {
    pub(crate) inner: &'a AnyResolver,
}

#[cfg(feature = "_online")]
impl ResolveWitness for ChainResolver<'_> {
    fn resolve_witness(&self, witness_id: RgbTxid) -> Result<WitnessStatus, WitnessResolverError> {
        #[cfg(test)]
        if let Some((tx, height)) = mock_chain_tx(&witness_id.to_string()) {
            let height = NonZeroU32::new(height).expect("mock block height cannot be 0");
            let pos = WitnessPos::bitcoin(height, now().unix_timestamp())
                .expect("current time is after the genesis block");
            return Ok(WitnessStatus::Resolved(tx, WitnessOrd::Mined(pos)));
        }
        self.inner.resolve_witness(witness_id)
    }
    fn get_block_header(&self, height: NonZeroU32) -> Result<BlockHeader, WitnessResolverError> {
        self.inner.get_block_header(height)
    }
    fn check_chain_net(&self, chain_net: ChainNet) -> Result<(), WitnessResolverError> {
        self.inner.check_chain_net(chain_net)
    }
}

/// Witness resolver looking up the witness with the given ID (if any) in the consignment, as it may
/// not have been broadcast yet, and the other witnesses with the fallback chain resolver.
#[cfg(feature = "_online")]
pub(crate) struct OffchainResolver<'cons, 'r, R: ResolveWitness + ?Sized, const TRANSFER: bool> {
    pub(crate) witness_id: Option<RgbTxid>,
    pub(crate) consignment: &'cons Consignment<TRANSFER>,
    pub(crate) fallback: &'r R,
}

#[cfg(feature = "_online")]
impl<R: ResolveWitness + ?Sized, const TRANSFER: bool> ResolveWitness
    for OffchainResolver<'_, '_, R, TRANSFER>
{
    fn resolve_witness(&self, witness_id: RgbTxid) -> Result<WitnessStatus, WitnessResolverError> {
        if Some(witness_id) != self.witness_id {
            #[cfg(test)]
            if spv_only() {
                return Err(WitnessResolverError::ResolverIssue(
                    Some(witness_id),
                    s!("witness TX lookup refused (mock)"),
                ));
            }
            return self.fallback.resolve_witness(witness_id);
        }
        self.consignment
            .bundled_witnesses()
            .find(|bw| bw.witness_id() == witness_id)
            .map(|p| p.tx.clone())
            .map_or_else(
                || self.fallback.resolve_witness(witness_id),
                |tx| Ok(WitnessStatus::Resolved(tx, WitnessOrd::Tentative)),
            )
    }
    fn get_block_header(&self, height: NonZeroU32) -> Result<BlockHeader, WitnessResolverError> {
        // lets the SPV proofs carried by the consignment be verified via the indexer headers
        self.fallback.get_block_header(height)
    }
    fn check_chain_net(&self, chain_net: ChainNet) -> Result<(), WitnessResolverError> {
        self.fallback.check_chain_net(chain_net)
    }
}

/// Validate the given consignment with the given validation config, resolving the witness with
/// the given ID (if any) from the consignment itself, as it may not have been broadcast yet, and
/// the other witnesses with the given chain resolver.
///
/// External anchors (mint events) are confirmed on the chain the asset's bridge lives on, through
/// the EVM RPC URL configured for it in `evm_rpc_urls`, scanning for each mint event at most
/// `max_mint_scan_blocks` blocks after the one it commits to.
#[cfg(feature = "_online")]
pub(crate) fn validate_consignment_internal<const TRANSFER: bool>(
    consignment: &Consignment<TRANSFER>,
    offchain_witness_id: Option<RgbTxid>,
    chain_resolver: &(impl ResolveWitness + ?Sized),
    evm_rpc_urls: &HashMap<u64, String>,
    max_mint_scan_blocks: u64,
    validation_config: &ValidationConfig,
) -> Result<ValidConsignment<TRANSFER>, Error> {
    let validation_error = |e: ValidationError| match e {
        ValidationError::InvalidConsignment(e) => Error::ConsignmentValidationFailed {
            details: e.to_string(),
        },
        ValidationError::ResolverError(e) => Error::Network {
            details: e.to_string(),
        },
        e => Error::Internal {
            details: format!("unexpected consignment validation error: {e}"),
        },
    };

    let asset_schema = AssetSchema::try_from(consignment.schema_id())?;
    // phase 1: everything that only depends on the consignment itself
    let mut pending = consignment
        .clone()
        .validate_deterministic(&asset_schema.schema_rules(), validation_config)
        .map_err(validation_error)?;

    // phase 2: resolve what depends on the chains, then finalize. External anchors (mint events)
    // live on the chain the asset's bridge contract is deployed on and cannot be resolved by the
    // witness resolver: confirm them against that chain before resolving the witnesses
    let anchors: Vec<ExternalAnchor> = pending.pending().unresolved_anchors().cloned().collect();
    if !anchors.is_empty() {
        if !asset_schema.supports_mint() {
            return Err(Error::ConsignmentValidationFailed {
                details: format!("{asset_schema} schema does not support external anchors"),
            });
        }
        let bridge_location = BridgedFungibleAsset::bridge_location(consignment.genesis())
            .map_err(|e| Error::Internal {
                details: format!("unexpected bridge location error: {e}"),
            })?;
        let BridgeLocation::Evm(EvmContract { chain_id, .. }) = &bridge_location;
        let evm_rpc_url = evm_rpc_urls.get(chain_id).ok_or(Error::NoEvmRpcUrl {
            chain_id: *chain_id,
        })?;
        let evm_client = EvmClient::new(evm_rpc_url)?;
        let bounded_resolver = BoundedAnchorResolver::new(&evm_client, max_mint_scan_blocks);
        let anchor_resolver = CheckedAnchorResolver::with(&bounded_resolver, bridge_location)?;
        for anchor in &anchors {
            match pending.resolve_anchor(&anchor_resolver, anchor) {
                // tell a mint that will never be confirmed from one that may still be
                Err(AnchorResolverError::Unconfirmed(opid))
                    if bounded_resolver.is_exhausted(&opid) =>
                {
                    return Err(Error::MintEventOutOfScanWindow {
                        opid: opid.to_string(),
                        max_scan_blocks: max_mint_scan_blocks,
                    });
                }
                res => res?,
            }
        }
    }

    // then resolve the witnesses
    let resolver = OffchainResolver {
        witness_id: offchain_witness_id,
        consignment,
        fallback: chain_resolver,
    };
    pending
        .pending()
        .resolve_all(&resolver)
        .map_err(validation_error)?;
    Ok(pending.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct MandatoryField {
        #[serde(deserialize_with = "from_str_or_number_mandatory")]
        val: u64,
    }

    #[derive(Debug, Deserialize)]
    struct OptionalField {
        #[serde(deserialize_with = "from_str_or_number_optional")]
        val: Option<u64>,
    }

    #[test]
    fn test_block_on_inside_tokio_runtime() {
        // calling block_on from within an active Tokio runtime takes the thread-spawn path
        // to avoid blocking the runtime thread
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { block_on(async { 42u32 }) });
        assert_eq!(result, 42);
    }

    #[test]
    fn test_from_str_or_number_mandatory() {
        // integer value
        let result: MandatoryField = serde_json::from_str(r#"{"val": 42}"#).unwrap();
        assert_eq!(result.val, 42);

        // float value (visit_f64 path)
        let result: MandatoryField = serde_json::from_str(r#"{"val": 42.0}"#).unwrap();
        assert_eq!(result.val, 42);

        // string value
        let result: MandatoryField = serde_json::from_str(r#"{"val": "99"}"#).unwrap();
        assert_eq!(result.val, 99);

        // null -> error
        let err = serde_json::from_str::<MandatoryField>(r#"{"val": null}"#).unwrap_err();
        assert!(
            err.to_string().contains("expected a number but got null"),
            "unexpected error message: {err}"
        );

        // invalid string -> parse error
        let err = serde_json::from_str::<MandatoryField>(r#"{"val": "abc"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid value"),
            "unexpected error message: {err}"
        );

        // unexpected type (bool) -> error names the expected types via `expecting`
        let err = serde_json::from_str::<MandatoryField>(r#"{"val": true}"#).unwrap_err();
        assert!(
            err.to_string().contains("a string, a number, or null"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn test_from_str_or_number_optional() {
        // integer value
        let result: OptionalField = serde_json::from_str(r#"{"val": 42}"#).unwrap();
        assert_eq!(result.val, Some(42));

        // float value (visit_f64 path)
        let result: OptionalField = serde_json::from_str(r#"{"val": 42.0}"#).unwrap();
        assert_eq!(result.val, Some(42));

        // string value
        let result: OptionalField = serde_json::from_str(r#"{"val": "99"}"#).unwrap();
        assert_eq!(result.val, Some(99));

        // null -> None (visit_unit path)
        let result: OptionalField = serde_json::from_str(r#"{"val": null}"#).unwrap();
        assert_eq!(result.val, None);

        // invalid string -> parse error
        let err = serde_json::from_str::<OptionalField>(r#"{"val": "abc"}"#).unwrap_err();
        assert!(
            err.to_string().contains("invalid value"),
            "unexpected error message: {err}"
        );

        // visit_none path: triggered by deserializer formats that signal absence via visit_none
        // rather than visit_unit (serde_json uses visit_unit for null, but other formats differ)
        struct VisitNoneDeserializer;
        impl<'de> Deserializer<'de> for VisitNoneDeserializer {
            type Error = serde::de::value::Error;
            fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
                visitor.visit_none()
            }
            serde::forward_to_deserialize_any! {
                bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
                bytes byte_buf option unit unit_struct newtype_struct seq tuple
                tuple_struct map struct enum identifier ignored_any
            }
        }
        let result: Option<u64> = from_str_or_number_optional(VisitNoneDeserializer).unwrap();
        assert_eq!(result, None);
    }

    #[cfg(feature = "_online")]
    #[test]
    fn test_check_proxy_json_rpc_error() {
        // server returns HTTP 200 with result=null and a JSON-RPC error field
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"jsonrpc":"2.0","id":null,"result":null,"error":{"code":-32601,"message":"method not found"}}"#,
            )
            .create();
        let result = check_proxy(&server.url());
        assert_matches!(result, Err(Error::Proxy { details }) if details == "method not found");
        mock.assert();
    }

    #[cfg(feature = "_online")]
    #[test]
    fn test_check_proxy_invalid_body_error_details() {
        // server returns HTTP 200 with a body that is not a JSON-RPC response: the transport
        // error (body decoding) must be reported, not the generic connection message
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not json")
            .create();
        let result = check_proxy(&server.url());
        let expected = format!("error decoding response body for url ({}/)", server.url());
        assert_matches!(result, Err(Error::Proxy { details }) if details == expected);
        mock.assert();
    }

    #[cfg(feature = "_online")]
    #[test]
    fn test_check_proxy_connection_error_details() {
        // bind then drop a listener to get a port that refuses connections: the transport error
        // (connection refused) must be reported, not the generic connection message
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let result = check_proxy(&format!("http://127.0.0.1:{port}"));
        let expected = format!("error sending request for url (http://127.0.0.1:{port}/)");
        assert_matches!(result, Err(Error::Proxy { details }) if details == expected);
    }

    #[test]
    fn test_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        // parent dir does not exist yet: it must be created
        let path = dir.path().join("sub").join("f.bin");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        // overwrites cleanly
        atomic_write(&path, b"world!!").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"world!!");
        // no temp file is left behind
        let tmp = path.parent().unwrap().join(".f.bin.tmp");
        assert!(!tmp.exists());
        // a failing write leaves the previous file intact and no partial temp file behind
        let res = atomic_write_with(&path, |tmp| {
            fs::write(tmp, b"partial")?;
            Err(Error::Internal {
                details: s!("write failed"),
            })
        });
        assert!(matches!(res, Err(Error::Internal { .. })));
        assert_eq!(fs::read(&path).unwrap(), b"world!!");
        assert!(!tmp.exists());
    }

    // The None return from detect_indexer is only reachable when electrum is enabled but esplora
    // is not, and the URL is not a valid electrum server. With esplora enabled detection always
    // falls back to Some(IndexerConfig::Esplora) instead.
    #[cfg(all(feature = "electrum", not(feature = "esplora")))]
    #[test]
    fn test_detect_indexer_invalid_url_returns_none() {
        let result = detect_indexer("not_a_valid_url", None);
        assert!(result.is_none());
    }

    #[cfg(feature = "_online")]
    #[test]
    fn test_build_indexer_invalid_url() {
        let result = build_indexer(&IndexerConfig::Electrum {
            url: s!("not_a_valid_url"),
        });
        #[cfg(feature = "electrum")]
        let details = "not a valid electrum server";
        #[cfg(not(feature = "electrum"))]
        let details = "electrum support is not enabled (requires the `electrum` feature)";
        // Indexer is not Debug, so the Ok variant cannot be printed by assert_matches
        assert!(matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details));
    }

    #[test]
    fn test_bitcoin_network_str_roundtrip() {
        // mainnet
        let network = BitcoinNetwork::Mainnet;
        let network_str = network.to_string();
        let network_from_str = BitcoinNetwork::from_str(&network_str).unwrap();
        assert_eq!(network, network_from_str);

        // testnet3
        let network = BitcoinNetwork::Testnet;
        let network_str = network.to_string();
        let network_from_str = BitcoinNetwork::from_str(&network_str).unwrap();
        assert_eq!(network, network_from_str);

        // testnet4
        let network = BitcoinNetwork::Testnet4;
        let network_str = network.to_string();
        let network_from_str = BitcoinNetwork::from_str(&network_str).unwrap();
        assert_eq!(network, network_from_str);

        // signet
        let network = BitcoinNetwork::Signet;
        let network_str = network.to_string();
        let network_from_str = BitcoinNetwork::from_str(&network_str).unwrap();
        assert_eq!(network, network_from_str);

        // regtest
        let network = BitcoinNetwork::Regtest;
        let network_str = network.to_string();
        let network_from_str = BitcoinNetwork::from_str(&network_str).unwrap();
        assert_eq!(network, network_from_str);

        // signet custom
        let network = BitcoinNetwork::SignetCustom;
        let network_str = network.to_string();
        let network_from_str = BitcoinNetwork::from_str(&network_str).unwrap();
        assert_eq!(network, network_from_str);

        // invalid network
        let network_str = "invalid";
        let result = BitcoinNetwork::from_str(network_str).unwrap_err();
        assert_matches!(result, Error::InvalidBitcoinNetwork { network } if network == "invalid");

        // signet- prefix with invalid hash
        let network_str = "signet-notahash";
        let result = BitcoinNetwork::from_str(network_str).unwrap_err();
        assert_matches!(result, Error::InvalidBitcoinNetwork { network } if network == "signet-notahash");
    }

    #[test]
    fn test_bitcoin_network_chain_net_roundtrip() {
        // mainnet
        let network = BitcoinNetwork::Mainnet;
        let chain_net = ChainNet::BitcoinMainnet;
        let network_from_chain_net = BitcoinNetwork::try_from(chain_net).unwrap();
        assert_eq!(network, network_from_chain_net);
        let chain_net_from_network = ChainNet::from(network);
        assert_eq!(chain_net, chain_net_from_network);

        // testnet3
        let network = BitcoinNetwork::Testnet;
        let chain_net = ChainNet::BitcoinTestnet3;
        let network_from_chain_net = BitcoinNetwork::try_from(chain_net).unwrap();
        assert_eq!(network, network_from_chain_net);
        let chain_net_from_network = ChainNet::from(network);
        assert_eq!(chain_net, chain_net_from_network);

        // testnet4
        let network = BitcoinNetwork::Testnet4;
        let chain_net = ChainNet::BitcoinTestnet4;
        let network_from_chain_net = BitcoinNetwork::try_from(chain_net).unwrap();
        assert_eq!(network, network_from_chain_net);
        let chain_net_from_network = ChainNet::from(network);
        assert_eq!(chain_net, chain_net_from_network);

        // signet
        let network = BitcoinNetwork::Signet;
        let chain_net = ChainNet::BitcoinSignet;
        let network_from_chain_net = BitcoinNetwork::try_from(chain_net).unwrap();
        assert_eq!(network, network_from_chain_net);
        let chain_net_from_network = ChainNet::from(network);
        assert_eq!(chain_net, chain_net_from_network);

        // regtest
        let network = BitcoinNetwork::Regtest;
        let chain_net = ChainNet::BitcoinRegtest;
        let network_from_chain_net = BitcoinNetwork::try_from(chain_net).unwrap();
        assert_eq!(network, network_from_chain_net);
        let chain_net_from_network = ChainNet::from(network);
        assert_eq!(chain_net, chain_net_from_network);

        // signet custom
        let network = BitcoinNetwork::SignetCustom;
        let chain_net = ChainNet::BitcoinSignetCustom;
        let network_from_chain_net = BitcoinNetwork::try_from(chain_net).unwrap();
        assert_eq!(network, network_from_chain_net);
        let chain_net_from_network = ChainNet::from(network);
        assert_eq!(chain_net, chain_net_from_network);

        // invalid chain net
        let chain_net = ChainNet::LiquidMainnet;
        let result = BitcoinNetwork::try_from(chain_net).unwrap_err();
        assert_matches!(result, Error::UnsupportedLayer1 { layer_1 } if layer_1 == "liquid");
    }

    #[test]
    fn test_bitcoin_network_try_from_rust_bitcoin_network() {
        // mainnet
        let network = BitcoinNetwork::Mainnet;
        let rust_bitcoin_network = bitcoin::Network::from(network);
        assert_eq!(rust_bitcoin_network, bitcoin::Network::Bitcoin);

        // testnet3
        let network = BitcoinNetwork::Testnet;
        let rust_bitcoin_network = bitcoin::Network::from(network);
        assert_eq!(rust_bitcoin_network, bitcoin::Network::Testnet);

        // testnet4
        let network = BitcoinNetwork::Testnet4;
        let rust_bitcoin_network = bitcoin::Network::from(network);
        assert_eq!(rust_bitcoin_network, bitcoin::Network::Testnet4);

        // signet
        let network = BitcoinNetwork::Signet;
        let rust_bitcoin_network = bitcoin::Network::from(network);
        assert_eq!(rust_bitcoin_network, bitcoin::Network::Signet);

        // regtest
        let network = BitcoinNetwork::Regtest;
        let rust_bitcoin_network = bitcoin::Network::from(network);
        assert_eq!(rust_bitcoin_network, bitcoin::Network::Regtest);

        // signet custom
        let network = BitcoinNetwork::SignetCustom;
        let rust_bitcoin_network = bitcoin::Network::from(network);
        assert_eq!(rust_bitcoin_network, bitcoin::Network::Signet);
    }

    /// Start a mock electrum server that accepts a single connection, refusing any further one,
    /// and answers the requests needed to check it can be reached, returning its URL.
    #[cfg(feature = "electrum")]
    fn start_single_connection_electrum_mock() -> String {
        use std::io::{BufRead, BufReader};
        use std::net::TcpListener;

        const GENESIS_HEADER_HEX: &str = "0100000000000000000000000000000000000000000000000000000000000000000000003ba3edfd7a7b12b27ac72c3e67768f617fc81bc3888a51323a9fb8aa4b1e5e4a29ab5f49ffff001d1dac2b7c";

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock electrum");
        let url = format!(
            "tcp://{}",
            listener.local_addr().expect("mock electrum addr")
        );
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("mock electrum accept");
            // close the listener before answering, so the next connection is refused
            drop(listener);
            let mut reader = BufReader::new(stream.try_clone().expect("clone mock stream"));
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                let req: serde_json::Value =
                    serde_json::from_str(line.trim()).expect("parse mock electrum request");
                let result = match req["method"].as_str() {
                    Some("server.version") => s!(r#"["mock-electrum","1.6"]"#),
                    Some("blockchain.block.header") => format!(r#""{GENESIS_HEADER_HEX}""#),
                    _ => return,
                };
                let id = &req["id"];
                if writeln!(stream, r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#).is_err() {
                    return;
                }
                line.clear();
            }
        });
        url
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn get_indexer_and_resolver_resolver_connection_refused() {
        // the indexer connects, then the resolver connection is refused
        let url = start_single_connection_electrum_mock();
        let result =
            get_indexer_and_resolver(&IndexerConfig::Electrum { url }, BitcoinNetwork::Regtest);
        // Indexer is not Debug, so the Ok variant cannot be printed by assert_matches
        assert!(matches!(result, Err(Error::InvalidIndexer { .. })));
    }
}
