//! Error
//!
//! This module defines the [`Error`] enum, containing all error variants returned by functions in
//! the library.

/// The error variants returned by functions
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No need to create more allocations
    #[error("Allocation already available")]
    AllocationsAlreadyAvailable,

    /// Requested asset was not found
    #[error("Asset with id {0} not found")]
    AssetNotFound(String),

    /// The requested batch transfer was not found
    #[error("Batch transfer with TXID {0} not found")]
    BatchTransferNotFound(String),

    /// Provided blinded UTXO has already been used for another transfer
    #[error("Blinded UTXO already used")]
    BlindedUTXOAlreadyUsed,

    /// A wallet cannot go online twice with different data
    #[error("Cannot change online object")]
    CannotChangeOnline(),

    /// Requested transfer cannot be deleted
    #[error("Transfer cannot be deleted")]
    CannotDeleteTransfer,

    /// Requested transfer cannot be failed
    #[error("Transfer cannot be set to failed status")]
    CannotFailTransfer,

    /// Error contacting the consignment proxy
    #[error("Consignment proxy error: {0}")]
    ConsignmentProxy(#[from] reqwest::Error),

    /// An error was received from the Electrum server
    #[error("Electrum error: {0}")]
    Electrum(#[from] electrum_client::Error),

    /// Syncing BDK with the blockchain has failed
    #[error("Failed bdk sync: {0}")]
    FailedBdkSync(String),

    /// Broadcasting the PSBT has failed
    #[error("Failed broadcast: {0}")]
    FailedBroadcast(String),

    /// Issued RGB asset has failed the validity check
    #[error("Failed issuance. Register status: {0}")]
    FailedIssuance(String),

    /// An error I/O error has been encountered
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),

    /// An inconsistency has been detected between the wallet's internal (database) and external
    /// (BDK, RGB) data
    #[error("Data is inconsistent ({0}). Please check its integrity.")]
    Inconsistency(String),

    /// The provided directory does not exist
    #[error("Inexistent data directory")]
    InexistentDataDir,

    /// There are not enough available allocation slots (UTXOs with available slots)
    #[error("Insufficient allocations")]
    InsufficientAllocationSlots,

    /// There are not enough spendable tokens of the requested asset to fulfill the request
    #[error("Insufficient assets")]
    InsufficientAssets,

    /// There are not enough bitcoins to fulfill the request
    #[error("Insufficient funds")]
    InsufficientFunds,

    /// An internal error has been encountered
    #[error("Internal error: {0}")]
    Internal(#[from] InternalError),

    /// An invalid bitcoin address has been provided
    #[error("Address error: {0}")]
    InvalidAddress(#[from] bitcoin::util::address::Error),

    /// Keys derived from the provided data do not match
    #[error("Invalid bitcoin keys")]
    InvalidBitcoinKeys(),

    /// The provided blinded UTXO is invalid
    #[error("Invalid blinded UTXO: {0}")]
    InvalidBlindedUTXO(#[from] bp::seals::txout::blind::ParseError),

    /// Electrum server does not provide the required functionality
    #[error("Invalid electrum server: {0}")]
    InvalidElectrum(String),

    /// The provided mnemonic phrase is invalid
    #[error("Invalid mnemonic error: {0}")]
    InvalidMnemonic(#[from] bdk::keys::bip39::Error),

    /// The provided asset name is invalid
    #[error("Invalid name: {0}")]
    InvalidName(String),

    /// The provided online object is invalid
    #[error("Invalid online object")]
    InvalidOnline(),

    /// The provided PSBT could not be parsed
    #[error("Invalid PSBT: {0}")]
    InvalidPsbt(#[from] bitcoin::util::psbt::PsbtParseError),

    /// The provided pubkey is invalid
    #[error("Invalid pubkey: {0}")]
    InvalidPubkey(#[from] bitcoin::util::bip32::Error),

    /// The provided asset ticker is invalid
    #[error("Invalid ticker: {0}")]
    InvalidTicker(String),

    /// Cannot issue an asset without knowing the amounts
    #[error("Issuance request with no provided amounts")]
    NoIssuanceAmounts,

    /// The requested transfer was not found
    #[error("Transfer with blinded UTXO {0} not found")]
    TransferNotFound(String),

    /// The requested operation cannot be processed by a watch-only wallet
    #[error("Operation not allowed on watch only wallet")]
    WatchOnly(),
}

#[derive(Debug, thiserror::Error)]
pub enum InternalError {
    #[error("Anchor error: {0}")]
    Anchor(#[from] dbc::tapret::PsbtCommitError),

    #[error("API error: {0}")]
    Api(#[from] reqwest::Error),

    #[error("Base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    #[error("Error from bdk: {0}")]
    Bdk(#[from] bdk::Error),

    #[error("Bech32 error: {0}")]
    Bech32(#[from] lnpbp::bech32::Error),

    #[error("Cannot query rgb-node")]
    CannotQueryRgbNode,

    #[error("Confidential data error: {0}")]
    ConfidentialData(#[from] rgb_core::ConfidentialDataError),

    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Encode error: {0}")]
    Encode(#[from] bitcoin::consensus::encode::Error),

    #[error("PSBT key error: {0}")]
    PsbtKey(#[from] rgb::psbt::KeyError),

    #[error("PSBT parse error: {0}")]
    PsbtParse(#[from] bitcoin::util::psbt::PsbtParseError),

    #[error("Rgb blank error: {0}")]
    RgbBlank(#[from] rgb::blank::Error),

    #[error("RPC error from rgb: {0}")]
    RgbRpc(#[from] rgb_rpc::Error),

    #[error("Seal parse error: {0}")]
    SealParse(#[from] bp::seals::txout::explicit::ParseError),

    #[error("Serde JSON error: {0}")]
    SerdeJSON(#[from] serde_json::Error),

    #[error("Strict encode error: {0}")]
    StrictEncode(#[from] strict_encoding::Error),

    #[error("Strip prefix error: {0}")]
    StripPrefix(#[from] std::path::StripPrefixError),

    #[error("System time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("Unexpected error")]
    Unexpected,
}
