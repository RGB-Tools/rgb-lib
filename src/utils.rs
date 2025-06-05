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
pub(crate) const COIN_RGB_MAINNET: u32 = 827166;
pub(crate) const COIN_RGB_TESTNET: u32 = 827167;
pub(crate) const ACCOUNT: u8 = 0;
pub(crate) const KEYCHAIN_RGB: u8 = 0;
pub(crate) const KEYCHAIN_BTC: u8 = 0;

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) const INDEXER_STOP_GAP: usize = 20;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) const INDEXER_TIMEOUT: u8 = 10;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) const INDEXER_RETRIES: u8 = 3;
#[cfg(feature = "electrum")]
pub(crate) const INDEXER_BATCH_SIZE: usize = 5;
#[cfg(feature = "esplora")]
pub(crate) const INDEXER_PARALLEL_REQUESTS: usize = 5;

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) const PROXY_TIMEOUT: u8 = 90;
#[cfg(any(feature = "electrum", feature = "esplora"))]
const PROXY_PROTOCOL_VERSION: &str = "0.2";

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
        write!(f, "{self:?}")
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
                });
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
            ChainNet::BitcoinTestnet3 => Ok(BitcoinNetwork::Testnet),
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

#[cfg(feature = "electrum")]
fn get_valid_txid_for_network(bitcoin_network: &BitcoinNetwork) -> String {
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

pub(crate) fn str_to_xpub(xpub: &str, bdk_network: BdkNetwork) -> Result<Xpub, Error> {
    let pubkey_btc = Xpub::from_str(xpub)?;
    let extended_key_btc: ExtendedKey = ExtendedKey::from(pubkey_btc);
    Ok(extended_key_btc.into_xpub(bdk_network, &Secp256k1::new()))
}

pub(crate) fn get_coin_type(bitcoin_network: &BitcoinNetwork, rgb: bool) -> u32 {
    match (bitcoin_network, rgb) {
        (BitcoinNetwork::Mainnet, true) => COIN_RGB_MAINNET,
        (_, true) => COIN_RGB_TESTNET,
        (_, false) => u32::from(*bitcoin_network != BitcoinNetwork::Mainnet),
    }
}

pub(crate) fn get_account_derivation_children(coin_type: u32) -> Vec<ChildNumber> {
    vec![
        ChildNumber::from_hardened_idx(PURPOSE as u32).unwrap(),
        ChildNumber::from_hardened_idx(coin_type).unwrap(),
        ChildNumber::from_hardened_idx(ACCOUNT as u32).unwrap(),
    ]
}

fn derive_account_xprv_from_mnemonic(
    bitcoin_network: BitcoinNetwork,
    mnemonic: &str,
    rgb: bool,
) -> Result<(Xpriv, Fingerprint), Error> {
    let coin_type = get_coin_type(&bitcoin_network, rgb);
    let account_derivation_children = get_account_derivation_children(coin_type);
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic.to_string())?;
    let master_xprv = Xpriv::new_master(bitcoin_network, &mnemonic.to_seed("")).unwrap();
    let master_xpub = Xpub::from_priv(&Secp256k1::new(), &master_xprv);
    let master_fingerprint = master_xpub.fingerprint();
    let account_xprv = master_xprv.derive_priv(&Secp256k1::new(), &account_derivation_children)?;
    Ok((account_xprv, master_fingerprint))
}

fn get_xpub_from_xprv(xprv: &Xpriv) -> Xpub {
    Xpub::from_priv(&Secp256k1::new(), xprv)
}

/// Get the account-level xPriv and xPub for the given mnemonic and Bitcoin network based on the
/// requested wallet side (colored or vanilla)
pub fn get_account_data(
    bitcoin_network: BitcoinNetwork,
    mnemonic: &str,
    rgb: bool,
) -> Result<(Xpriv, Xpub, Fingerprint), Error> {
    let (account_xprv, master_fingerprint) =
        derive_account_xprv_from_mnemonic(bitcoin_network, mnemonic, rgb)?;
    let account_xpub = get_xpub_from_xprv(&account_xprv);
    Ok((account_xprv, account_xpub, master_fingerprint))
}

pub(crate) fn get_account_xpubs(
    bitcoin_network: BitcoinNetwork,
    mnemonic: &str,
) -> Result<(Xpub, Xpub), Error> {
    let (_, account_xpub_vanilla, _) = get_account_data(bitcoin_network, mnemonic, false)?;
    let (_, account_xpub_colored, _) = get_account_data(bitcoin_network, mnemonic, true)?;
    Ok((account_xpub_vanilla, account_xpub_colored))
}

