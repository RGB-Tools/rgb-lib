//! RGB wallet
//!
//! This module defines the [`Wallet`] related modules.

pub(crate) mod backup;
pub(crate) mod offline;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) mod online;

#[cfg(test)]
mod test;

pub use offline::*;
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub use online::*;
