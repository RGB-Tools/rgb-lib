uniffi_macros::include_scaffolding!("rgb-lib");

type BitcoinNetwork = rgb_lib::BitcoinNetwork;
type Keys = rgb_lib::keys::Keys;
type RgbLibError = rgb_lib::Error;

fn generate_keys(bitcoin_network: BitcoinNetwork) -> Keys {
    rgb_lib::generate_keys(bitcoin_network)
}

fn restore_keys(bitcoin_network: BitcoinNetwork, mnemonic: String) -> Result<Keys, RgbLibError> {
    rgb_lib::restore_keys(bitcoin_network, mnemonic)
}
