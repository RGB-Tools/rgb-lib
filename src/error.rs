//! Error
//!
//! This module defines the [`Error`] enum, containing all error variants returned by functions in
//! the library.

/// The error variants returned by functions
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No need to create more allocations
    #[error("Allocations already available")]
    AllocationsAlreadyAvailable,

    /// Requested asset was not found
    #[error("Asset with id {asset_id} not found")]
    AssetNotFound {
        /// Asset ID
        asset_id: String,
    },

    /// The requested batch transfer was not found
    #[error("Batch transfer with TXID {txid} not found")]
    BatchTransferNotFound {
        /// Transaction ID
        txid: String,
    },

    /// A wallet cannot go online twice with different data
    #[error("Cannot change online object")]
    CannotChangeOnline,

    /// Requested transfer cannot be deleted
    #[error("Transfer cannot be deleted")]
    CannotDeleteTransfer,

    /// Requested transfer cannot be failed
    #[error("Transfer cannot be set to failed status")]
    CannotFailTransfer,

    /// An error was received from the Electrum server
    #[error("Electrum error: {details}")]
    Electrum {
        /// Error details
        details: String,
    },

    /// Syncing BDK with the blockchain has failed
    #[error("Failed bdk sync: {details}")]
    FailedBdkSync {
        /// Error details
        details: String,
    },

    /// Broadcasting the PSBT has failed
    #[error("Failed broadcast: {details}")]
    FailedBroadcast {
        /// Error details
        details: String,
    },

    /// Issued RGB asset has failed the validity check
    #[error("Failed issuance. Register status: {details}")]
    FailedIssuance {
        /// Error details
        details: String,
    },

    /// The file already exists
    #[error("The file already exists: {path}")]
    FileAlreadyExists {
        /// The file path
        path: String,
    },

    /// An error I/O error has been encountered
    #[error("I/O error: {details}")]
    IO {
        /// Error details
        details: String,
    },

    /// An inconsistency has been detected between the wallet's internal (database) and external
    /// (BDK, RGB) data
    #[error("Data is inconsistent ({details}). Please check its integrity.")]
    Inconsistency {
        /// Error details
        details: String,
    },

    /// The provided directory does not exist
    #[error("Inexistent data directory")]
    InexistentDataDir,

    /// There are not enough available allocation slots (UTXOs with available slots)
    #[error("Insufficient allocations")]
    InsufficientAllocationSlots,

    /// There are not enough bitcoins to fulfill the request
    #[error("Insufficient bitcoin funds: needed '{needed}', available '{available}'")]
    InsufficientBitcoins {
        /// Sats needed for some transaction
        needed: u64,
        /// Sats available for spending
        available: u64,
    },

    /// There are not enough spendable tokens of the requested asset to fulfill the request
    #[error("Insufficient spendable funds for asset: {asset_id}")]
    InsufficientSpendableAssets {
        /// Asset ID
        asset_id: String,
    },

    /// There are not enough total tokens of the requested asset to fulfill the request
    #[error("Insufficient total funds for asset: {asset_id}")]
    InsufficientTotalAssets {
        /// Asset ID
        asset_id: String,
    },

    /// An internal error has been encountered
    #[error("Internal error: {details}")]
    Internal {
        /// Error details
        details: String,
    },

    /// An invalid bitcoin address has been provided
    #[error("Address error: {details}")]
    InvalidAddress {
        /// Error details
        details: String,
    },

    /// An invalid asset ID has been provided
    #[error("Invalid asset ID: {asset_id}")]
    InvalidAssetID {
        /// Asset ID
        asset_id: String,
    },

    /// Keys derived from the provided data do not match
    #[error("Invalid bitcoin keys")]
    InvalidBitcoinKeys,

    /// Invalid bitcoin network
    #[error("Invalid bitcoin network: {network}")]
    InvalidBitcoinNetwork {
        /// The invalid network
        network: String,
    },

    /// The provided blinded UTXO is invalid
    #[error("Invalid blinded UTXO: {details}")]
    InvalidBlindedUTXO {
        /// Error details
        details: String,
    },

    /// The provided recipient ID is neither a blinded UTXO or a script
    #[error("The provided recipient ID is neither a blinded UTXO or a script")]
    InvalidRecipientID,

    /// The provided transport endpoint is invalid
    #[error("Invalid transport endpoint: {details}")]
    InvalidTransportEndpoint {
        /// Error details
        details: String,
    },

    /// The provided transport endpoints are invalid
    #[error("Invalid transport endpoints: {details}")]
    InvalidTransportEndpoints {
        /// Error details
        details: String,
    },

    /// The provided asset description is invalid
    #[error("Invalid description: {details}")]
    InvalidDescription {
        /// Error details
        details: String,
    },

    /// Electrum server does not provide the required functionality
    #[error("Invalid electrum server: {details}")]
    InvalidElectrum {
        /// Error details
        details: String,
    },

    /// The provided fee rate is invalid
    #[error("Invalid fee rate: {details}")]
    InvalidFeeRate {
        /// Error details
        details: String,
    },

    /// The provided file path is invalid
    #[error("Invalid file path: {file_path}")]
    InvalidFilePath {
        /// File path
        file_path: String,
    },

    /// The provided invoice is invalid
    #[error("Invalid invoice: {details}")]
    InvalidInvoice {
        /// Error details
        details: String,
    },

    /// The provided invoice data is invalid
    #[error("Invalid invoice data: {details}")]
    InvalidInvoiceData {
        /// Error details
        details: String,
    },

    /// The provided mnemonic phrase is invalid
    #[error("Invalid mnemonic error: {details}")]
    InvalidMnemonic {
        /// Error details
        details: String,
    },

    /// The provided asset name is invalid
    #[error("Invalid name: {details}")]
    InvalidName {
        /// Error details
        details: String,
    },

    /// The provided asset precision is invalid
    #[error("Invalid precision: {details}")]
    InvalidPrecision {
        /// Error details
        details: String,
    },

    /// The provided PSBT could not be parsed
    #[error("Invalid PSBT: {details}")]
    InvalidPsbt {
        /// Error details
        details: String,
    },

    /// The provided pubkey is invalid
    #[error("Invalid pubkey: {details}")]
    InvalidPubkey {
        /// Error details
        details: String,
    },

    /// The provided script is invalid
    #[error("Invalid script: {details}")]
    InvalidScript {
        /// Error details
        details: String,
    },

    /// The provided asset ticker is invalid
    #[error("Invalid ticker: {details}")]
    InvalidTicker {
        /// Error details
        details: String,
    },

    /// The provided vanilla keychain is invalid
    #[error("Invalid vanilla keychain")]
    InvalidVanillaKeychain,

    /// Cannot issue an asset without knowing the amounts
    #[error("Issuance request with no provided amounts")]
    NoIssuanceAmounts,

    /// No valid transport endpoint found
    #[error("No valid transport endpoint found")]
    NoValidTransportEndpoint,

    /// Trying to perform an online operation with offline wallet
    #[error("Wallet is offline. Hint: call go_online")]
    Offline,

    /// Output created is under the dust limit
    #[error("Output below the dust limit")]
    OutputBelowDustLimit,

    /// Error contacting the RGB proxy
    #[error("Proxy error: {details}")]
    Proxy {
        /// Error details
        details: String,
    },

    /// Provided recipient ID has already been used for another transfer
    #[error("Recipient ID already used")]
    RecipientIDAlreadyUsed,

    /// Provided recipient map has duplicated recipient IDs
    #[error("Recipient ID duplicated")]
    RecipientIDDuplicated,

    /// Trying to issue too many assets
    #[error("Trying to issue too many assets")]
    TooHighIssuanceAmounts,

    /// The requested transfer was not found
    #[error("Transfer with recipient ID {recipient_id} not found")]
    TransferNotFound {
        /// Recipient ID
        recipient_id: String,
    },

    /// The detected RGB interface is unknown
    #[error("Unknown RGB interface: {interface}")]
    UnknownRgbInterface {
        /// RGB interface
        interface: String,
    },

    /// The detected RGB schema is unknown
    #[error("Unknown RGB schema: {schema_id}")]
    UnknownRgbSchema {
        /// RGB schema ID
        schema_id: String,
    },

    /// The backup version is not supported
    #[error("Backup version not supported")]
    UnsupportedBackupVersion {
        /// Backup version
        version: String,
    },

    /// The given transport type is not supported
    #[error("Transport type is not supported")]
    UnsupportedTransportType,

    /// The given invoice type is not supported
    #[error("Invoice type is not supported")]
    UnsupportedInvoice,

    /// The requested operation cannot be processed by a watch-only wallet
    #[error("Operation not allowed on watch only wallet")]
    WatchOnly,

    /// The provided password is incorrect
    #[error("The provided password is incorrect")]
    WrongPassword,
}

