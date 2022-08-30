//! Error
//!
//! This module defines the [`Error`] enum, containing all error variants returned by functions in
//! the library.

/// The error variants returned by functions
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The provided mnemonic phrase is invalid
    #[error("Invalid mnemonic error: {0}")]
    InvalidMnemonic(#[from] bdk::keys::bip39::Error),
}
