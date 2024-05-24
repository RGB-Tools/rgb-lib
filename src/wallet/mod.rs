//! RGB wallet
//!
//! This module defines the [`Wallet`] related modules.

pub(crate) mod backup;
pub(crate) mod offline;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) mod online;
pub mod rust_only;

#[cfg(test)]
pub(crate) mod test;

pub use offline::*;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub use online::*;

use super::*;
