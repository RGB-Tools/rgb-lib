//! RGB utilities
//!
//! This module defines some utility methods.

use amplify::s;
use bdk::bitcoin::bip32::ExtendedPrivKey;
use bdk::bitcoin::bip32::{DerivationPath, ExtendedPubKey, KeySource};
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::Network as BdkNetwork;
use bdk::descriptor::Segwitv0;
use bdk::keys::bip39::{Language, Mnemonic};
use bdk::keys::DescriptorKey::{Public, Secret};
use bdk::keys::{DerivableKey, DescriptorKey, DescriptorSecretKey};
use bdk::miniscript::DescriptorPublicKey;
use bitcoin::bip32::ChildNumber;
use bp::{Outpoint, Txid};
use commit_verify::mpc::MerkleBlock;
use rgb::Runtime;
use rgb_core::validation::Status;
use rgb_core::{
    Anchor, ContractId, Genesis, GenesisSeal, GraphSeal, Opout, SchemaId, SubSchema,
    TransitionBundle,
};
use rgbstd::containers::{Bindle, BuilderSeal, Contract, Transfer};
use rgbstd::interface::{ContractIface, Iface, IfaceId, IfaceImpl, TransitionBuilder, TypedState};
use rgbstd::persistence::{Inventory, Stash};
use rgbstd::resolvers::ResolveHeight;
use rgbstd::Chain as RgbNetwork;
use serde::{Deserialize, Serialize};
use slog::{Drain, Logger};
use slog_term::{FullFormat, PlainDecorator};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::Path;
use std::str::FromStr;
use std::{fs::OpenOptions, path::PathBuf};
use strict_encoding::TypeName;
use time::OffsetDateTime;

use crate::error::InternalError;
use crate::Error;

const TIMESTAMP_FORMAT: &[time::format_description::FormatItem] = time::macros::format_description!(
    "[year]-[month]-[day]T[hour repr:24]:[minute]:[second].[subsecond digits:3]+00"
);

const RGB_RUNTIME_LOCK_FILE: &str = "rgb_runtime.lock";

pub(crate) const LOG_FILE: &str = "log";
pub(crate) const PURPOSE: u8 = 84;
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