fn derive_descriptor(
    bitcoin_network: BitcoinNetwork,
    mnemonic: &str,
    rgb: bool,
    keychain: u8,
    expected_xpub: Xpub,
) -> Result<String, Error> {
    let (account_xprv, account_xpub, master_fingerprint) =
        get_account_data(bitcoin_network, mnemonic, rgb)?;
    if account_xpub != expected_xpub {
        return Err(Error::InvalidBitcoinKeys);
    }
    let coin_type = get_coin_type(&bitcoin_network, rgb);
    calculate_descriptor_from_xprv(&master_fingerprint, coin_type, account_xprv, keychain)
}

pub(crate) fn get_descriptors(
    bitcoin_network: BitcoinNetwork,
    mnemonic: &str,
    vanilla_keychain: Option<u8>,
    expected_xpub_btc: Xpub,
    expected_xpub_rgb: Xpub,
) -> Result<(String, String), Error> {
    let descriptor_colored = derive_descriptor(
        bitcoin_network,
        mnemonic,
        true,
        KEYCHAIN_RGB,
        expected_xpub_rgb,
    )?;
    let descriptor_vanilla = derive_descriptor(
        bitcoin_network,
        mnemonic,
        false,
        vanilla_keychain.unwrap_or(KEYCHAIN_BTC),
        expected_xpub_btc,
    )?;
    Ok((descriptor_colored, descriptor_vanilla))
}

pub(crate) fn get_descriptors_from_xpubs(
    bitcoin_network: BitcoinNetwork,
    master_fingerprint: &str,
    xpub_rgb: Xpub,
    xpub_btc: Xpub,
    vanilla_keychain: Option<u8>,
) -> Result<(String, String), Error> {
    let master_fingerprint =
        Fingerprint::from_str(master_fingerprint).map_err(|_| Error::InvalidFingerprint)?;
    let descriptor_colored = calculate_descriptor_from_xpub(
        &master_fingerprint,
        get_coin_type(&bitcoin_network, true),
        xpub_rgb,
        KEYCHAIN_RGB,
    )?;
    let descriptor_vanilla = calculate_descriptor_from_xpub(
        &master_fingerprint,
        get_coin_type(&bitcoin_network, false),
        xpub_btc,
        vanilla_keychain.unwrap_or(KEYCHAIN_BTC),
    )?;
    Ok((descriptor_colored, descriptor_vanilla))
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
            let script_pubkey = pay_2_vout.script_pubkey();
            let script_bytes = script_pubkey.as_script_bytes();
            let script_bytes_vec = script_bytes.clone().into_vec();
            let script_buf = ScriptBuf::from_bytes(script_bytes_vec);
            Ok(Some(script_buf))
        }
        Beneficiary::BlindedSeal(_) => Ok(None),
    }
}

pub(crate) fn beneficiary_from_script_buf(script_buf: ScriptBuf) -> Beneficiary {
    let address_payload =
        AddressPayload::from_script(&ScriptPubkey::try_from(script_buf.into_bytes()).unwrap())
            .unwrap();
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
) -> Result<String, Error> {
    // derive final xpub from account-level xpub
    let path = get_derivation_path(keychain);
    let der_xprv = &xprv
        .derive_priv(&Secp256k1::new(), &path)
        .expect("provided path should be derivable in an xprv");
    // derive descriptor with master fingerprint and full derivation path
    let account_derivation_children = get_account_derivation_children(coin_type);
    let full_path = get_extended_derivation_path(account_derivation_children, keychain);
    let origin_prv: KeySource = (*master_fingerprint, full_path.clone());
    let der_xprv_desc_key: DescriptorKey<Segwitv0> = der_xprv
        .into_descriptor_key(Some(origin_prv), DerivationPath::default())
        .expect("should be able to convert xprv in a descriptor key");
    let key = if let Secret(key, _, _) = der_xprv_desc_key {
        key
    } else {
        return Err(InternalError::Unexpected)?;
    };
    Ok(format!("tr({key})"))
}

