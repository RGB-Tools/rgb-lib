#![allow(clippy::too_many_arguments)]
#![warn(missing_docs)]

//! A library to manage wallets for RGB assets.
//!
//! ## Wallet
//! The main component of the library is the [`Wallet`].
//!
//! It allows to create and operate an RGB wallet that can issue, send and receive RGB20 and RGB121
//! assets. The library also manages UTXOs and asset allocations.
//!
//! ## Backend
//! The library uses BDK for walleting operations and several components from the RGB ecosystem for
//! RGB asset operations.
//!
//! ## Database
//! A SQLite database is used to persist data to disk.
//!
//! Database support is designed in order to support multiple database backends. At the moment only
//! SQLite is supported but adding more should be relatively easy.
//!
//! ## Api
//! RGB asset transfers require the exchange of off-chain data in the form of consignment or media
//! files.
//!
//! The library currently implements the API for a proxy server to support these data exchanges
//! between sender and receiver.
//!
//! ## Errors
//! Errors are handled with the crate `thiserror`.
//!
//! ## FFI
//! Library functionality is exposed for other languages via the sub-crate `rgb-lib-ffi`.
//!
//! It uses `uniffi` and the exposed functionality is defined in the `rgb-lib-ffi/src/rgb-lib.udl`
//! file.
//!
//! ## Examples
//! ### Create an RGB wallet
//! ```
//! use rgb_lib::wallet::{DatabaseType, Wallet, WalletData};
//! use rgb_lib::{generate_keys, BitcoinNetwork};
//!
//! fn main() -> Result<(), rgb_lib::Error> {
//!     let data_dir = tempfile::tempdir()?;
//!     let keys = generate_keys(BitcoinNetwork::Regtest);
//!     let wallet_data = WalletData {
//!         data_dir: data_dir.path().to_str().unwrap().to_string(),
//!         bitcoin_network: BitcoinNetwork::Regtest,
//!         database_type: DatabaseType::Sqlite,
//!         pubkey: keys.xpub,
//!         mnemonic: Some(keys.mnemonic),
//!     };
//!     let wallet = Wallet::new(wallet_data)?;
//!
//!     Ok(())
//! }
//! ```

#[macro_use]
extern crate slog;

#[macro_use]
extern crate amplify;

pub(crate) mod api;
pub(crate) mod database;
pub(crate) mod error;
pub mod keys;
pub(crate) mod utils;
pub mod wallet;

pub use crate::database::enums::{ConsignmentEndpointProtocol, TransferStatus};
pub use crate::error::Error;
pub use crate::keys::generate_keys;
pub use crate::keys::restore_keys;
pub use crate::utils::BitcoinNetwork;
pub use crate::wallet::Wallet;
