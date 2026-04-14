//! Errors.
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

    /// The given PSBTs cannot be combined
    #[error("The given PSBTs cannot be combined")]
    CannotCombinePsbts,

    /// Requested pending vanilla TX cannot be aborted
    #[error("Pending vanilla TX cannot be aborted")]
    CannotAbortPendingVanillaTx,

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

    /// The provided cosigner is invalid
    #[error("Invalid cosigner: {details}")]
    InvalidCosigner {
        /// Error details
        details: String,
    },

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

    /// The provided expiration is invalid
    #[error("Invalid expiration")]
    InvalidExpiration,

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

    /// Invalid multisig threshold
    #[error("Invalid multisig threshold: required={required} with total={total}")]
    InvalidMultisigThreshold {
        /// Required threshold
        required: u8,
        /// Total cosigners
        total: u8,
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

    /// The provided PSBT is invalid
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

    /// The provided recipient map is invalid
    #[error("The provided recipient map is invalid")]
    InvalidRecipientMap,

    /// The provided recipient ID is for a different network than the wallet's one
    #[error("The provided recipient ID is for a different network than the wallet's one")]
    InvalidRecipientNetwork,

    /// The provided reject list URL is invalid
    #[error("Invalid reject list URL: {details}")]
    InvalidRejectListUrl {
        /// Error details
        details: String,
    },

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

    /// Multisig hub service error
    #[error("Multisig hub service error: {details}")]
    MultisigHubService {
        /// Error details
        details: String,
    },

    /// Cannot mark operation as processed
    #[error("Cannot mark operation as processed: {details}")]
    MultisigCannotMarkOperationProcessed {
        /// Error details
        details: String,
    },

    /// Cannot respond to operation
    #[error("Cannot respond to operation: {details}")]
    MultisigCannotRespondToOperation {
        /// Error details
        details: String,
    },

    /// Cannot initiate a new operation while another is in progress
    #[error("Cannot initiate a new operation while another is in progress")]
    MultisigOperationInProgress,

    /// The requested operation was not found
    #[error("Operation with idx {operation_idx} not found")]
    MultisigOperationNotFound {
        /// Operation idx
        operation_idx: i32,
    },

    /// Transfer status already set to a different value
    #[error("Transfer status already set to a different value")]
    MultisigTransferStatusMismatch,

    /// Received unexpected data from hub
    #[error("Unexpected hub data: {details}")]
    MultisigUnexpectedData {
        /// Error details
        details: String,
    },

    /// The user is not a cosigner
    #[error("User is not a cosigner")]
    MultisigUserNotCosigner,

    /// A network error occurred
    #[error("Network error: {details}")]
    Network {
        /// Error details
        details: String,
    },

    /// No consignment found
    #[error("No consignment found")]
    NoConsignment,

    /// No cosigners supplied
    #[error("No cosigners supplied")]
    NoCosignersSupplied,

    /// Cannot inflate an asset with unknown o zero amounts
    #[error("Inflation request with no amounts or zero amounts")]
    NoInflationAmounts,

    /// Cannot issue an asset without knowing the amounts
    #[error("Issuance request with no provided amounts")]
    NoIssuanceAmounts,

    /// No keys supplied
    #[error("No keys supplied")]
    NoKeysSupplied,

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

    /// Error during PSBT inspection
    #[error("Error during PSBT inspection: {details}")]
    PsbtInspection {
        /// Error details
        details: String,
    },

    /// Provided recipient ID has already been used for another transfer
    #[error("Recipient ID already used")]
    RecipientIDAlreadyUsed,

    /// Provided recipient map has duplicated recipient IDs
    #[error("Recipient ID duplicated")]
    RecipientIDDuplicated,

    /// Error contacting the reject list service
    #[error("Reject list service error: {details}")]
    RejectListService {
        /// Error details
        details: String,
    },

    /// Error building a rest client
    #[error("Error building a rest client")]
    RestClientBuild {
        /// Error details
        details: String,
    },

    /// Error during RGB inspection
    #[error("Error during RGB inspection: {details}")]
    RgbInspection {
        /// Error details
        details: String,
    },

    /// The inflation amount exceeds the max possible supply
    #[error("The inflation amount exceeds the max possible supply")]
    TooHighInflationAmounts,

    /// Trying to issue too many assets
    #[error("Trying to issue too many assets")]
    TooHighIssuanceAmounts,

    /// Provided too many cosigners
    #[error("Provided too many cosigners")]
    TooManyCosigners,

    /// PSBT has too many signatures
    #[error("PSBT has too many signatures")]
    TooManySignaturesInPsbt,

    /// The detected RGB schema is unknown
    #[error("Unknown RGB schema: {schema_id}")]
    UnknownRgbSchema {
        /// RGB schema ID
        schema_id: String,
    },

    /// The detected transfer is unknown
    #[error("Unknown transfer: {txid}")]
    UnknownTransfer {
        /// Transfer TXID
        txid: String,
    },

    /// The backup version is not supported
    #[error("Backup version not supported")]
    UnsupportedBackupVersion {
        /// Backup version
        version: String,
    },

    /// The schema doesn't support inflation
    #[error("Inflation not supported")]
    UnsupportedInflation {
        /// Asset schema
        asset_schema: AssetSchema,
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

    #[error("Error creating RGB commitment: {0}")]
    Commit(String),

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
    SealParse(#[from] rgbstd::txout::explicit::ParseError),

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

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::RestClientBuild {
            details: e.to_string(),
        }
    }
}

impl From<bdk_wallet::bitcoin::psbt::ExtractTxError> for InternalError {
    fn from(e: bdk_wallet::bitcoin::psbt::ExtractTxError) -> Self {
        InternalError::BdkExtractTxError(e.to_string())
    }
}

impl From<psrgbt::CommitError> for InternalError {
    fn from(e: psrgbt::CommitError) -> Self {
        InternalError::Commit(e.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_PSBT: &str = "cHNidP8BAF4CAAAAATQi69UHNVN1H3GVJyjCt2qx9Xsmt56SLSMwGM/GQxgBAQAAAAD9////AfQBAAAAAAAAIlEgoK74YaTaHlE4t4tfisItYxVkOmBakMt96x+kMAS6ArcZCgAAAAEBK+gDAAAAAAAAIlEgjWk7HP1AQKe/fp/RWQmQzRsfIQWHq+fWUteGf/YfEy0hFoJzo2OMevtseGK/uBkC+bautE/IgnmbDCz6eMojamp4GQA39GtzVgAAgB+fDIAAAACAAAAAAAAAAAABFyCCc6NjjHr7bHhiv7gZAvm2rrRPyIJ5mwws+njKI2pqeAAA";

    #[test]
    fn from_error() {
        // NotFound error
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = Error::from(io_err);
        assert_matches!(err, Error::IO { details } if details.contains("file not found"));

        // Unexpected error
        let internal = InternalError::Unexpected;
        let err = Error::from(internal);
        assert_matches!(err, Error::Internal { details } if details == "Unexpected error");

        // Bip39 error
        let bip39_err = bdk_wallet::keys::bip39::Error::BadWordCount(5);
        let err = Error::from(bip39_err);
        assert_matches!(err, Error::InvalidMnemonic { details } if !details.is_empty());

        // Bip32 error
        let bip32_err = bdk_wallet::bitcoin::bip32::Error::CannotDeriveFromHardenedKey;
        let err = Error::from(bip32_err);
        assert_matches!(err, Error::InvalidPubkey { details } if !details.is_empty());

        // BuilderError error
        let err = rgbstd::contract::BuilderError::InvalidStateField(FieldName::from("test"));
        let err = Error::from(err);
        assert_matches!(err, Error::Internal { details } if !details.is_empty());

        // LoadWithPersistError error
        let err = bdk_wallet::LoadWithPersistError::Persist(bdk_wallet::FileStoreError::Write(
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access"),
        ));
        let err = Error::from(err);
        assert_matches!(err, Error::IO { details } if !details.is_empty());

        // CreateWithPersistError error
        let err = bdk_wallet::CreateWithPersistError::Persist(bdk_wallet::FileStoreError::Write(
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access"),
        ));
        let err = Error::from(err);
        assert_matches!(err, Error::IO { details } if !details.is_empty());

        // FileStoreError error
        let err = bdk_wallet::FileStoreError::Write(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "no access",
        ));
        let err = Error::from(err);
        assert_matches!(err, Error::IO { details } if !details.is_empty());

        // StoreErrorWithDump error
        let err = bdk_wallet::file_store::StoreErrorWithDump::<ChangeSet>::from(
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access"),
        );
        let err = Error::from(err);
        assert_matches!(err, Error::IO { details } if !details.is_empty());

        // IndexerError error
        #[cfg(feature = "electrum")]
        {
            let err = IndexerError::Electrum(ElectrumError::MissingDomain);
            let err = Error::from(err);
            assert_matches!(err, Error::Indexer { details } if !details.is_empty());
        }
        #[cfg(all(feature = "esplora", not(feature = "electrum")))]
        {
            let err = IndexerError::Esplora(EsploraError::Minreq(minreq::Error::AddressNotFound));
            let err = Error::from(err);
            assert_matches!(err, Error::Indexer { details } if !details.is_empty());
        }
    }

    #[test]
    fn from_internal_error() {
        // BackupInvalidPath error
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access");
        let internal = InternalError::from(io_err);
        assert_matches!(
            internal,
            InternalError::BackupInvalidPath(ref e) if e.to_string().contains("no access")
        );

        // SerdeJSON error
        let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let msg = serde_err.to_string();
        let internal = InternalError::from(serde_err);
        assert_matches!(
            internal,
            InternalError::SerdeJSON(ref e) if e.to_string() == msg
        );

        // StripPrefix error
        let strip_err = Path::new("foo").strip_prefix("bar").unwrap_err();
        let msg = strip_err.to_string();
        let internal = InternalError::from(strip_err);
        assert_matches!(
            internal,
            InternalError::StripPrefix(ref e) if e.to_string() == msg
        );

        // ExtractTxError error
        let psbt = Psbt::from_str(FAKE_PSBT).unwrap();
        let err = bdk_wallet::bitcoin::psbt::ExtractTxError::SendingTooMuch { psbt };
        let err = InternalError::from(err);
        assert_matches!(err, InternalError::BdkExtractTxError(ref e) if !e.is_empty());

        // RgbPsbtError error
        let err = psrgbt::RgbPsbtError::NoContracts;
        let err = InternalError::from(err);
        assert_matches!(err, InternalError::RgbPsbtError(ref e) if !e.is_empty());

        // OpretKeyError error
        let err = psrgbt::OpretKeyError::NoCommitment;
        let err = InternalError::from(err);
        assert_matches!(err, InternalError::RgbPsbtError(ref e) if !e.is_empty());

        // MpcPsbtError error
        let err = psrgbt::MpcPsbtError::KeyAlreadyPresent;
        let err = InternalError::from(err);
        assert_matches!(err, InternalError::RgbPsbtError(ref e) if !e.is_empty());

        // CommitError error
        let err = psrgbt::CommitError::Rgb(psrgbt::RgbPsbtError::NoContracts);
        let err = InternalError::from(err);
        assert_matches!(err, InternalError::Commit(ref e) if !e.is_empty());

        // StockError errors
        let err: rgbstd::persistence::StockError<
            rgbstd::persistence::MemStash,
            rgbstd::persistence::MemState,
            rgbstd::persistence::MemIndex,
            rgbstd::persistence::FasciaError,
        > = rgbstd::persistence::StockError::AbsentValidWitness;
        let err = InternalError::from(err);
        assert_matches!(err, InternalError::StockError(ref e) if !e.is_empty());
        let err: rgbstd::persistence::StockError<
            rgbstd::persistence::MemStash,
            rgbstd::persistence::MemState,
            rgbstd::persistence::MemIndex,
            rgbstd::persistence::ConsignError,
        > = rgbstd::persistence::StockError::AbsentValidWitness;
        let err = InternalError::from(err);
        assert_matches!(err, InternalError::StockError(ref e) if !e.is_empty());
        let err: rgbstd::persistence::StockError<
            rgbstd::persistence::MemStash,
            rgbstd::persistence::MemState,
            rgbstd::persistence::MemIndex,
        > = rgbstd::persistence::StockError::Resolver(s!("test"));
        let err = InternalError::from(err);
        assert_matches!(err, InternalError::StockError(ref e) if !e.is_empty());
    }
}
