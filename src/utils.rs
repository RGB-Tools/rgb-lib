use bdk::bitcoin::Network as BdkNetwork;

/// Supported Bitcoin networks
#[derive(Clone, Copy, PartialEq, Eq)]
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
