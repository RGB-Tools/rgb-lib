#![warn(missing_docs)]

//! A library to manage wallets for RGB assets.

pub(crate) mod error;
pub(crate) mod utils;
pub mod keys;

pub use crate::error::Error;
pub use crate::keys::generate_keys;
pub use crate::keys::restore_keys;
pub use crate::utils::BitcoinNetwork;
