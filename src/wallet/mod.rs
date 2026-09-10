//! Wallet functionality.
//!
//! This module defines the [`Wallet`] and [`MultisigWallet`] structures and related functionality.

pub(crate) mod backup;
pub(crate) mod core;
#[cfg(feature = "_online")]
pub(crate) mod indexer;
pub(crate) mod multisig;
pub(crate) mod objects;
pub(crate) mod offline;
#[cfg(feature = "_online")]
pub(crate) mod online;
pub mod rust_only;
pub(crate) mod singlesig;

#[cfg(test)]
pub(crate) mod test;

pub use backup::restore_backup;
pub use multisig::{Cosigner, MultisigKeys, MultisigWallet};
#[cfg(feature = "_online")]
pub use multisig::{
    HubInfo, InitOperationResult, MultisigOnlineOptions, MultisigVotingStatus, Operation,
    OperationInfo, RespondToOperation, UserRole,
};
pub use objects::{
    Address, AssetCFA, AssetFilter, AssetIFA, AssetNIA, AssetUDA, Assets, AssignmentsCollection,
    Balance, BlockTime, BtcBalance, DatabaseType, EmbeddedMedia, Invoice, InvoiceData, Media,
    Metadata, Online, Outpoint, PendingVanillaTx, ProofOfReserves, PsbtInputInfo, PsbtInspection,
    PsbtOutputInfo, ReceiveData, Recipient, RecipientInfo, RecipientType, RgbAllocation,
    RgbInputInfo, RgbInspection, RgbOperationInfo, RgbOutputInfo, RgbTransitionInfo, Token,
    TokenLight, Transaction, TransactionType, Transfer, TransferKind, TransferTransportEndpoint,
    TransportEndpoint, TypeOfTransition, Unspent, Utxo, WalletData, WalletDescriptors, WitnessData,
};
#[cfg(feature = "_online")]
pub use objects::{
    BitcoindAuth, BurnBeginResult, BurnDetails, IndexerConfig, IndexerProtocol, InflateBeginResult,
    InflateDetails, OnlineOptions, OperationResult, RefreshFilter, RefreshResult,
    RefreshTransferStatus, RefreshedTransfer, SendBeginResult, SendDetails,
};
pub use offline::RgbWalletOpsOffline;
#[cfg(feature = "_online")]
pub use online::{RgbWalletOpsOnline, check_indexer_url};
pub use singlesig::{SinglesigKeys, Wallet};

pub(crate) use backup::WalletBackup;
pub(crate) use core::{
    ASSETS_DIR, MEDIA_DIR, NUM_KNOWN_SCHEMAS, WalletCore, WalletInternals, WalletManifest,
    setup_bdk, setup_db, setup_new_wallet, setup_rgb,
};
#[cfg(feature = "_online")]
pub use core::{SyncKeychain, SyncOptions, SyncStrategy};
#[cfg(feature = "bitcoind")]
pub(crate) use indexer::BitcoindClient;
#[cfg(feature = "_online")]
pub(crate) use indexer::Indexer;
#[cfg(feature = "_online")]
pub(crate) use objects::{
    AssetInfo, AssetSpend, BeginOperationData, BtcChange, FailTransfersOutcome, LocalRecipient,
    LocalRecipientData, LocalWitnessData, OnlineData, PrepareRgbPsbtResult,
    PrepareTransferPsbtResult, ReceiveMatcher, ReceiveMode, ReceivedConsignmentMeta,
    RefreshResultTrait, TryFailBatchTransferOutcome,
};
pub(crate) use objects::{
    InfoAssetTransfer, InfoBatchTransfer, IssueData, IssuedAssetDetails, LocalAssetData,
    LocalRgbAllocation, LocalTransportEndpoint, LocalUnspent, ReceiveDataInternal, TransferData,
    TransferEndData,
};
#[cfg(feature = "_online")]
pub(crate) use offline::TRANSFER_DATA_FILE;
#[cfg(feature = "rgb_fs_store_migration")]
pub(crate) use offline::TRANSFERS_DIR;
pub(crate) use offline::WalletOffline;
#[cfg(feature = "_online")]
pub(crate) use online::WalletOnline;

use super::*;

pub(crate) const CONSIGNMENT_FILE: &str = "consignment_out";
pub(crate) const FASCIA_FILE: &str = "fascia";
pub(crate) const UNSIGNED_PSBT_FILE: &str = "unsigned.psbt";

pub(crate) const SCHEMA_ID_NIA: &str =
    "rgb:sch:RWhwUfTMpuP2Zfx1~j4nswCANGeJrYOqDcKelaMV4zU#remote-digital-pegasus";
pub(crate) const SCHEMA_ID_UDA: &str =
    "rgb:sch:~6rjymf3GTE840lb5JoXm2aFwE8eWCk3mCjOf_mUztE#spider-montana-fantasy";
pub(crate) const SCHEMA_ID_CFA: &str =
    "rgb:sch:JgqK5hJX9YBT4osCV7VcW_iLTcA5csUCnLzvaKTTrNY#mars-house-friend";
pub(crate) const SCHEMA_ID_IFA: &str =
    "rgb:sch:Y2a6SW4rY3gRwLE40OxILhwOyL2FR9ppte790oLbIe0#athena-paul-horizon";

pub(crate) const RGB_STATE_ASSET_OWNER: &str = "assetOwner";
pub(crate) const RGB_STATE_INFLATION_ALLOWANCE: &str = "inflationAllowance";
#[cfg(feature = "_online")]
pub(crate) const RGB_GLOBAL_BURNED_ASSET: &str = "burnedAsset";
#[cfg(feature = "_online")]
pub(crate) const RGB_GLOBAL_BURN_REASON: &str = "burnReason";
pub(crate) const RGB_GLOBAL_ISSUED_SUPPLY: &str = "issuedSupply";
pub(crate) const RGB_GLOBAL_REJECT_LIST_URL: &str = "rejectListUrl";
#[cfg(feature = "_online")]
pub(crate) const RGB_METADATA_ALLOWED_INFLATION: &str = "allowedInflation";
