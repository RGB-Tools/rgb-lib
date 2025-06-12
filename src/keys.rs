//! RGB wallet keys
//!
//! This module defines the [`Keys`] structure and its related functions.

use super::*;

/// A set of Bitcoin keys used by the wallet.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Keys {
    /// Mnemonic phrase
    pub mnemonic: String,
    /// Master xPub
    pub xpub: String,
    /// Account-level xPub of the vanilla-side of the wallet
    pub account_xpub_vanilla: String,
    /// Account-level xPub of the colored-side of the wallet
    pub account_xpub_colored: String,
    /// Fingerprint of the master xPub
    pub master_fingerprint: String,
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
    let (account_xpub_vanilla, account_xpub_colored) =
        get_account_xpubs(bitcoin_network, &mnemonic_str).unwrap();
    let master_fingerprint = xpub.fingerprint().to_string();
    Keys {
        mnemonic: mnemonic_str,
        xpub: xpub.clone().to_string(),
        account_xpub_vanilla: account_xpub_vanilla.to_string(),
        account_xpub_colored: account_xpub_colored.to_string(),
        master_fingerprint,
    }
}

/// Recreate a set of [`Keys`] from the given mnemonic phrase.
pub fn restore_keys(bitcoin_network: BitcoinNetwork, mnemonic: String) -> Result<Keys, Error> {
    let bdk_network = BdkNetwork::from(bitcoin_network);
    let (account_xpub_vanilla, account_xpub_colored) =
        get_account_xpubs(bitcoin_network, &mnemonic)?;
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
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn generate_success() {
        let Keys {
            mnemonic,
            xpub,
            account_xpub_vanilla,
            account_xpub_colored,
            master_fingerprint,
        } = generate_keys(BitcoinNetwork::Regtest);

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
    }

    #[test]
    fn restore_success() {
        let network = BitcoinNetwork::Regtest;
        let Keys {
            mnemonic,
            xpub,
            account_xpub_vanilla,
            account_xpub_colored,
            master_fingerprint,
        } = generate_keys(network);

        let keys = restore_keys(network, mnemonic).unwrap();
        assert_eq!(keys.xpub, xpub);
        assert_eq!(keys.master_fingerprint, master_fingerprint);
        assert_eq!(keys.account_xpub_colored, account_xpub_colored);
        assert_eq!(keys.account_xpub_vanilla, account_xpub_vanilla);
    }
}
