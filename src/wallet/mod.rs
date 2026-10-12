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
    Address, AssetBFA, AssetCFA, AssetFilter, AssetIFA, AssetNIA, AssetUDA, Assets,
    AssignmentsCollection, Balance, BlockTime, BtcBalance, DatabaseType, EmbeddedMedia, Invoice,
    InvoiceData, Media, Metadata, Online, Outpoint, PendingVanillaTx, ProofOfReserves,
    PsbtInputInfo, PsbtInspection, PsbtOutputInfo, ReceiveData, ReceivedMint, Recipient,
    RecipientInfo, RecipientType, RgbAllocation, Token, TokenLight, Transaction, TransactionType,
    Transfer, TransferKind, TransferTransportEndpoint, TransportEndpoint, Unspent, Utxo,
    WalletData, WalletDescriptors, WitnessData,
};
#[cfg(feature = "_online")]
pub use objects::{
    BitcoindAuth, BurnBeginResult, BurnDetails, IndexerConfig, IndexerProtocol, InflateBeginResult,
    InflateDetails, MintBeginResult, MintDetails, OnlineOptions, OperationResult, RefreshFilter,
    RefreshResult, RefreshTransferStatus, RefreshedTransfer, SendBeginResult, SendDetails,
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
    ReceivedConsignmentMeta, RefreshResultTrait, RejectList, RejectListSource,
    TryFailBatchTransferOutcome,
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

/// Margin, in seconds, by which a deadline must have passed on the local clock before
/// the corresponding proxy deadline is considered to have passed as well. It matches the
/// tolerance the proxy applies to timestamps.
#[cfg(feature = "_online")]
pub(crate) const PROXY_CLOCK_SKEW_MARGIN_SECS: u32 = 300;

/// Minimum time, in seconds, between the expiration of an outgoing operation and the earliest
/// invoice expiration among its recipients, so the TX can only be released while the recipients
/// are still waiting for it, leaving time to detect the ACKs and broadcast the TX and accounting
/// for clock skew (see `v1.0-changes-spec.md` 6.1).
#[cfg(feature = "_online")]
pub(crate) const INVOICE_EXPIRATION_MARGIN_SECS: u32 = 2 * PROXY_CLOCK_SKEW_MARGIN_SECS;

/// Minimum time, in seconds, the recipient's invoice must have left when a mint begins: the lock
/// has to be included on the external chain by the invoice expiration, so a shorter window would
/// leave too little time to make it.
#[cfg(feature = "_online")]
pub(crate) const MIN_MINT_LOCK_WINDOW_SECS: u32 = 60 * 60;

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

/// Time, in seconds, a mint whose lock was included on the external chain by its expiration is
/// still waited for past it, for the lock to be finalized. It only applies when such a lock
/// exists, so it can be generous enough for the slowest finality (and for finality stalls) of any
/// external chain.
#[cfg(feature = "_online")]
pub(crate) const MINT_FINALITY_GRACE_SECS: i64 = 24 * 60 * 60;

/// Time, in seconds, the block a received mint commits to (`after_block`, the latest finalized
/// external chain block when the mint began) may precede the creation of the recipient's invoice.
/// It covers the finality lag of the external chain (about 13 minutes on Ethereum, longer on some
/// L2s) and clock skew. An earlier block would only make the recipient scan more of the external
/// chain for the lock.
#[cfg(feature = "_online")]
pub(crate) const MINT_AFTER_BLOCK_GRACE_SECS: i64 = 60 * 60;

/// Maximum number of blocks, after the one a mint in the history of a received asset commits to
/// (its `after_block`), in which the event confirming it is searched for. It is about 70 days on
/// Ethereum, 11 on Base or Optimism and 37 hours on Arbitrum, at most 500 `eth_getLogs` calls per
/// mint.
///
/// The `after_block` is chosen by whoever created the mint, so without a limit an untrusted
/// consignment could make the scan go through the whole EVM chain (e.g. with `after_block` set to
/// 0). A mint whose event is not found within the limit makes the consignment refused, so the
/// limit needs to cover the time a minter may take to lock the funds after beginning the mint.
///
/// TODO: the limit will be set by each asset, with a global of the BFA schema (to be added in
/// rgb-schemas): remove this hardcoded value then.
#[cfg(feature = "_online")]
pub(crate) const MAX_MINT_SCAN_BLOCKS: u64 = 500_000;

/// Maximum number of blocks scanned for a mint event (see [`MAX_MINT_SCAN_BLOCKS`]).
#[cfg(feature = "_online")]
pub(crate) fn max_mint_scan_blocks() -> u64 {
    #[cfg(test)]
    if let Some(max_scan_blocks) = mock_max_mint_scan_blocks() {
        return max_scan_blocks;
    }
    MAX_MINT_SCAN_BLOCKS
}

/// Maximum proof of work difficulty (leading zero bits) the wallet accepts to compute when posting
/// a consignment; a proxy requiring more cannot be used to create transfers.
#[cfg(feature = "_online")]
pub(crate) const MAX_POW_DIFFICULTY: u32 = 28;

pub(crate) const RGB_STATE_ASSET_OWNER: &str = "assetOwner";
pub(crate) const RGB_STATE_INFLATION_ALLOWANCE: &str = "inflationAllowance";
pub(crate) const RGB_STATE_MINT_RIGHT: &str = "mintRight";
pub(crate) const RGB_GLOBAL_BRIDGE_LOCATION: &str = "bridgeLocation";
pub(crate) const RGB_GLOBAL_REJECT_LIST: &str = "rejectList";
pub(crate) const RGB_GLOBAL_REJECT_LIST_URL: &str = "rejectListUrl";
