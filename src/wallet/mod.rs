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

pub use crate::database::enums::TypeOfTransition;
pub use crate::no_wallet::{
    RgbInputInfo, RgbInspection, RgbOperationInfo, RgbOutputInfo, RgbTransitionInfo,
    TransitionGlobal, TransitionMetadata,
};
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
    PsbtOutputInfo, ReceiveData, Recipient, RecipientInfo, RecipientType, RgbAllocation, Token,
    TokenLight, Transaction, TransactionType, Transfer, TransferKind, TransferScope,
    TransferTransportEndpoint, TransportEndpoint, Unspent, Utxo, WalletData, WalletDescriptors,
    WitnessData,
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
    AssetInfo, AssetSpend, BeginOperationData, BtcChange, CleanAckOutcome, FailTransfersOutcome,
    LocalRecipient, LocalRecipientData, LocalWitnessData, OnlineData, PrepareRgbPsbtResult,
    PrepareTransferPsbtResult, ProxyReceiveData, ReceiveMatcher, ReceiveMode,
    ReceivedConsignmentMeta, RefreshResultTrait, RejectList, TryFailBatchTransferOutcome,
};
pub(crate) use objects::{
    AssetTransferColoringInfo, InfoAssetTransfer, InfoBatchTransfer, IssueData, IssuedAssetDetails,
    LocalAssetData, LocalRgbAllocation, LocalTransportEndpoint, LocalUnspent, ReceiveDataInternal,
    TransferData, TransferEndData,
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

/// Maximum time in the future, in seconds, an expiration can be set to, both for an invoice and
/// for an outgoing operation: about 10 years. A later one is refused with
/// [`Error::InvalidExpiration`].
///
/// Expirations are untrusted input (the one of an invoice being paid comes from the counterparty)
/// and the deadlines derived from them get the proxy retention grace and the margins of the
/// deadline checks added to them, which an expiration close to [`i64::MAX`] would overflow. An
/// invoice outliving any wallet that could pay it is of no use anyway, and a proxy refuses to
/// reserve one lasting more than its `max_expiry_secs` (30 days by default).
pub const MAX_EXPIRATION_SECS: u32 = 10 * 365 * 24 * 60 * 60;

/// Margin, in seconds, by which a deadline must have passed on the local clock before
/// the corresponding proxy deadline is considered to have passed as well. It matches the
/// tolerance the proxy applies to timestamps.
///
/// The docs of `send_begin` and `send_init` state, in minutes, the operation expiration
/// requirements following from it (this margin, [`INVOICE_EXPIRATION_MARGIN_SECS`] and their sum):
/// update them along with it.
#[cfg(feature = "_online")]
pub(crate) const PROXY_CLOCK_SKEW_MARGIN_SECS: u32 = 300;

/// Minimum time, in seconds, between the expiration of an outgoing operation and the earliest
/// invoice expiration among its recipients, so the TX can only be released while the recipients
/// are still waiting for it, leaving time to detect the ACKs and broadcast the TX and accounting
/// for clock skew (see `v1.0-changes-spec.md` 6.1).
#[cfg(feature = "_online")]
pub(crate) const INVOICE_EXPIRATION_MARGIN_SECS: u32 = 2 * PROXY_CLOCK_SKEW_MARGIN_SECS;

/// Minimum retention grace, in seconds, a recipient requires from the sender proxy
/// (`retention_grace_secs`) to ACK a transfer: it's the time a released signed TX can still be
/// retrieved after the operation expiration, so a recipient offline for longer might miss it. A
/// transfer whose sender proxy grants less is NACKed (see `v1.0-changes-spec.md` 6.4).
#[cfg(feature = "_online")]
pub(crate) const MIN_SENDER_RETENTION_GRACE_SECS: u32 = 86400;

/// Time, in seconds, a recipient waits after the broadcast deadline (the operation expiration plus
/// the sender proxy retention grace) before failing a transfer whose TX has not been confirmed,
/// giving a TX broadcast shortly before the deadline time to confirm and covering clock
/// differences (see `v1.0-changes-spec.md` 6.4).
#[cfg(feature = "_online")]
pub(crate) const CONFIRMATION_MARGIN_SECS: u32 = 86400;

/// Maximum proof of work difficulty (leading zero bits) the wallet accepts to compute when posting
/// a consignment; a proxy requiring more cannot be used to create transfers.
#[cfg(feature = "_online")]
pub(crate) const MAX_POW_DIFFICULTY: u32 = 28;

pub(crate) const RGB_STATE_ASSET_OWNER: &str = "assetOwner";
pub(crate) const RGB_STATE_INFLATION_ALLOWANCE: &str = "inflationAllowance";
pub(crate) const RGB_GLOBAL_REJECT_LIST_URL: &str = "rejectListUrl";