#[derive(Debug, thiserror::Error)]
pub enum InternalError {
    #[error("Aead error: {0}")]
    AeadError(String),

    #[error("API error: {0}")]
    Api(#[from] reqwest::Error),

    #[error("Invalid backup path")]
    BackupInvalidPath(#[from] std::io::Error),

    #[error("Base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    #[error("Error from bdk: {0}")]
    Bdk(#[from] bdk::Error),

    #[error("Cannot query rgb-node")]
    CannotQueryRgbNode,

    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Encode error: {0}")]
    Encode(#[from] bitcoin::consensus::encode::Error),

    #[error("Hash error: {0}")]
    HashError(#[from] scrypt::password_hash::Error),

    #[error("Infallible error: {0}")]
    Infallible(#[from] std::convert::Infallible),

    #[error("No password hash returned")]
    NoPasswordHashError,

    #[error("PSBT parse error: {0}")]
    PsbtParse(#[from] bdk::bitcoin::psbt::PsbtParseError),

    #[error("Restore directory is not empty")]
    RestoreDirNotEmpty,

    #[error("RGB builder error: {0}")]
    RgbBuilder(#[from] rgbstd::interface::BuilderError),

    #[error("RGB consigner error: {0}")]
    RgbConsigner(String),

    #[error("RGB DBC PSBT error: {0}")]
    RgbDbcPsbtError(#[from] rgbwallet::psbt::DbcPsbtError),

    #[error("RGB inventory error: {0}")]
    RgbInventory(String),

    #[error("RGB inventory data error: {0}")]
    RgbInventoryData(String),

    #[error("RGB load error: {0}")]
    RgbLoad(#[from] rgbstd::containers::LoadError),

    #[error("RGB PSBT error: {0}")]
    RgbPsbtError(String),

    #[error("RGB runtime error: {0}")]
    RgbRuntime(#[from] rgb::RuntimeError),

    #[error("RGB stash error: {0}")]
    RgbStash(#[from] rgbstd::persistence::StashError<std::convert::Infallible>),

    #[error("Seal parse error: {0}")]
    SealParse(#[from] bp::seals::txout::explicit::ParseError),

    #[error("Serde JSON error: {0}")]
    SerdeJSON(#[from] serde_json::Error),

    #[error("Strip prefix error: {0}")]
    StripPrefix(#[from] std::path::StripPrefixError),

    #[error("System time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("Unexpected error")]
    Unexpected,

    #[error("Zip error: {0}")]
    ZipError(#[from] zip::result::ZipError),
}

impl From<bdk::keys::bip39::Error> for Error {
    fn from(e: bdk::keys::bip39::Error) -> Self {
        Error::InvalidMnemonic {
            details: e.to_string(),
        }
    }
}

impl From<bdk::bitcoin::address::Error> for Error {
    fn from(e: bdk::bitcoin::address::Error) -> Self {
        Error::InvalidAddress {
            details: e.to_string(),
        }
    }
}

impl From<bdk::bitcoin::bip32::Error> for Error {
    fn from(e: bdk::bitcoin::bip32::Error) -> Self {
        Error::InvalidPubkey {
            details: e.to_string(),
        }
    }
}

impl From<bdk::bitcoin::psbt::PsbtParseError> for Error {
    fn from(e: bdk::bitcoin::psbt::PsbtParseError) -> Self {
        Error::InvalidPsbt {
            details: e.to_string(),
        }
    }
}

impl From<electrum_client::Error> for Error {
    fn from(e: electrum_client::Error) -> Self {
        Error::Electrum {
            details: e.to_string(),
        }
    }
}

impl From<rgbstd::persistence::ConsignerError<std::convert::Infallible, std::convert::Infallible>>
    for InternalError
{
    fn from(
        e: rgbstd::persistence::ConsignerError<std::convert::Infallible, std::convert::Infallible>,
    ) -> Self {
        InternalError::RgbConsigner(e.to_string())
    }
}

impl From<rgbstd::persistence::InventoryDataError<std::convert::Infallible>> for InternalError {
    fn from(e: rgbstd::persistence::InventoryDataError<std::convert::Infallible>) -> Self {
        InternalError::RgbInventoryData(e.to_string())
    }
}

impl From<rgbstd::persistence::InventoryError<std::convert::Infallible>> for InternalError {
    fn from(e: rgbstd::persistence::InventoryError<std::convert::Infallible>) -> Self {
        InternalError::RgbInventory(e.to_string())
    }
}

impl From<rgbwallet::psbt::RgbPsbtError> for InternalError {
    fn from(e: rgbwallet::psbt::RgbPsbtError) -> Self {
        InternalError::RgbPsbtError(e.to_string())
    }
}

impl From<rgbwallet::TransportParseError> for Error {
    fn from(e: rgbwallet::TransportParseError) -> Self {
        Error::InvalidTransportEndpoint {
            details: e.to_string(),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Proxy {
            details: e.to_string(),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IO {
            details: e.to_string(),
        }
    }
}

impl From<InternalError> for Error {
    fn from(e: InternalError) -> Self {
        Error::Internal {
            details: e.to_string(),
        }
    }
}
