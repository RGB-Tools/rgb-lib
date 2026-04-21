//! Wallet keys.
//!
//! This module defines the key-related structures and functions.

use super::*;

/// Supported bitcoin witness versions.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum WitnessVersion {
    /// Segregated Witness version 0
    SegWitV0,
    /// Segregated Witness version 1
    #[default]
    Taproot,
}

impl WitnessVersion {
    pub(crate) fn purpose(&self) -> u32 {
        match self {
            WitnessVersion::SegWitV0 => 84,
            WitnessVersion::Taproot => 86,
        }
    }

    pub(crate) fn descriptor_fn(&self) -> &'static str {
        match self {
            WitnessVersion::SegWitV0 => "wpkh",
            WitnessVersion::Taproot => "tr",
        }
    }
}

impl fmt::Display for WitnessVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl FromStr for WitnessVersion {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "segwitv0" => WitnessVersion::SegWitV0,
            "taproot" => WitnessVersion::Taproot,
            _ => {
                return Err(Error::InvalidWitnessVersion {
                    witness_version: s.to_string(),
                });
            }
        })
    }
}

/// A set of Bitcoin keys used by the wallet.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Keys {
    /// Mnemonic phrase
    pub mnemonic: String,
    /// Master xPub
    pub xpub: String,
    /// Account-level xPub of the vanilla side of the wallet
    pub account_xpub_vanilla: String,
    /// Account-level xPub of the colored side of the wallet
    pub account_xpub_colored: String,
    /// Fingerprint of the master xPub
    pub master_fingerprint: String,
    /// Witness version these keys were derived with
    #[serde(default)]
    pub witness_version: WitnessVersion,
}

/// Generate a set of [`Keys`] for the given Bitcoin network and witness version.
pub fn generate_keys(bitcoin_network: BitcoinNetwork, witness_version: WitnessVersion) -> Keys {
    let bdk_network = BdkNetwork::from(bitcoin_network);
    let mnemonic = Mnemonic::generate((WordCount::Words12, Language::English))
        .expect("to be able to generate a new mnemonic");
    let xkey: ExtendedKey = mnemonic
        .clone()
        .into_extended_key()
        .expect("a valid key should have been provided");
    let xpub = &xkey.into_xpub(bdk_network, &Secp256k1::new());
    let mnemonic_str = mnemonic.to_string();
    let (account_xpub_vanilla, account_xpub_colored) =
        get_account_xpubs(&bitcoin_network, &mnemonic_str, witness_version).unwrap();
    let master_fingerprint = xpub.fingerprint().to_string();
    Keys {
        mnemonic: mnemonic_str,
        xpub: xpub.clone().to_string(),
        account_xpub_vanilla: account_xpub_vanilla.to_string(),
        account_xpub_colored: account_xpub_colored.to_string(),
        master_fingerprint,
        witness_version,
    }
}

/// Recreate a set of [`Keys`] from the given mnemonic phrase for the given witness version.
pub fn restore_keys(
    bitcoin_network: BitcoinNetwork,
    mnemonic: String,
    witness_version: WitnessVersion,
) -> Result<Keys, Error> {
    let bdk_network = BdkNetwork::from(bitcoin_network);
    let (account_xpub_vanilla, account_xpub_colored) =
        get_account_xpubs(&bitcoin_network, &mnemonic, witness_version)?;
    let mnemonic_parsed = Mnemonic::parse_in(Language::English, &mnemonic)?;
    let xkey: ExtendedKey = mnemonic_parsed
        .clone()
        .into_extended_key()
        .expect("a valid key should have been provided");
    let xpub = &xkey.into_xpub(bdk_network, &Secp256k1::new());
    let master_fingerprint = xpub.fingerprint().to_string();
    Ok(Keys {
        mnemonic,
        xpub: xpub.clone().to_string(),
        account_xpub_vanilla: account_xpub_vanilla.to_string(),
        account_xpub_colored: account_xpub_colored.to_string(),
        master_fingerprint,
        witness_version,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn witness_version_display_and_parse() {
        for wv in [WitnessVersion::SegWitV0, WitnessVersion::Taproot] {
            let s = wv.to_string();
            assert_eq!(WitnessVersion::from_str(&s).unwrap(), wv);
            assert_eq!(WitnessVersion::from_str(&s.to_lowercase()).unwrap(), wv);
        }

        let err = WitnessVersion::from_str("nonsense").unwrap_err();
        assert_eq!(
            err,
            Error::InvalidWitnessVersion {
                witness_version: "nonsense".to_string(),
            },
        );
    }

    #[test]
    fn generate_success() {
        let Keys {
            mnemonic,
            xpub,
            account_xpub_vanilla,
            account_xpub_colored,
            master_fingerprint,
            witness_version,
        } = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);

        assert!(Mnemonic::from_str(&mnemonic).is_ok());
        let pubkey = Xpub::from_str(&xpub);
        assert!(pubkey.is_ok());
        assert_eq!(
            pubkey.unwrap().fingerprint().to_string(),
            master_fingerprint
        );
        let account_pubkey_rgb = Xpub::from_str(&account_xpub_colored);
        assert!(account_pubkey_rgb.is_ok());
        let account_pubkey_btc = Xpub::from_str(&account_xpub_vanilla);
        assert!(account_pubkey_btc.is_ok());
        assert_eq!(witness_version, WitnessVersion::Taproot);
    }

    #[test]
    fn restore_success() {
        let network = BitcoinNetwork::Regtest;

        // round-trip generate → restore for each supported witness version
        for wv in [WitnessVersion::Taproot, WitnessVersion::SegWitV0] {
            let Keys {
                mnemonic,
                xpub,
                account_xpub_vanilla,
                account_xpub_colored,
                master_fingerprint,
                witness_version,
            } = generate_keys(network, wv);

            let keys = restore_keys(network, mnemonic, witness_version).unwrap();
            assert_eq!(keys.xpub, xpub);
            assert_eq!(keys.master_fingerprint, master_fingerprint);
            assert_eq!(keys.account_xpub_colored, account_xpub_colored);
            assert_eq!(keys.account_xpub_vanilla, account_xpub_vanilla);
            assert_eq!(keys.witness_version, witness_version);
            assert_eq!(witness_version, wv);
        }

        // same mnemonic + different witness versions ⇒ same master xpub
        // and fingerprint but different account xpubs (different BIP purpose)
        let tr = generate_keys(network, WitnessVersion::Taproot);
        let wpkh = restore_keys(network, tr.mnemonic.clone(), WitnessVersion::SegWitV0).unwrap();
        assert_eq!(tr.xpub, wpkh.xpub);
        assert_eq!(tr.master_fingerprint, wpkh.master_fingerprint);
        assert_ne!(tr.account_xpub_colored, wpkh.account_xpub_colored);
        assert_ne!(tr.account_xpub_vanilla, wpkh.account_xpub_vanilla);
    }
}