impl From<RgbNetwork> for BitcoinNetwork {
    fn from(x: RgbNetwork) -> BitcoinNetwork {
        match x {
            RgbNetwork::Bitcoin => BitcoinNetwork::Mainnet,
            RgbNetwork::Testnet3 => BitcoinNetwork::Testnet,
            RgbNetwork::Signet => BitcoinNetwork::Signet,
            RgbNetwork::Regtest => BitcoinNetwork::Regtest,
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

impl From<BitcoinNetwork> for RgbNetwork {
    fn from(x: BitcoinNetwork) -> RgbNetwork {
        match x {
            BitcoinNetwork::Mainnet => RgbNetwork::Bitcoin,
            BitcoinNetwork::Testnet => RgbNetwork::Testnet3,
            BitcoinNetwork::Signet => RgbNetwork::Signet,
            BitcoinNetwork::Regtest => RgbNetwork::Regtest,
        }
    }
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

pub(crate) fn get_valid_txid_for_network(bitcoin_network: &BitcoinNetwork) -> String {
    match bitcoin_network {
        BitcoinNetwork::Mainnet => {
            s!("33e794d097969002ee05d336686fc03c9e15a597c1b9827669460fac98799036")
        }
        BitcoinNetwork::Testnet => {
            s!("5e6560fd518aadbed67ee4a55bdc09f19e619544f5511e9343ebba66d2f62653")
        }
        BitcoinNetwork::Signet => {
            s!("8153034f45e695453250a8fb7225a5e545144071d8ed7b0d3211efa1f3c92ad8")
        }
        BitcoinNetwork::Regtest => s!("_"),
    }
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
    Ok(format!("wpkh({key})"))
}

pub(crate) fn calculate_descriptor_from_xpub(
    xpub: ExtendedPubKey,
    keychain: u8,
) -> Result<String, Error> {
    let key = get_descriptor_pub_key(xpub, keychain)?;
    Ok(format!("wpkh({key})"))
}

fn convert_time_fmt_error(cause: time::error::Format) -> io::Error {
    io::Error::new(io::ErrorKind::Other, cause)
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
    let file = OpenOptions::new()
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

/// Wrapper for the RGB runtime and its lockfile.
pub struct RgbRuntime {
    /// The RGB runtime
    pub runtime: Runtime,
    /// The wallet directory, where the lockfile for the runtime is to be held
    pub wallet_dir: PathBuf,
}

impl RgbRuntime {
    pub(crate) fn accept_transfer<R: ResolveHeight>(
        &mut self,
        transfer: Transfer,
        resolver: &mut R,
        force: bool,
    ) -> Result<Status, InternalError>
    where
        R::Error: 'static,
    {
        self.runtime
            .accept_transfer(transfer, resolver, force)
            .map_err(InternalError::from)
    }

    pub(crate) fn blank_builder(
        &mut self,
        contract_id: ContractId,
        iface: impl Into<TypeName>,
    ) -> Result<TransitionBuilder, InternalError> {
        self.runtime
            .blank_builder(contract_id, iface)
            .map_err(InternalError::from)
    }

    pub(crate) fn chain(&self) -> RgbNetwork {
        self.runtime.chain()
    }

    pub(crate) fn consume_anchor(
        &mut self,
        anchor: Anchor<MerkleBlock>,
    ) -> Result<(), InternalError> {
        self.runtime
            .consume_anchor(anchor)
            .map_err(InternalError::from)
    }

    pub(crate) fn consume_bundle(
        &mut self,
        contract_id: ContractId,
        bundle: TransitionBundle,
        witness_txid: Txid,
    ) -> Result<(), InternalError> {
        self.runtime
            .consume_bundle(contract_id, bundle, witness_txid)
            .map_err(InternalError::from)
    }

    pub(crate) fn contract_ids(&self) -> Result<BTreeSet<ContractId>, InternalError> {
        self.runtime.contract_ids().map_err(InternalError::from)
    }

    pub(crate) fn contract_iface(
        &mut self,
        contract_id: ContractId,
        iface_id: IfaceId,
    ) -> Result<ContractIface, InternalError> {
        self.runtime
            .contract_iface(contract_id, iface_id)
            .map_err(InternalError::from)
    }

    pub(crate) fn contracts_by_outpoints(
        &mut self,
        outpoints: impl IntoIterator<Item = impl Into<Outpoint>>,
    ) -> Result<BTreeSet<ContractId>, InternalError> {
        self.runtime
            .contracts_by_outpoints(outpoints)
            .map_err(InternalError::from)
    }

    pub(crate) fn genesis(&self, contract_id: ContractId) -> Result<&Genesis, InternalError> {
        self.runtime
            .genesis(contract_id)
            .map_err(InternalError::from)
    }

    pub(crate) fn iface_by_name(&self, name: &TypeName) -> Result<&Iface, InternalError> {
        self.runtime
            .iface_by_name(name)
            .map_err(InternalError::from)
    }

    pub(crate) fn import_contract<R: ResolveHeight>(
        &mut self,
        contract: Contract,
        resolver: &mut R,
    ) -> Result<Status, InternalError>
    where
        R::Error: 'static,
    {
        self.runtime
            .import_contract(contract, resolver)
            .map_err(InternalError::from)
    }

    pub(crate) fn import_iface(
        &mut self,
        iface: impl Into<Bindle<Iface>>,
    ) -> Result<Status, InternalError> {
        self.runtime
            .import_iface(iface)
            .map_err(InternalError::from)
    }

    pub(crate) fn import_iface_impl(
        &mut self,
        iimpl: impl Into<Bindle<IfaceImpl>>,
    ) -> Result<Status, InternalError> {
        self.runtime
            .import_iface_impl(iimpl)
            .map_err(InternalError::from)
    }

    pub(crate) fn import_schema(
        &mut self,
        schema: impl Into<Bindle<SubSchema>>,
    ) -> Result<Status, InternalError> {
        self.runtime
            .import_schema(schema)
            .map_err(InternalError::from)
    }

    pub(crate) fn schema_ids(&self) -> Result<BTreeSet<SchemaId>, InternalError> {
        self.runtime.schema_ids().map_err(InternalError::from)
    }

    pub(crate) fn state_for_outpoints(
        &mut self,
        contract_id: ContractId,
        outpoints: impl IntoIterator<Item = impl Into<Outpoint>>,
    ) -> Result<BTreeMap<Opout, TypedState>, InternalError> {
        self.runtime
            .state_for_outpoints(contract_id, outpoints)
            .map_err(InternalError::from)
    }

    pub(crate) fn store_seal_secret(&mut self, seal: GraphSeal) -> Result<(), InternalError> {
        self.runtime
            .store_seal_secret(seal)
            .map_err(InternalError::from)
    }

    pub(crate) fn transfer(
        &mut self,
        contract_id: ContractId,
        seals: impl IntoIterator<Item = impl Into<BuilderSeal<GenesisSeal>>>,
    ) -> Result<Bindle<Transfer>, InternalError> {
        self.runtime
            .transfer(contract_id, seals)
            .map_err(InternalError::from)
    }

    pub(crate) fn transition_builder(
        &mut self,
        contract_id: ContractId,
        iface: impl Into<TypeName>,
        transition_name: Option<impl Into<TypeName>>,
    ) -> Result<TransitionBuilder, InternalError> {
        self.runtime
            .transition_builder(contract_id, iface, transition_name)
            .map_err(InternalError::from)
    }
}

impl Drop for RgbRuntime {
    fn drop(&mut self) {
        std::fs::remove_file(self.wallet_dir.join(RGB_RUNTIME_LOCK_FILE))
            .expect("should be able to drop lockfile")
    }
}

fn _write_rgb_runtime_lockfile(wallet_dir: &Path) {
    let lock_file_path = wallet_dir.join(RGB_RUNTIME_LOCK_FILE);
    loop {
        match OpenOptions::new()
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
pub fn load_rgb_runtime(
    wallet_dir: PathBuf,
    bitcoin_network: BitcoinNetwork,
) -> Result<RgbRuntime, Error> {
    _write_rgb_runtime_lockfile(&wallet_dir);
    let runtime = Runtime::load(wallet_dir.clone(), RgbNetwork::from(bitcoin_network))
        .map_err(InternalError::from)?;
    Ok(RgbRuntime {
        runtime,
        wallet_dir,
    })
}
