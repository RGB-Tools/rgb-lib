//! Error
//!
//! This module defines the [`Error`] enum, containing all error variants returned by functions in
//! the library.

use super::*;

/// The error variants returned by functions.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Deserialize, Serialize)]
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
    #[error("Batch transfer with idx {idx} not found")]
    BatchTransferNotFound {
        /// Transfer idx
        idx: i32,
    },

    /// The wallet has already been loaded on a different bitcoin network
    #[error("Bitcoin network mismatch")]
    BitcoinNetworkMismatch,

    /// A wallet cannot go online twice with different data
    #[error("Cannot change online object")]
    CannotChangeOnline,

    /// Requested batch transfer cannot be deleted
    #[error("Batch transfer cannot be deleted")]
    CannotDeleteBatchTransfer,

    /// Cannot estimate fees
    #[error("Cannot estimate fees")]
    CannotEstimateFees,

    /// Requested batch transfer cannot be failed
    #[error("Batch transfer cannot be set to failed status")]
    CannotFailBatchTransfer,

    /// The given PSBT cannot be finalized
    #[error("The given PSBT cannot be finalized")]
    CannotFinalizePsbt,

    /// Cannot use IFA schema on mainnet
    #[error("Cannot use IFA schema on mainnet")]
    CannotUseIfaOnMainnet,

    /// The provided file is empty
    #[error("Empty file: {file_path}")]
    EmptyFile {
        /// File path
        file_path: String,
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

    /// The master fingerprint derived from the mnemonic doesn't match the provided one
    #[error("Fingerprint mismatch")]
    FingerprintMismatch,

    /// An I/O error has been encountered
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

    /// An error was received from the indexer
    #[error("Indexer error: {details}")]
    Indexer {
        /// Error details
        details: String,
    },

    /// The provided directory does not exist
    #[error("Inexistent data directory")]
    InexistentDataDir,

    /// There are not enough available allocation slots (UTXOs with available slots)
    #[error("Insufficient allocations")]
    InsufficientAllocationSlots,

    /// There are not enough assignments of the requested asset to fulfill the request
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    #[error("Insufficient total assignments for asset: {asset_id}")]
    InsufficientAssignments {
        /// Asset ID
        asset_id: String,
        /// Available assignments
        available: AssignmentsCollection,
    },

    /// There are not enough bitcoins to fulfill the request
    #[error("Insufficient bitcoin funds: needed '{needed}', available '{available}'")]
    InsufficientBitcoins {
        /// Sats needed for some transaction
        needed: u64,
        /// Sats available for spending
        available: u64,
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

    /// An invalid 0 amount has been provided
    #[error("Amount 0 is invalid")]
    InvalidAmountZero,

    /// An invalid asset ID has been provided
    #[error("Invalid asset ID: {asset_id}")]
    InvalidAssetID {
        /// Asset ID
        asset_id: String,
    },

    /// An invalid assignment has been provided
    #[error("Invalid assignment")]
    InvalidAssignment,

    /// The provided attachments are invalid
    #[error("Invalid attachments: {details}")]
    InvalidAttachments {
        /// Error details
        details: String,
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

    /// The provided coloring info is invalid
    #[error("Invalid coloring info")]
    InvalidColoringInfo {
        /// Error details
        details: String,
    },

    /// The consignment is invalid
    #[error("Invalid consignment")]
    InvalidConsignment,

    /// The provided asset details is invalid
    #[error("Invalid details: {details}")]
    InvalidDetails {
        /// Error details
        details: String,
    },

    /// Electrum server does not provide the required functionality
    ///
    /// There are multiple electrum server variants and one with `verbose` support in
    /// `blockchain.transaction.get` is required, see this
    /// [issue](https://github.com/Blockstream/electrs/pull/36) on blockstream's electrs fork for
    /// more info
    #[error("Invalid electrum server: {details}")]
    InvalidElectrum {
        /// Error details
        details: String,
    },

    /// Trying to request fee estimation for an invalid block number, it must be between 1 and 1008
    #[error("Trying to request fee estimation for an invalid block number")]
    InvalidEstimationBlocks,

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

    /// The fingerprint is invalid
    #[error("Invalid fingerprint")]
    InvalidFingerprint,

    /// The provided indexer is invalid
    #[error("Invalid indexer: {details}")]
    InvalidIndexer {
        /// Error details
        details: String,
    },

    /// The provided invoice is invalid
    #[error("Invalid invoice: {details}")]
    InvalidInvoice {
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

    /// The provided proxy URL points to a proxy running an unsupported protocol version
    #[error("Invalid proxy protocol version: {version}")]
    InvalidProxyProtocol {
        /// Detected version
        version: String,
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

    /// The provided recipient data is invalid
    #[error("The provided recipient data is invalid: {details}")]
    InvalidRecipientData {
        /// Error details
        details: String,
    },

    /// The provided recipient ID is neither a blinded UTXO or a script
    #[error("The provided recipient ID is neither a blinded UTXO or a script")]
    InvalidRecipientID,

    /// The provided recipient ID is for a different network than the wallet's one
    #[error("The provided recipient ID is for a different network than the wallet's one")]
    InvalidRecipientNetwork,

    /// The provided asset ticker is invalid
    #[error("Invalid ticker: {details}")]
    InvalidTicker {
        /// Error details
        details: String,
    },

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

    /// The provided TXID is invalid
    #[error("Invalid TXID")]
    InvalidTxid,

    /// The provided vanilla keychain is invalid
    #[error("Invalid vanilla keychain")]
    InvalidVanillaKeychain,

    /// The maximum fee has been exceeded
    #[error("Max fee exceeded for transfer with TXID: {txid}")]
    MaxFeeExceeded {
        /// TXID of the transfer having fee issues
        txid: String,
    },

    /// The minimum fee is not met
    #[error("Min fee not met for transfer with TXID: {txid}")]
    MinFeeNotMet {
        /// TXID of the transfer having fee issues
        txid: String,
    },

    /// A network error occurred
    #[error("Network error: {details}")]
    Network {
        /// Error details
        details: String,
    },

    /// No consignment found
    #[error("No consignment found")]
    NoConsignment,

    /// Cannot issue an asset without knowing the amounts
    #[error("Issuance request with no provided amounts")]
    NoIssuanceAmounts,

    /// Cannot create a wallet with no supported schemas
    #[error("Cannot create a wallet with no supported schemas")]
    NoSupportedSchemas,

    /// No valid transport endpoint found
    #[error("No valid transport endpoint found")]
    NoValidTransportEndpoint,

    /// Trying to perform an online operation with offline wallet
    #[error("Wallet is offline. Hint: call go_online")]
    Offline,

    /// The Online object is needed to perform this operation
    #[error("The Online object is needed to perform this operation")]
    OnlineNeeded,

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

    /// The inflation amount exceeds the max possible supply
    #[error("The inflation amount exceeds the max possible supply")]
    TooHighInflationAmounts,

    /// Trying to issue too many assets
    #[error("Trying to issue too many assets")]
    TooHighIssuanceAmounts,

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

    /// The given layer 1 is not supported
    #[error("Layer 1 {layer_1} is not supported")]
    UnsupportedLayer1 {
        /// Layer 1
        layer_1: String,
    },

    /// The given schema is not supported
    #[error("Schema {asset_schema} is not supported")]
    UnsupportedSchema {
        /// Asset schema
        asset_schema: AssetSchema,
    },

    /// The given transport type is not supported
    #[error("Transport type is not supported")]
    UnsupportedTransportType,

    /// The specified wallet directory already exists
    #[error("The specified wallet directory already exists: {path}")]
    WalletDirAlreadyExists {
        /// The directory path
        path: String,
    },

    /// The requested operation cannot be processed by a watch-only wallet
    #[error("Operation not allowed on watch only wallet")]
    WatchOnly,

    /// The provided password is incorrect
    #[error("The provided password is incorrect")]
    WrongPassword,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum IndexerError {
    #[cfg(feature = "electrum")]
    #[error("Electrum error: {0}")]
    Electrum(#[from] ElectrumError),

    #[cfg(feature = "esplora")]
    #[error("Esplora error: {0}")]
    Esplora(#[from] EsploraError),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum InternalError {
    #[error("Aead error: {0}")]
    AeadError(String),

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    #[error("API error: {0}")]
    Api(#[from] reqwest::Error),

    #[error("Invalid backup path")]
    BackupInvalidPath(#[from] std::io::Error),

    #[error("Base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    #[error("Error from bdk adding UTXOs: {0}")]
    BdkAddUtxoError(#[from] bdk_wallet::tx_builder::AddUtxoError),

    #[error("Error from bdk extracting TX: {0}")]
    BdkExtractTxError(String),

    #[error("Error from bdk signing: {0}")]
    BdkSignerError(#[from] bdk_wallet::signer::SignerError),

    #[error("Confinement error: {0}")]
    Confinement(#[from] amplify::confinement::Error),

    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Encode error: {0}")]
    Encode(#[from] bitcoin::consensus::encode::Error),

    #[error("From slice error: {0}")]
    FromSlice(#[from] amplify::FromSliceError),

    #[error("Hash error: {0}")]
    HashError(#[from] scrypt::password_hash::Error),

    #[error("Infallible error: {0}")]
    Infallible(#[from] std::convert::Infallible),

    #[error("No password hash returned")]
    NoPasswordHashError,

    #[error("PSBT parse error: {0}")]
    PsbtParse(#[from] bdk_wallet::bitcoin::psbt::PsbtParseError),

    #[error("RGB load error: {0}")]
    RgbLoad(#[from] rgbstd::containers::LoadError),

    #[error("RGB PSBT error: {0}")]
    RgbPsbtError(String),

    #[error("Seal parse error: {0}")]
    SealParse(#[from] seals::txout::explicit::ParseError),

    #[error("Serde JSON error: {0}")]
    SerdeJSON(#[from] serde_json::Error),

    #[error("Stash error: {0}")]
    StashError(String),

    #[error("Stock error: {0}")]
    StockError(String),

    #[error("Strip prefix error: {0}")]
    StripPrefix(#[from] std::path::StripPrefixError),

    #[error("System time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("Unexpected error")]
    Unexpected,

    #[error("Zip error: {0}")]
    ZipError(#[from] zip::result::ZipError),
}

impl From<bdk_wallet::keys::bip39::Error> for Error {
    fn from(e: bdk_wallet::keys::bip39::Error) -> Self {
        Error::InvalidMnemonic {
            details: e.to_string(),
        }
    }
}

impl From<bdk_wallet::bitcoin::bip32::Error> for Error {
    fn from(e: bdk_wallet::bitcoin::bip32::Error) -> Self {
        Error::InvalidPubkey {
            details: e.to_string(),
        }
    }
}

impl From<bdk_wallet::bitcoin::psbt::PsbtParseError> for Error {
    fn from(e: bdk_wallet::bitcoin::psbt::PsbtParseError) -> Self {
        Error::InvalidPsbt {
            details: e.to_string(),
        }
    }
}

impl From<IndexerError> for Error {
    fn from(e: IndexerError) -> Self {
        Error::Indexer {
            details: e.to_string(),
        }
    }
}

impl From<bdk_wallet::bitcoin::psbt::ExtractTxError> for InternalError {
    fn from(e: bdk_wallet::bitcoin::psbt::ExtractTxError) -> Self {
        InternalError::BdkExtractTxError(e.to_string())
    }
}

impl From<psrgbt::RgbPsbtError> for InternalError {
    fn from(e: psrgbt::RgbPsbtError) -> Self {
        InternalError::RgbPsbtError(e.to_string())
    }
}

impl From<psrgbt::OpretKeyError> for InternalError {
    fn from(e: psrgbt::OpretKeyError) -> Self {
        InternalError::RgbPsbtError(e.to_string())
    }
}

impl From<psrgbt::MpcPsbtError> for InternalError {
    fn from(e: psrgbt::MpcPsbtError) -> Self {
        InternalError::RgbPsbtError(e.to_string())
    }
}

impl From<rgbstd::persistence::StashProviderError<std::convert::Infallible>> for InternalError {
    fn from(e: rgbstd::persistence::StashProviderError<std::convert::Infallible>) -> Self {
        InternalError::StashError(e.to_string())
    }
}

impl From<rgbstd::persistence::StockError> for InternalError {
    fn from(e: rgbstd::persistence::StockError) -> Self {
        InternalError::StockError(e.to_string())
    }
}

impl
    From<
        rgbstd::persistence::StockError<
            rgbstd::persistence::MemStash,
            rgbstd::persistence::MemState,
            rgbstd::persistence::MemIndex,
            rgbstd::persistence::ConsignError,
        >,
    > for InternalError
{
    fn from(
        e: rgbstd::persistence::StockError<
            rgbstd::persistence::MemStash,
            rgbstd::persistence::MemState,
            rgbstd::persistence::MemIndex,
            rgbstd::persistence::ConsignError,
        >,
    ) -> Self {
        InternalError::StockError(e.to_string())
    }
}

impl
    From<
        rgbstd::persistence::StockError<
            rgbstd::persistence::MemStash,
            rgbstd::persistence::MemState,
            rgbstd::persistence::MemIndex,
            rgbstd::persistence::FasciaError,
        >,
    > for InternalError
{
    fn from(
        e: rgbstd::persistence::StockError<
            rgbstd::persistence::MemStash,
            rgbstd::persistence::MemState,
            rgbstd::persistence::MemIndex,
            rgbstd::persistence::FasciaError,
        >,
    ) -> Self {
        InternalError::StockError(e.to_string())
    }
}

impl From<rgbinvoice::TransportParseError> for Error {
    fn from(e: rgbinvoice::TransportParseError) -> Self {
        Error::InvalidTransportEndpoint {
            details: e.to_string(),
        }
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Proxy {
            details: e.to_string(),
        }
    }
}

impl From<bdk_wallet::file_store::StoreErrorWithDump<ChangeSet>> for Error {
    fn from(e: bdk_wallet::file_store::StoreErrorWithDump<ChangeSet>) -> Self {
        Error::IO {
            details: e.to_string(),
        }
    }
}

impl From<bdk_wallet::FileStoreError> for Error {
    fn from(e: bdk_wallet::FileStoreError) -> Self {
        Error::IO {
            details: e.to_string(),
        }
    }
}

impl From<bdk_wallet::CreateWithPersistError<bdk_wallet::FileStoreError>> for Error {
    fn from(e: bdk_wallet::CreateWithPersistError<bdk_wallet::FileStoreError>) -> Self {
        Error::IO {
            details: e.to_string(),
        }
    }
}

impl From<bdk_wallet::LoadWithPersistError<bdk_wallet::FileStoreError>> for Error {
    fn from(e: bdk_wallet::LoadWithPersistError<bdk_wallet::FileStoreError>) -> Self {
        match e {
            bdk_wallet::LoadWithPersistError::InvalidChangeSet(
                bdk_wallet::LoadError::Mismatch(bdk_wallet::LoadMismatch::Genesis { .. }),
            ) => Error::BitcoinNetworkMismatch,
            _ => Error::IO {
                details: e.to_string(),
            },
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

impl From<rgbstd::contract::BuilderError> for Error {
    fn from(e: rgbstd::contract::BuilderError) -> Self {
        Error::Internal {
            details: e.to_string(),
        }
    }
}