pub(crate) fn calculate_descriptor_from_xpub(
    master_fingerprint: &Fingerprint,
    coin_type: u32,
    xpub: Xpub,
    keychain: u8,
) -> Result<String, Error> {
    // derive final xpub from account-level xpub
    let path = get_derivation_path(keychain);
    let der_xpub = &xpub
        .derive_pub(&Secp256k1::new(), &path)
        .expect("provided path should be derivable in an xpub");
    // derive descriptor with master fingerprint and full derivation path
    let account_derivation_children = get_account_derivation_children(coin_type);
    let full_path = get_extended_derivation_path(account_derivation_children, keychain);
    let origin_pub: KeySource = (*master_fingerprint, full_path);
    let der_xpub_desc_key: DescriptorKey<Segwitv0> = der_xpub
        .into_descriptor_key(Some(origin_pub), DerivationPath::default())
        .expect("should be able to convert xpub in a descriptor key");
    let key = if let Public(key, _, _) = der_xpub_desc_key {
        key
    } else {
        return Err(InternalError::Unexpected)?;
    };
    Ok(format!("tr({key})"))
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn check_genesis_hash(bitcoin_network: &BitcoinNetwork, indexer: &Indexer) -> Result<(), Error> {
    let expected = get_genesis_hash(bitcoin_network);
    let block_hash = indexer.block_hash(0)?;
    if expected != block_hash {
        return Err(Error::InvalidIndexer {
            details: s!("indexer is for a network different from the wallet's one"),
        });
    }

    Ok(())
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_proxy_client() -> Result<RestClient, Error> {
    Ok(RestClient::builder()
        .timeout(Duration::from_secs(PROXY_TIMEOUT as u64))
        .build()?)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn check_proxy(proxy_url: &str, rest_client: Option<&RestClient>) -> Result<(), Error> {
    let rest_client = if let Some(rest_client) = rest_client {
        rest_client.clone()
    } else {
        get_proxy_client()?
    };
    let mut err_details = s!("unable to connect to proxy");
    if let Ok(server_info) = rest_client.clone().get_info(proxy_url) {
        if let Some(info) = server_info.result {
            if info.protocol_version == *PROXY_PROTOCOL_VERSION {
                return Ok(());
            } else {
                return Err(Error::InvalidProxyProtocol {
                    version: info.protocol_version,
                });
            }
        }
        if let Some(err) = server_info.error {
            err_details = err.message;
        }
    };
    Err(Error::Proxy {
        details: err_details,
    })
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_indexer(
    indexer_url: &str,
    bitcoin_network: BitcoinNetwork,
) -> Result<Indexer, Error> {
    // detect indexer type
    let indexer = build_indexer(indexer_url);
    let mut invalid_indexer = true;
    if let Some(ref indexer) = indexer {
        invalid_indexer = indexer.block_hash(0).is_err();
    }
    if invalid_indexer {
        return Err(Error::InvalidIndexer {
            details: s!("not a valid electrum nor esplora server"),
        });
    }
    let indexer = indexer.unwrap();

    // check the indexer server is for the correct network
    check_genesis_hash(&bitcoin_network, &indexer)?;

    #[cfg(feature = "electrum")]
    if matches!(indexer, Indexer::Electrum(_)) {
        // check the electrum server has the required functionality (verbose transactions)
        indexer
            .get_tx_confirmations(&get_valid_txid_for_network(&bitcoin_network))
            .map_err(|_| Error::InvalidElectrum {
                details: s!("verbose transactions are currently unsupported"),
            })?;
    }

    Ok(indexer)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn build_indexer(indexer_url: &str) -> Option<Indexer> {
    #[cfg(feature = "electrum")]
    {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let opts = ConfigBuilder::new()
            .retry(INDEXER_RETRIES)
            .timeout(Some(INDEXER_TIMEOUT))
            .build();
        if let Ok(client) = ElectrumClient::from_config(indexer_url, opts) {
            let client = BdkElectrumClient::new(client);
            let indexer = Indexer::Electrum(Box::new(client));
            return Some(indexer);
        }
    }
    if cfg!(feature = "esplora") {
        #[cfg(feature = "esplora")]
        {
            let opts = EsploraBuilder::new(indexer_url)
                .max_retries(INDEXER_RETRIES.into())
                .timeout(INDEXER_TIMEOUT.into());
            let client = EsploraClient::from_builder(opts);
            let indexer = Indexer::Esplora(Box::new(client));
            return Some(indexer);
        }
    }
    None
}

fn convert_time_fmt_error(cause: time::error::Format) -> io::Error {
    io::Error::other(cause)
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
    fn resolve_pub_witness(&self, _: RgbTxid) -> Result<Tx, WitnessResolverError> {
        unreachable!()
    }

    fn resolve_pub_witness_ord(&self, _: RgbTxid) -> Result<WitnessOrd, WitnessResolverError> {
        unreachable!()
    }

    fn check_chain_net(&self, _: ChainNet) -> Result<(), WitnessResolverError> {
        Ok(())
    }
}

/// Wrapper for the RGB stock and its lockfile.
pub(crate) struct RgbRuntime {
    /// The RGB stock
    stock: Stock,
    /// The wallet directory, where the lockfile for the runtime is to be held
    wallet_dir: PathBuf,
}

impl RgbRuntime {
    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn accept_transfer<R: ResolveWitness>(
        &mut self,
        contract: ValidTransfer,
        resolver: &R,
    ) -> Result<Status, InternalError> {
        self.stock
            .accept_transfer(contract, resolver)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn consume_fascia(
        &mut self,
        fascia: Fascia,
        witness_id: RgbTxid,
        witness_ord: Option<WitnessOrd>,
    ) -> Result<(), InternalError> {
        struct FasciaResolver {
            witness_id: RgbTxid,
            witness_ord: WitnessOrd,
        }
        impl ResolveWitness for FasciaResolver {
            fn resolve_pub_witness(&self, _: RgbTxid) -> Result<Tx, WitnessResolverError> {
                unreachable!()
            }
            fn resolve_pub_witness_ord(
                &self,
                witness_id: RgbTxid,
            ) -> Result<WitnessOrd, WitnessResolverError> {
                debug_assert_eq!(witness_id, self.witness_id);
                Ok(self.witness_ord)
            }
            fn check_chain_net(&self, _: ChainNet) -> Result<(), WitnessResolverError> {
                unreachable!()
            }
        }

        let resolver = FasciaResolver {
            witness_id,
            witness_ord: witness_ord.unwrap_or(WitnessOrd::Tentative),
        };

        self.stock
            .consume_fascia(fascia, resolver)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn contracts(&self) -> Result<Vec<ContractInfo>, InternalError> {
        Ok(self
            .stock
            .contracts()
            .map_err(InternalError::from)?
            .collect())
    }

    pub(crate) fn contract_wrapper<C: IssuerWrapper>(
        &self,
        contract_id: ContractId,
    ) -> Result<C::Wrapper<MemContract<&MemContractState>>, InternalError> {
        self.stock
            .contract_wrapper::<C>(contract_id)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn contracts_assigning(
        &self,
        outputs: impl IntoIterator<Item = impl Into<RgbOutpoint>>,
    ) -> Result<BTreeSet<ContractId>, InternalError> {
        Ok(FromIterator::from_iter(
            self.stock
                .contracts_assigning(outputs)
                .map_err(InternalError::from)?,
        ))
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
        self.stock
            .as_stash_provider()
            .genesis(contract_id)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn import_contract<R: ResolveWitness>(
        &mut self,
        contract: ValidContract,
        resolver: &R,
    ) -> Result<Status, InternalError> {
        self.stock
            .import_contract(contract, resolver)
            .map_err(InternalError::from)
    }

    pub(crate) fn import_kit(&mut self, kit: ValidKit) -> Result<Status, InternalError> {
        self.stock.import_kit(kit).map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn contract_assignments_for(
        &self,
        contract_id: ContractId,
        outpoints: impl IntoIterator<Item = impl Into<RgbOutpoint>>,
    ) -> Result<HashMap<OutputSeal, HashMap<Opout, AllocatedState>>, InternalError> {
        self.stock
            .contract_assignments_for(contract_id, outpoints)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn contract_schema(
        &self,
        contract_id: ContractId,
    ) -> Result<&Schema, InternalError> {
        self.stock
            .as_stash_provider()
            .contract_schema(contract_id)
            .map_err(InternalError::from)
    }

    pub(crate) fn schemata(&self) -> Result<Vec<SchemaInfo>, InternalError> {
        Ok(self
            .stock
            .schemata()
            .map_err(InternalError::from)?
            .collect())
    }

    pub(crate) fn store_secret_seal(&mut self, seal: GraphSeal) -> Result<bool, InternalError> {
        self.stock
            .store_secret_seal(seal)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn transfer(
        &self,
        contract_id: ContractId,
        outputs: impl AsRef<[OutputSeal]>,
        secret_seals: impl AsRef<[SecretSeal]>,
        witness_id: Option<RgbTxid>,
    ) -> Result<RgbTransfer, InternalError> {
        self.stock
            .transfer(contract_id, outputs, secret_seals, witness_id)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn transition_builder(
        &self,
        contract_id: ContractId,
        transition_name: impl Into<FieldName>,
    ) -> Result<TransitionBuilder, InternalError> {
        self.stock
            .transition_builder(contract_id, transition_name)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn transition_builder_raw(
        &self,
        contract_id: ContractId,
        transition_type: TransitionType,
    ) -> Result<TransitionBuilder, InternalError> {
        self.stock
            .transition_builder_raw(contract_id, transition_type)
            .map_err(InternalError::from)
    }

    #[cfg_attr(not(any(feature = "electrum", feature = "esplora")), allow(dead_code))]
    pub(crate) fn update_witnesses<R: ResolveWitness>(
        &mut self,
        resolver: &R,
        after_height: u32,
        force_witnesses: Vec<RgbTxid>,
    ) -> Result<UpdateRes, InternalError> {
        self.stock
            .update_witnesses(resolver, after_height, force_witnesses)
            .map_err(InternalError::from)
    }
}

impl Drop for RgbRuntime {
    fn drop(&mut self) {
        self.stock.store().expect("unable to save stock");
        fs::remove_file(self.wallet_dir.join(RGB_RUNTIME_LOCK_FILE))
            .expect("should be able to drop lockfile")
    }
}

fn _write_rgb_runtime_lockfile(wallet_dir: &Path) -> Result<(), Error> {
    let lock_file_path = wallet_dir.join(RGB_RUNTIME_LOCK_FILE);
    let t_0 = OffsetDateTime::now_utc();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_file_path.clone())
        {
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 3600.0 {
                    return Err(Error::Internal {
                        details: s!("unreleased lock file"),
                    });
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(400))
                }
            }
            Err(e) => {
                return Err(Error::IO {
                    details: e.to_string(),
                });
            }
        }
    }
}

pub(crate) fn load_rgb_runtime(wallet_dir: PathBuf) -> Result<RgbRuntime, Error> {
    _write_rgb_runtime_lockfile(&wallet_dir)?;

    let rgb_dir = wallet_dir.join(RGB_RUNTIME_DIR);
    if !rgb_dir.exists() {
        fs::create_dir_all(&rgb_dir)?;
    }
    let provider = FsBinStore::new(rgb_dir.clone())?;
    let stock = Stock::load(provider.clone(), true).or_else(|err| {
        if err
            .0
            .downcast_ref::<DeserializeError>()
            .map(|e| matches!(e, DeserializeError::Decode(DecodeError::Io(e)) if e.kind() == ErrorKind::NotFound))
            .unwrap_or_default()
        {
            let mut stock = Stock::in_memory();
            stock.make_persistent(provider, true).expect("unable to save stock");
            return Ok(stock)
        }
        Err(Error::IO { details: err.to_string() })
    })?;

    Ok(RgbRuntime { stock, wallet_dir })
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) struct OffchainResolver<'a, 'cons, const TRANSFER: bool> {
    pub(crate) witness_id: RgbTxid,
    pub(crate) consignment: &'cons IndexedConsignment<'cons, TRANSFER>,
    pub(crate) fallback: &'a AnyResolver,
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl<const TRANSFER: bool> ResolveWitness for OffchainResolver<'_, '_, TRANSFER> {
    fn resolve_pub_witness(&self, witness_id: RgbTxid) -> Result<Tx, WitnessResolverError> {
        if witness_id != self.witness_id {
            return self.fallback.resolve_pub_witness(witness_id);
        }
        self.consignment
            .pub_witness(witness_id)
            .and_then(|pw| pw.tx().cloned())
            .ok_or(WitnessResolverError::Unknown(witness_id))
            .or_else(|_| self.fallback.resolve_pub_witness(witness_id))
    }
    fn resolve_pub_witness_ord(
        &self,
        witness_id: RgbTxid,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        if witness_id != self.witness_id {
            return self.fallback.resolve_pub_witness_ord(witness_id);
        }
        Ok(WitnessOrd::Tentative)
    }
    fn check_chain_net(&self, chain_net: ChainNet) -> Result<(), WitnessResolverError> {
        self.fallback.check_chain_net(chain_net)
    }
}
