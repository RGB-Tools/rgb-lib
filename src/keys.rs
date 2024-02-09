//! RGB wallet keys
//!
//! This module defines the [`Keys`] structure and its related functions.

use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::Network as BdkNetwork;
use bdk::keys::bip39::{Language, Mnemonic, WordCount};
use bdk::keys::{DerivableKey, ExtendedKey, GeneratableKey};
use serde::{Deserialize, Serialize};

use crate::utils::{derive_account_xprv_from_mnemonic, get_xpub_from_xprv};
use crate::{BitcoinNetwork, Error};

/// A set of Bitcoin keys used by the wallet.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Keys {
    /// Mnemonic phrase
    pub mnemonic: String,
    /// Master xPub
    pub xpub: String,
    /// Account-level xPub
    pub account_xpub: String,
    /// Fingerprint of the account-level xPub
    pub account_xpub_fingerprint: String,
}

/// Generate a set of [`Keys`] for the given Bitcoin network.
pub fn generate_keys(bitcoin_network: BitcoinNetwork) -> Keys {
    let bdk_network = BdkNetwork::from(bitcoin_network);
    let mnemonic = Mnemonic::generate((WordCount::Words12, Language::English))
        .expect("to be able to generate a new mnemonic");
    let xkey: ExtendedKey = mnemonic
        .clone()
        .into_extended_key()
        .expect("a valid key should have been provided");
    let xpub = &xkey.into_xpub(bdk_network, &Secp256k1::new());
    let mnemonic_str = mnemonic.to_string();
    let account_xprv = derive_account_xprv_from_mnemonic(bitcoin_network, &mnemonic_str).unwrap();
    let account_xpub = get_xpub_from_xprv(&account_xprv);
    let account_xpub_fingerprint = account_xpub.fingerprint().to_string();
    Keys {
        mnemonic: mnemonic_str,
        xpub: xpub.clone().to_string(),
        account_xpub_fingerprint,
        account_xpub: account_xpub.to_string(),
    }
}

/// Recreate a set of [`Keys`] from the given mnemonic phrase.
pub fn restore_keys(bitcoin_network: BitcoinNetwork, mnemonic: String) -> Result<Keys, Error> {
    let bdk_network = BdkNetwork::from(bitcoin_network);
    let account_xprv = derive_account_xprv_from_mnemonic(bitcoin_network, &mnemonic).unwrap();
    let account_xpub = get_xpub_from_xprv(&account_xprv);
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)?;
    let xkey: ExtendedKey = mnemonic
        .clone()
        .into_extended_key()
        .expect("a valid key should have been provided");
    let xpub = &xkey.into_xpub(bdk_network, &Secp256k1::new());
    let account_xpub_fingerprint = account_xpub.fingerprint().to_string();
    Ok(Keys {
        mnemonic: mnemonic.to_string(),
        xpub: xpub.clone().to_string(),
        account_xpub_fingerprint,
        account_xpub: account_xpub.to_string(),
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use bitcoin::bip32::ExtendedPubKey;
    use std::str::FromStr;

    #[test]
    fn generate_success() {
        let Keys {
            mnemonic,
            xpub,
            account_xpub_fingerprint,
            account_xpub,
        } = generate_keys(BitcoinNetwork::Regtest);

        assert!(Mnemonic::from_str(&mnemonic).is_ok());
        let pubkey = ExtendedPubKey::from_str(&xpub);
        assert!(pubkey.is_ok());
        let account_pubkey = ExtendedPubKey::from_str(&account_xpub);
        assert!(account_pubkey.is_ok());
        assert_eq!(
            account_pubkey.unwrap().fingerprint().to_string(),
            account_xpub_fingerprint
        );
    }

    #[test]
    fn restore_success() {
        let network = BitcoinNetwork::Regtest;
        let Keys {
            mnemonic,
            xpub,
            account_xpub_fingerprint,
            account_xpub,
        } = generate_keys(network);

        let keys = restore_keys(network, mnemonic).unwrap();
        assert_eq!(keys.xpub, xpub);
        assert_eq!(keys.account_xpub_fingerprint, account_xpub_fingerprint);
        assert_eq!(keys.account_xpub, account_xpub);
    }
}
