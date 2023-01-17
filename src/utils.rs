use amplify::s;
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::util::bip32::ExtendedPrivKey;
use bdk::bitcoin::Network as BdkNetwork;
use bdk::descriptor::Segwitv0;
use bdk::keys::DescriptorKey::Public;
use bdk::keys::{DerivableKey, DescriptorKey};
use bitcoin::util::bip32::{DerivationPath, ExtendedPubKey, KeySource};
use lnpbp::chain::Chain as RgbNetwork;
use std::io;
use std::str::FromStr;
use std::{fs::OpenOptions, path::PathBuf};

use slog::{Drain, Logger};
use slog_term::{FullFormat, PlainDecorator};
use time::OffsetDateTime;

use crate::error::InternalError;
use crate::Error;

const DERIVATION_PATH_ACCOUNT: u32 = 827166;

const TIMESTAMP_FORMAT: &[time::format_description::FormatItem] = time::macros::format_description!(
    "[year]-[month]-[day]T[hour repr:24]:[minute]:[second].[subsecond digits:3]+00"
);

const LOG_FILE: &str = "log";

/// Supported Bitcoin networks
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

impl From<BdkNetwork> for BitcoinNetwork {
    fn from(x: BdkNetwork) -> BitcoinNetwork {
        match x {
            BdkNetwork::Bitcoin => BitcoinNetwork::Mainnet,
            BdkNetwork::Testnet => BitcoinNetwork::Testnet,
            BdkNetwork::Signet => BitcoinNetwork::Signet,
            BdkNetwork::Regtest => BitcoinNetwork::Regtest,
        }
    }
}

impl From<BitcoinNetwork> for BdkNetwork {
    fn from(x: BitcoinNetwork) -> BdkNetwork {
        match x {
            BitcoinNetwork::Mainnet => BdkNetwork::Bitcoin,
            BitcoinNetwork::Testnet => BdkNetwork::Testnet,
            BitcoinNetwork::Signet => BdkNetwork::Signet,
            BitcoinNetwork::Regtest => BdkNetwork::Regtest,
        }
    }
}

impl From<BitcoinNetwork> for RgbNetwork {
    fn from(x: BitcoinNetwork) -> RgbNetwork {
        match x {
            BitcoinNetwork::Mainnet => RgbNetwork::Mainnet,
            BitcoinNetwork::Testnet => RgbNetwork::Testnet3,
            BitcoinNetwork::Signet => RgbNetwork::Signet,
            BitcoinNetwork::Regtest => {
                RgbNetwork::from_str("regtest").expect("regtest should be a valid RGB network")
            }
        }
    }
}

pub(crate) fn get_txid(bitcoin_network: BitcoinNetwork) -> String {
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

pub(crate) fn _get_derivation_path(
    watch_only: bool,
    bitcoin_network: BitcoinNetwork,
    change: bool,
) -> String {
    let change_num = u8::from(change);
    let coin_type = i32::from(bitcoin_network != BitcoinNetwork::Mainnet);
    let hardened = if watch_only { "" } else { "'" };
    let child_number = if watch_only { "" } else { "/*" };
    let master = if watch_only { "m" } else { "" };
    format!("{master}/84{hardened}/{coin_type}{hardened}/{DERIVATION_PATH_ACCOUNT}{hardened}/{change_num}{child_number}")
}

pub(crate) fn calculate_descriptor_from_xprv(
    xprv: ExtendedPrivKey,
    bitcoin_network: BitcoinNetwork,
    change: bool,
) -> String {
    let derivation_path = _get_derivation_path(false, bitcoin_network, change);
    format!("wpkh({xprv}{derivation_path})")
}

pub(crate) fn calculate_descriptor_from_xpub(
    xpub: ExtendedPubKey,
    bitcoin_network: BitcoinNetwork,
    change: bool,
) -> Result<String, Error> {
    let derivation_path = _get_derivation_path(true, bitcoin_network, change);
    let path =
        DerivationPath::from_str(&derivation_path).expect("derivation path should be well-formed");
    let der_xpub = &xpub
        .derive_pub(&Secp256k1::new(), &path)
        .expect("provided path should be derivable in an xpub");
    let origin_pub: KeySource = (xpub.fingerprint(), path);
    let der_xpub_desc_key: DescriptorKey<Segwitv0> = der_xpub
        .into_descriptor_key(Some(origin_pub), DerivationPath::default())
        .expect("should be able to convert xpub in a descriptor key");
    if let Public(key, _, _) = der_xpub_desc_key {
        Ok(format!("wpkh({})", key))
    } else {
        Err(InternalError::Unexpected)?
    }
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

pub(crate) fn setup_logger(log_path: PathBuf) -> Result<Logger, Error> {
    let log_filepath = log_path.join(LOG_FILE);
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
