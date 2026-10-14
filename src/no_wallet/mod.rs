//! No wallet.
//!
//! This module defines the methods that don't require a wallet (available without the `wallet`
//! feature) and the objects they return, some of which are shared with wallet methods (e.g. the
//! inspection ones). Methods that need to reach the network (e.g. the validation ones) require an
//! indexer validation feature (`bitcoind_validation`, `electrum_validation` or
//! `esplora_validation`), which doesn't enable the `wallet` feature.

pub(crate) mod inspection;
#[cfg(feature = "validation")]
pub(crate) mod validation;

use super::*;

pub use inspection::{
    BundleFilter, ConsignmentInspection, RgbGenesisInfo, RgbInputInfo, RgbInspection,
    RgbOperationInfo, RgbOutputInfo, RgbTransitionInfo, TransitionGlobal, TransitionMetadata,
    inspect_consignment,
};
#[cfg(feature = "wallet")]
pub(crate) use inspection::{bundle_info, verify_witness_commitment};
#[cfg(feature = "_validation")]
pub use validation::{BitcoindAuth, IndexerConfig, IndexerProtocol, validate_consignment};
#[cfg(feature = "validation")]
pub use validation::{
    ConsignmentValidation, ValidatedWitness, ValidationOptions, ValidationWarning,
    WitnessValidationStatus, validate_consignment_with_resolver,
};

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

#[cfg(feature = "wallet")]
impl BitcoinNetwork {
    pub(crate) fn network_kind(&self) -> bitcoin::NetworkKind {
        match self {
            BitcoinNetwork::Mainnet => bitcoin::NetworkKind::Main,
            _ => bitcoin::NetworkKind::Test,
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

impl From<BitcoinNetwork> for bitcoin::NetworkKind {
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

/// Check the file at the given path exists.
pub(crate) fn check_file_exists(file_path: &str) -> Result<(), Error> {
    if !Path::new(file_path).exists() {
        return Err(Error::InvalidFilePath {
            file_path: file_path.to_string(),
        });
    }
    Ok(())
}
