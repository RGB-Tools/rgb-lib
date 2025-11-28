#![allow(clippy::too_many_arguments)]

use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard},
};

use rgb_lib::{
    AssetSchema, Assignment as RgbLibAssignment, CloseMethod, Error as RgbLibError, TransferStatus,
    TransportType,
    keys::Keys,
    utils::BitcoinNetwork as RgbLibBitcoinNetwork,
    wallet::{
        Address as RgbLibAddress, AssetCFA, AssetIFA, AssetNIA, AssetUDA, Assets,
        AssignmentsCollection, Balance, BlockTime, BtcBalance, Cosigner as CosignerData,
        DatabaseType, EmbeddedMedia, HubInfo, InflateBeginResult, InflateDetails,
        InitOperationResult, Invoice as RgbLibInvoice, InvoiceData as RgbLibInvoiceData, Media,
        Metadata, MultisigKeys, MultisigVotingStatus as RgbLibMultisigVotingStatus,
        MultisigWallet as RgbLibMultisigWallet, Online, Operation as RgbLibOperation,
        OperationInfo as RgbLibOperationInfo, OperationResult, Outpoint, ProofOfReserves,
        PsbtInputInfo, PsbtInspection, PsbtOutputInfo, ReceiveData, Recipient as RgbLibRecipient,
        RecipientInfo as RgbLibRecipientInfo, RecipientType, RefreshFilter, RefreshTransferStatus,
        RefreshedTransfer, RespondToOperation as RgbLibRespondToOperation,
        RgbAllocation as RgbLibRgbAllocation, RgbInputInfo as RgbLibRgbInputInfo,
        RgbInspection as RgbLibRgbInspection, RgbOperationInfo as RgbLibRgbOperationInfo,
        RgbOutputInfo as RgbLibRgbOutputInfo, RgbTransitionInfo as RgbLibRgbTransitionInfo,
        RgbWalletOpsOffline, RgbWalletOpsOnline, SendBeginResult, SendDetails, SinglesigKeys,
        Token, TokenLight, Transaction, TransactionType, Transfer as RgbLibTransfer, TransferKind,
        TransferTransportEndpoint, TransportEndpoint as RgbLibTransportEndpoint, TypeOfTransition,
        Unspent as RgbLibUnspent, UserRole, Utxo, Wallet as RgbLibWallet,
        WalletData as RgbLibWalletData, WalletDescriptors, WitnessData,
    },
};

uniffi::include_scaffolding!("rgb-lib");

// temporary solution needed because the Enum attribute doesn't support the Remote one
pub enum BitcoinNetwork {
    Mainnet,
    Testnet,
    Testnet4,
    Signet,
    Regtest,
    SignetCustom { genesis_hash: Vec<u8> },
}
impl From<RgbLibBitcoinNetwork> for BitcoinNetwork {
    fn from(orig: RgbLibBitcoinNetwork) -> Self {
        match orig {
            RgbLibBitcoinNetwork::Mainnet => BitcoinNetwork::Mainnet,
            RgbLibBitcoinNetwork::Testnet => BitcoinNetwork::Testnet,
            RgbLibBitcoinNetwork::Testnet4 => BitcoinNetwork::Testnet4,
            RgbLibBitcoinNetwork::Signet => BitcoinNetwork::Signet,
            RgbLibBitcoinNetwork::Regtest => BitcoinNetwork::Regtest,
            RgbLibBitcoinNetwork::SignetCustom(genesis_hash) => BitcoinNetwork::SignetCustom {
                genesis_hash: genesis_hash.to_vec(),
            },
        }
    }
}
impl TryFrom<BitcoinNetwork> for RgbLibBitcoinNetwork {
    type Error = RgbLibError;

    fn try_from(orig: BitcoinNetwork) -> Result<Self, Self::Error> {
        Ok(match orig {
            BitcoinNetwork::Mainnet => RgbLibBitcoinNetwork::Mainnet,
            BitcoinNetwork::Testnet => RgbLibBitcoinNetwork::Testnet,
            BitcoinNetwork::Testnet4 => RgbLibBitcoinNetwork::Testnet4,
            BitcoinNetwork::Signet => RgbLibBitcoinNetwork::Signet,
            BitcoinNetwork::Regtest => RgbLibBitcoinNetwork::Regtest,
            BitcoinNetwork::SignetCustom { genesis_hash } => {
                RgbLibBitcoinNetwork::SignetCustom(genesis_hash.try_into().map_err(|_| {
                    RgbLibError::InvalidBitcoinNetwork {
                        network: "SignetCustom".to_string(),
                    }
                })?)
            }
        })
    }
}

// temporary solution needed because the Enum attribute doesn't support the Remote one
pub enum Assignment {
    Fungible { amount: u64 },
    NonFungible,
    InflationRight { amount: u64 },
    Any,
}
impl From<RgbLibAssignment> for Assignment {
    fn from(orig: RgbLibAssignment) -> Self {
        match orig {
            RgbLibAssignment::Fungible(amount) => Assignment::Fungible { amount },
            RgbLibAssignment::NonFungible => Assignment::NonFungible,
            RgbLibAssignment::InflationRight(amount) => Assignment::InflationRight { amount },
            RgbLibAssignment::Any => Assignment::Any,
        }
    }
}
impl From<Assignment> for RgbLibAssignment {
    fn from(orig: Assignment) -> Self {
        match orig {
            Assignment::Fungible { amount } => RgbLibAssignment::Fungible(amount),
            Assignment::NonFungible => RgbLibAssignment::NonFungible,
            Assignment::InflationRight { amount } => RgbLibAssignment::InflationRight(amount),
            Assignment::Any => RgbLibAssignment::Any,
        }
    }
}
pub struct InvoiceData {
    pub recipient_id: String,
    pub asset_schema: Option<AssetSchema>,
    pub asset_id: Option<String>,
    pub assignment: Assignment,
    pub assignment_name: Option<String>,
    pub network: BitcoinNetwork,
    pub expiration_timestamp: Option<u64>,
    pub transport_endpoints: Vec<String>,
}
impl From<RgbLibInvoiceData> for InvoiceData {
    fn from(orig: RgbLibInvoiceData) -> Self {
        Self {
            recipient_id: orig.recipient_id,
            asset_schema: orig.asset_schema,
            asset_id: orig.asset_id,
            assignment: orig.assignment.into(),
            assignment_name: orig.assignment_name,
            network: orig.network.into(),
            expiration_timestamp: orig.expiration_timestamp,
            transport_endpoints: orig.transport_endpoints,
        }
    }
}
impl TryFrom<InvoiceData> for RgbLibInvoiceData {
    type Error = RgbLibError;

    fn try_from(orig: InvoiceData) -> Result<Self, Self::Error> {
        Ok(RgbLibInvoiceData {
            recipient_id: orig.recipient_id,
            asset_schema: orig.asset_schema,
            asset_id: orig.asset_id,
            assignment: orig.assignment.into(),
            assignment_name: orig.assignment_name,
            network: orig.network.try_into()?,
            expiration_timestamp: orig.expiration_timestamp,
            transport_endpoints: orig.transport_endpoints,
        })
    }
}
pub struct Recipient {
    pub recipient_id: String,
    pub witness_data: Option<WitnessData>,
    pub assignment: Assignment,
    pub transport_endpoints: Vec<String>,
}
impl From<RgbLibRecipient> for Recipient {
    fn from(orig: RgbLibRecipient) -> Self {
        Self {
            recipient_id: orig.recipient_id,
            witness_data: orig.witness_data,
            assignment: orig.assignment.into(),
            transport_endpoints: orig.transport_endpoints,
        }
    }
}
impl From<Recipient> for RgbLibRecipient {
    fn from(orig: Recipient) -> Self {
        Self {
            recipient_id: orig.recipient_id,
            witness_data: orig.witness_data,
            assignment: orig.assignment.into(),
            transport_endpoints: orig.transport_endpoints,
        }
    }
}
pub struct RgbAllocation {
    pub asset_id: Option<String>,
    pub assignment: Assignment,
    pub settled: bool,
}
impl From<RgbLibRgbAllocation> for RgbAllocation {
    fn from(orig: RgbLibRgbAllocation) -> Self {
        Self {
            asset_id: orig.asset_id,
            assignment: orig.assignment.into(),
            settled: orig.settled,
        }
    }
}
impl From<RgbAllocation> for RgbLibRgbAllocation {
    fn from(orig: RgbAllocation) -> Self {
        Self {
            asset_id: orig.asset_id,
            assignment: orig.assignment.into(),
            settled: orig.settled,
        }
    }
}
pub struct Transfer {
    pub idx: i32,
    pub batch_transfer_idx: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: TransferStatus,
    pub requested_assignment: Option<Assignment>,
    pub assignments: Vec<Assignment>,
    pub kind: TransferKind,
    pub txid: Option<String>,
    pub recipient_id: Option<String>,
    pub receive_utxo: Option<Outpoint>,
    pub change_utxo: Option<Outpoint>,
    pub expiration_timestamp: Option<u64>,
    pub transport_endpoints: Vec<TransferTransportEndpoint>,
    pub invoice_string: Option<String>,
    pub consignment_path: Option<String>,
}
impl From<RgbLibTransfer> for Transfer {
    fn from(orig: RgbLibTransfer) -> Self {
        Self {
            idx: orig.idx,
            batch_transfer_idx: orig.batch_transfer_idx,
            created_at: orig.created_at,
            updated_at: orig.updated_at,
            status: orig.status,
            requested_assignment: orig.requested_assignment.map(|a| a.into()),
            assignments: orig.assignments.into_iter().map(|a| a.into()).collect(),
            kind: orig.kind,
            txid: orig.txid,
            recipient_id: orig.recipient_id,
            receive_utxo: orig.receive_utxo,
            change_utxo: orig.change_utxo,
            expiration_timestamp: orig.expiration_timestamp,
            transport_endpoints: orig.transport_endpoints,
            invoice_string: orig.invoice_string.clone(),
            consignment_path: orig.consignment_path.clone(),
        }
    }
}
impl From<Transfer> for RgbLibTransfer {
    fn from(orig: Transfer) -> Self {
        Self {
            idx: orig.idx,
            batch_transfer_idx: orig.batch_transfer_idx,
            created_at: orig.created_at,
            updated_at: orig.updated_at,
            status: orig.status,
            requested_assignment: orig.requested_assignment.map(|a| a.into()),
            assignments: orig.assignments.into_iter().map(|a| a.into()).collect(),
            kind: orig.kind,
            txid: orig.txid,
            recipient_id: orig.recipient_id,
            receive_utxo: orig.receive_utxo,
            change_utxo: orig.change_utxo,
            expiration_timestamp: orig.expiration_timestamp,
            transport_endpoints: orig.transport_endpoints,
            invoice_string: orig.invoice_string.clone(),
            consignment_path: orig.consignment_path.clone(),
        }
    }
}
pub struct Unspent {
    pub utxo: Utxo,
    pub rgb_allocations: Vec<RgbAllocation>,
    pub pending_blinded: u32,
}
impl From<RgbLibUnspent> for Unspent {
    fn from(orig: RgbLibUnspent) -> Self {
        Self {
            utxo: orig.utxo,
            rgb_allocations: orig.rgb_allocations.into_iter().map(|a| a.into()).collect(),
            pending_blinded: orig.pending_blinded,
        }
    }
}
impl From<Unspent> for RgbLibUnspent {
    fn from(orig: Unspent) -> Self {
        Self {
            utxo: orig.utxo,
            rgb_allocations: orig.rgb_allocations.into_iter().map(|a| a.into()).collect(),
            pending_blinded: orig.pending_blinded,
        }
    }
}
pub struct WalletData {
    pub data_dir: String,
    pub bitcoin_network: BitcoinNetwork,
    pub database_type: DatabaseType,
    pub max_allocations_per_utxo: u32,
    pub supported_schemas: Vec<AssetSchema>,
}
impl From<RgbLibWalletData> for WalletData {
    fn from(orig: RgbLibWalletData) -> Self {
        Self {
            data_dir: orig.data_dir,
            bitcoin_network: orig.bitcoin_network.into(),
            database_type: orig.database_type,
            max_allocations_per_utxo: orig.max_allocations_per_utxo,
            supported_schemas: orig.supported_schemas,
        }
    }
}
impl TryFrom<WalletData> for RgbLibWalletData {
    type Error = RgbLibError;

    fn try_from(orig: WalletData) -> Result<Self, Self::Error> {
        Ok(Self {
            data_dir: orig.data_dir,
            bitcoin_network: orig.bitcoin_network.try_into()?,
            database_type: orig.database_type,
            max_allocations_per_utxo: orig.max_allocations_per_utxo,
            supported_schemas: orig.supported_schemas,
        })
    }
}
pub struct RgbInputInfo {
    pub vin: u32,
    pub assignment: Assignment,
}
impl From<RgbLibRgbInputInfo> for RgbInputInfo {
    fn from(orig: RgbLibRgbInputInfo) -> Self {
        Self {
            vin: orig.vin,
            assignment: orig.assignment.into(),
        }
    }
}
impl From<RgbInputInfo> for RgbLibRgbInputInfo {
    fn from(orig: RgbInputInfo) -> Self {
        Self {
            vin: orig.vin,
            assignment: orig.assignment.into(),
        }
    }
}
pub struct RgbOutputInfo {
    pub vout: Option<u32>,
    pub assignment: Assignment,
    pub is_concealed: bool,
    pub is_ours: bool,
}
impl From<RgbLibRgbOutputInfo> for RgbOutputInfo {
    fn from(orig: RgbLibRgbOutputInfo) -> Self {
        Self {
            vout: orig.vout,
            assignment: orig.assignment.into(),
            is_concealed: orig.is_concealed,
            is_ours: orig.is_ours,
        }
    }
}
impl From<RgbOutputInfo> for RgbLibRgbOutputInfo {
    fn from(orig: RgbOutputInfo) -> Self {
        Self {
            vout: orig.vout,
            assignment: orig.assignment.into(),
            is_concealed: orig.is_concealed,
            is_ours: orig.is_ours,
        }
    }
}
pub struct RgbTransitionInfo {
    pub r#type: TypeOfTransition,
    pub inputs: Vec<RgbInputInfo>,
    pub outputs: Vec<RgbOutputInfo>,
}
impl From<RgbLibRgbTransitionInfo> for RgbTransitionInfo {
    fn from(orig: RgbLibRgbTransitionInfo) -> Self {
        Self {
            r#type: orig.r#type,
            inputs: orig.inputs.into_iter().map(|i| i.into()).collect(),
            outputs: orig.outputs.into_iter().map(|o| o.into()).collect(),
        }
    }
}
impl From<RgbTransitionInfo> for RgbLibRgbTransitionInfo {
    fn from(orig: RgbTransitionInfo) -> Self {
        Self {
            r#type: orig.r#type,
            inputs: orig.inputs.into_iter().map(|i| i.into()).collect(),
            outputs: orig.outputs.into_iter().map(|o| o.into()).collect(),
        }
    }
}
pub struct RgbOperationInfo {
    pub asset_id: String,
    pub transitions: Vec<RgbTransitionInfo>,
}
impl From<RgbLibRgbOperationInfo> for RgbOperationInfo {
    fn from(orig: RgbLibRgbOperationInfo) -> Self {
        Self {
            asset_id: orig.asset_id,
            transitions: orig.transitions.into_iter().map(|t| t.into()).collect(),
        }
    }
}
impl From<RgbOperationInfo> for RgbLibRgbOperationInfo {
    fn from(orig: RgbOperationInfo) -> Self {
        Self {
            asset_id: orig.asset_id,
            transitions: orig.transitions.into_iter().map(|t| t.into()).collect(),
        }
    }
}
pub struct RgbInspection {
    pub close_method: CloseMethod,
    pub commitment_hex: String,
    pub operations: Vec<RgbOperationInfo>,
}
impl From<RgbLibRgbInspection> for RgbInspection {
    fn from(orig: RgbLibRgbInspection) -> Self {
        Self {
            close_method: orig.close_method,
            commitment_hex: orig.commitment_hex,
            operations: orig.operations.into_iter().map(|a| a.into()).collect(),
        }
    }
}
impl From<RgbInspection> for RgbLibRgbInspection {
    fn from(orig: RgbInspection) -> Self {
        Self {
            close_method: orig.close_method,
            commitment_hex: orig.commitment_hex,
            operations: orig.operations.into_iter().map(|a| a.into()).collect(),
        }
    }
}

// temporary solution needed because the Enum attribute doesn't support the Remote one
pub enum Operation {
    CreateUtxosToReview {
        psbt: String,
        status: MultisigVotingStatus,
    },
    CreateUtxosPending {
        status: MultisigVotingStatus,
    },
    CreateUtxosCompleted {
        txid: String,
        status: MultisigVotingStatus,
    },
    CreateUtxosDiscarded {
        status: MultisigVotingStatus,
    },
    SendBtcToReview {
        psbt: String,
        status: MultisigVotingStatus,
    },
    SendBtcPending {
        status: MultisigVotingStatus,
    },
    SendBtcCompleted {
        txid: String,
        status: MultisigVotingStatus,
    },
    SendBtcDiscarded {
        status: MultisigVotingStatus,
    },
    SendToReview {
        psbt: String,
        details: SendDetails,
        status: MultisigVotingStatus,
    },
    SendPending {
        details: SendDetails,
        status: MultisigVotingStatus,
    },
    SendCompleted {
        txid: String,
        details: SendDetails,
        status: MultisigVotingStatus,
    },
    SendDiscarded {
        details: SendDetails,
        status: MultisigVotingStatus,
    },
    InflationToReview {
        psbt: String,
        details: InflateDetails,
        status: MultisigVotingStatus,
    },
    InflationPending {
        details: InflateDetails,
        status: MultisigVotingStatus,
    },
    InflationCompleted {
        txid: String,
        details: InflateDetails,
        status: MultisigVotingStatus,
    },
    InflationDiscarded {
        details: InflateDetails,
        status: MultisigVotingStatus,
    },
    IssuanceCompleted {
        asset_id: String,
    },
    BlindReceiveCompleted {
        details: ReceiveData,
    },
    WitnessReceiveCompleted {
        details: ReceiveData,
    },
}
pub struct MultisigVotingStatus {
    pub acked_by: Vec<String>,
    pub nacked_by: Vec<String>,
    pub threshold: u8,
    pub my_response: Option<bool>,
}

impl From<RgbLibMultisigVotingStatus> for MultisigVotingStatus {
    fn from(orig: RgbLibMultisigVotingStatus) -> Self {
        Self {
            acked_by: orig.acked_by.into_iter().collect(),
            nacked_by: orig.nacked_by.into_iter().collect(),
            threshold: orig.threshold,
            my_response: orig.my_response,
        }
    }
}
impl From<MultisigVotingStatus> for RgbLibMultisigVotingStatus {
    fn from(orig: MultisigVotingStatus) -> Self {
        Self {
            acked_by: orig.acked_by.into_iter().collect(),
            nacked_by: orig.nacked_by.into_iter().collect(),
            threshold: orig.threshold,
            my_response: orig.my_response,
        }
    }
}

impl From<RgbLibOperation> for Operation {
    fn from(orig: RgbLibOperation) -> Self {
        match orig {
            RgbLibOperation::CreateUtxosToReview { psbt, status } => {
                Operation::CreateUtxosToReview {
                    psbt,
                    status: status.into(),
                }
            }
            RgbLibOperation::CreateUtxosPending { status } => Operation::CreateUtxosPending {
                status: status.into(),
            },
            RgbLibOperation::CreateUtxosCompleted { txid, status } => {
                Operation::CreateUtxosCompleted {
                    txid,
                    status: status.into(),
                }
            }
            RgbLibOperation::CreateUtxosDiscarded { status } => Operation::CreateUtxosDiscarded {
                status: status.into(),
            },
            RgbLibOperation::SendBtcToReview { psbt, status } => Operation::SendBtcToReview {
                psbt,
                status: status.into(),
            },
            RgbLibOperation::SendBtcPending { status } => Operation::SendBtcPending {
                status: status.into(),
            },
            RgbLibOperation::SendBtcCompleted { txid, status } => Operation::SendBtcCompleted {
                txid,
                status: status.into(),
            },
            RgbLibOperation::SendBtcDiscarded { status } => Operation::SendBtcDiscarded {
                status: status.into(),
            },
            RgbLibOperation::SendToReview {
                psbt,
                details,
                status,
            } => Operation::SendToReview {
                psbt,
                details,
                status: status.into(),
            },
            RgbLibOperation::SendPending { details, status } => Operation::SendPending {
                details,
                status: status.into(),
            },
            RgbLibOperation::SendCompleted {
                txid,
                details,
                status,
            } => Operation::SendCompleted {
                txid,
                details,
                status: status.into(),
            },
            RgbLibOperation::SendDiscarded { details, status } => Operation::SendDiscarded {
                details,
                status: status.into(),
            },
            RgbLibOperation::InflationToReview {
                psbt,
                details,
                status,
            } => Operation::InflationToReview {
                psbt,
                details,
                status: status.into(),
            },
            RgbLibOperation::InflationPending { status, details } => Operation::InflationPending {
                details,
                status: status.into(),
            },
            RgbLibOperation::InflationCompleted {
                txid,
                details,
                status,
            } => Operation::InflationCompleted {
                txid,
                details,
                status: status.into(),
            },
            RgbLibOperation::InflationDiscarded { details, status } => {
                Operation::InflationDiscarded {
                    details,
                    status: status.into(),
                }
            }
            RgbLibOperation::IssuanceCompleted { asset_id } => {
                Operation::IssuanceCompleted { asset_id }
            }
            RgbLibOperation::BlindReceiveCompleted { details } => {
                Operation::BlindReceiveCompleted { details }
            }
            RgbLibOperation::WitnessReceiveCompleted { details } => {
                Operation::WitnessReceiveCompleted { details }
            }
        }
    }
}
impl From<Operation> for RgbLibOperation {
    fn from(orig: Operation) -> Self {
        match orig {
            Operation::CreateUtxosToReview { psbt, status } => {
                RgbLibOperation::CreateUtxosToReview {
                    psbt,
                    status: status.into(),
                }
            }
            Operation::CreateUtxosPending { status } => RgbLibOperation::CreateUtxosPending {
                status: status.into(),
            },
            Operation::CreateUtxosCompleted { txid, status } => {
                RgbLibOperation::CreateUtxosCompleted {
                    txid,
                    status: status.into(),
                }
            }
            Operation::CreateUtxosDiscarded { status } => RgbLibOperation::CreateUtxosDiscarded {
                status: status.into(),
            },
            Operation::SendBtcToReview { psbt, status } => RgbLibOperation::SendBtcToReview {
                psbt,
                status: status.into(),
            },
            Operation::SendBtcPending { status } => RgbLibOperation::SendBtcPending {
                status: status.into(),
            },
            Operation::SendBtcCompleted { txid, status } => RgbLibOperation::SendBtcCompleted {
                txid,
                status: status.into(),
            },
            Operation::SendBtcDiscarded { status } => RgbLibOperation::SendBtcDiscarded {
                status: status.into(),
            },
            Operation::SendToReview {
                psbt,
                details,
                status,
            } => RgbLibOperation::SendToReview {
                psbt,
                details,
                status: status.into(),
            },
            Operation::SendPending { details, status } => RgbLibOperation::SendPending {
                details,
                status: status.into(),
            },
            Operation::SendCompleted {
                txid,
                details,
                status,
            } => RgbLibOperation::SendCompleted {
                txid,
                details,
                status: status.into(),
            },
            Operation::SendDiscarded { details, status } => RgbLibOperation::SendDiscarded {
                details,
                status: status.into(),
            },
            Operation::InflationToReview {
                psbt,
                details,
                status,
            } => RgbLibOperation::InflationToReview {
                psbt,
                details,
                status: status.into(),
            },
            Operation::InflationPending { status, details } => RgbLibOperation::InflationPending {
                details,
                status: status.into(),
            },
            Operation::InflationCompleted {
                txid,
                details,
                status,
            } => RgbLibOperation::InflationCompleted {
                txid,
                details,
                status: status.into(),
            },
            Operation::InflationDiscarded { details, status } => {
                RgbLibOperation::InflationDiscarded {
                    details,
                    status: status.into(),
                }
            }
            Operation::IssuanceCompleted { asset_id } => {
                RgbLibOperation::IssuanceCompleted { asset_id }
            }
            Operation::BlindReceiveCompleted { details } => {
                RgbLibOperation::BlindReceiveCompleted { details }
            }
            Operation::WitnessReceiveCompleted { details } => {
                RgbLibOperation::WitnessReceiveCompleted { details }
            }
        }
    }
}
pub struct OperationInfo {
    pub operation_idx: i32,
    pub initiator_xpub: String,
    pub operation: Operation,
}
impl From<RgbLibOperationInfo> for OperationInfo {
    fn from(orig: RgbLibOperationInfo) -> Self {
        Self {
            operation_idx: orig.operation_idx,
            initiator_xpub: orig.initiator_xpub,
            operation: orig.operation.into(),
        }
    }
}

// temporary solution needed because the Enum attribute doesn't support the Remote one
pub enum RespondToOperation {
    Ack { signed_psbt: String },
    Nack,
}
impl From<RgbLibRespondToOperation> for RespondToOperation {
    fn from(orig: RgbLibRespondToOperation) -> Self {
        match orig {
            RgbLibRespondToOperation::Ack(signed_psbt) => RespondToOperation::Ack { signed_psbt },
            RgbLibRespondToOperation::Nack => RespondToOperation::Nack,
        }
    }
}
impl From<RespondToOperation> for RgbLibRespondToOperation {
    fn from(orig: RespondToOperation) -> Self {
        match orig {
            RespondToOperation::Ack { signed_psbt } => RgbLibRespondToOperation::Ack(signed_psbt),
            RespondToOperation::Nack => RgbLibRespondToOperation::Nack,
        }
    }
}

fn generate_keys(bitcoin_network: BitcoinNetwork) -> Result<Keys, RgbLibError> {
    Ok(rgb_lib::keys::generate_keys(bitcoin_network.try_into()?))
}

fn restore_keys(bitcoin_network: BitcoinNetwork, mnemonic: String) -> Result<Keys, RgbLibError> {
    rgb_lib::keys::restore_keys(bitcoin_network.try_into()?, mnemonic)
}

fn restore_backup(
    backup_path: String,
    password: String,
    data_dir: String,
) -> Result<(), RgbLibError> {
    rgb_lib::wallet::restore_backup(&backup_path, &password, &data_dir)
}

struct RecipientInfo {
    recipient_info: RwLock<RgbLibRecipientInfo>,
}

impl RecipientInfo {
    fn new(recipient_id: String) -> Result<Self, RgbLibError> {
        Ok(RecipientInfo {
            recipient_info: RwLock::new(RgbLibRecipientInfo::new(recipient_id)?),
        })
    }

    fn _get_recipient_info(&self) -> RwLockReadGuard<'_, RgbLibRecipientInfo> {
        self.recipient_info.read().expect("recipient_info")
    }

    fn network(&self) -> BitcoinNetwork {
        self._get_recipient_info().network.into()
    }

    fn recipient_type(&self) -> RecipientType {
        self._get_recipient_info().recipient_type
    }
}

struct TransportEndpoint {
    transport_endpoint: RwLock<RgbLibTransportEndpoint>,
}

impl TransportEndpoint {
    fn new(transport_endpoint: String) -> Result<Self, RgbLibError> {
        Ok(TransportEndpoint {
            transport_endpoint: RwLock::new(RgbLibTransportEndpoint::new(transport_endpoint)?),
        })
    }

    fn _get_transport_endpoint(&self) -> RwLockReadGuard<'_, RgbLibTransportEndpoint> {
        self.transport_endpoint.read().expect("transport_endpoint")
    }

    fn transport_type(&self) -> TransportType {
        self._get_transport_endpoint().transport_type()
    }
}

struct Address {
    _address: RwLock<RgbLibAddress>,
}

impl Address {
    fn new(address_string: String, bitcoin_network: BitcoinNetwork) -> Result<Self, RgbLibError> {
        Ok(Address {
            _address: RwLock::new(RgbLibAddress::new(
                address_string,
                bitcoin_network.try_into()?,
            )?),
        })
    }
}

struct Cosigner {
    cosigner: RwLock<CosignerData>,
}

impl Cosigner {
    fn new(cosigner_string: String) -> Result<Self, RgbLibError> {
        Ok(Cosigner {
            cosigner: RwLock::new(CosignerData::from_str(&cosigner_string)?),
        })
    }

    fn from_data(data: CosignerData) -> Result<Self, RgbLibError> {
        Ok(Cosigner {
            cosigner: RwLock::new(data),
        })
    }

    fn _get_cosigner(&self) -> RwLockReadGuard<'_, CosignerData> {
        self.cosigner.read().expect("cosigner")
    }

    fn cosigner_string(&self) -> String {
        self._get_cosigner().to_string()
    }

    fn cosigner_data(&self) -> CosignerData {
        self._get_cosigner().clone()
    }
}

struct Invoice {
    invoice: RwLock<RgbLibInvoice>,
}

impl Invoice {
    fn new(invoice_string: String) -> Result<Self, RgbLibError> {
        Ok(Invoice {
            invoice: RwLock::new(RgbLibInvoice::new(invoice_string)?),
        })
    }

    fn _get_invoice(&self) -> RwLockReadGuard<'_, RgbLibInvoice> {
        self.invoice.read().expect("invoice")
    }

    fn invoice_data(&self) -> InvoiceData {
        self._get_invoice().invoice_data().into()
    }

    fn invoice_string(&self) -> String {
        self._get_invoice().invoice_string()
    }
}

struct Wallet {
    wallet_mutex: Mutex<RgbLibWallet>,
}

impl Wallet {
    fn new(wallet_data: WalletData, keys: SinglesigKeys) -> Result<Self, RgbLibError> {
        Ok(Wallet {
            wallet_mutex: Mutex::new(RgbLibWallet::new(wallet_data.try_into()?, keys)?),
        })
    }

    fn _get_wallet(&self) -> MutexGuard<'_, RgbLibWallet> {
        self.wallet_mutex.lock().expect("wallet")
    }

    fn get_wallet_data(&self) -> WalletData {
        self._get_wallet().get_wallet_data().into()
    }

    fn get_keys(&self) -> SinglesigKeys {
        self._get_wallet().get_keys()
    }

    fn get_descriptors(&self) -> WalletDescriptors {
        self._get_wallet().get_descriptors()
    }

    fn get_wallet_dir(&self) -> String {
        self._get_wallet()
            .get_wallet_dir()
            .to_string_lossy()
            .to_string()
    }

    fn get_media_dir(&self) -> String {
        self._get_wallet()
            .get_media_dir()
            .to_string_lossy()
            .to_string()
    }

    fn backup(&self, backup_path: String, password: String) -> Result<(), RgbLibError> {
        self._get_wallet().backup(&backup_path, &password)
    }

    fn backup_info(&self) -> Result<bool, RgbLibError> {
        self._get_wallet().backup_info()
    }

    fn blind_receive(
        &self,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, RgbLibError> {
        self._get_wallet().blind_receive(
            asset_id,
            assignment.into(),
            expiration_timestamp,
            transport_endpoints,
            min_confirmations,
        )
    }

    fn witness_receive(
        &self,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, RgbLibError> {
        self._get_wallet().witness_receive(
            asset_id,
            assignment.into(),
            expiration_timestamp,
            transport_endpoints,
            min_confirmations,
        )
    }

    fn finalize_psbt(&self, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().finalize_psbt(signed_psbt, None)
    }

    fn sign_psbt(&self, unsigned_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().sign_psbt(unsigned_psbt, None)
    }

    fn create_utxos(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<u8, RgbLibError> {
        self._get_wallet()
            .create_utxos(online, up_to, num, size, fee_rate, skip_sync)
    }

    fn create_utxos_begin(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .create_utxos_begin(online, up_to, num, size, fee_rate, skip_sync)
    }

    fn create_utxos_end(
        &self,
        online: Online,
        signed_psbt: String,
        skip_sync: bool,
    ) -> Result<u8, RgbLibError> {
        self._get_wallet()
            .create_utxos_end(online, signed_psbt, skip_sync)
    }

    fn delete_transfers(
        &self,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
    ) -> Result<bool, RgbLibError> {
        self._get_wallet()
            .delete_transfers(batch_transfer_idx, no_asset_only)
    }

    fn drain_to(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
        fee_rate: u64,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .drain_to(online, address, destroy_assets, fee_rate)
    }

    fn drain_to_begin(
        &self,
        online: Online,
        address: String,
        destroy_assets: bool,
        fee_rate: u64,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .drain_to_begin(online, address, destroy_assets, fee_rate)
    }

    fn drain_to_end(&self, online: Online, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().drain_to_end(online, signed_psbt)
    }

    fn fail_transfers(
        &self,
        online: Online,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
        skip_sync: bool,
    ) -> Result<bool, RgbLibError> {
        self._get_wallet()
            .fail_transfers(online, batch_transfer_idx, no_asset_only, skip_sync)
    }

    fn get_address(&self) -> Result<String, RgbLibError> {
        self._get_wallet().get_address()
    }

    fn get_asset_balance(&self, asset_id: String) -> Result<Balance, RgbLibError> {
        self._get_wallet().get_asset_balance(asset_id)
    }

    fn get_btc_balance(
        &self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<BtcBalance, RgbLibError> {
        self._get_wallet().get_btc_balance(online, skip_sync)
    }

    fn get_asset_metadata(&self, asset_id: String) -> Result<Metadata, RgbLibError> {
        self._get_wallet().get_asset_metadata(asset_id)
    }

    fn get_fee_estimation(&self, online: Online, blocks: u16) -> Result<f64, RgbLibError> {
        self._get_wallet().get_fee_estimation(online, blocks)
    }

    fn go_online(
        &self,
        skip_consistency_check: bool,
        indexer_url: String,
    ) -> Result<Online, RgbLibError> {
        self._get_wallet()
            .go_online(skip_consistency_check, indexer_url)
    }

    fn inflate(
        &self,
        online: Online,
        asset_id: String,
        inflation_amounts: Vec<u64>,
        fee_rate: u64,
        min_confirmations: u8,
    ) -> Result<OperationResult, RgbLibError> {
        self._get_wallet().inflate(
            online,
            asset_id,
            inflation_amounts,
            fee_rate,
            min_confirmations,
        )
    }

    fn inflate_begin(
        &self,
        online: Online,
        asset_id: String,
        inflation_amounts: Vec<u64>,
        fee_rate: u64,
        min_confirmations: u8,
    ) -> Result<InflateBeginResult, RgbLibError> {
        self._get_wallet().inflate_begin(
            online,
            asset_id,
            inflation_amounts,
            fee_rate,
            min_confirmations,
        )
    }

    fn inflate_end(
        &self,
        online: Online,
        signed_psbt: String,
    ) -> Result<OperationResult, RgbLibError> {
        self._get_wallet().inflate_end(online, signed_psbt)
    }

    fn issue_asset_nia(
        &self,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetNIA, RgbLibError> {
        self._get_wallet()
            .issue_asset_nia(ticker, name, precision, amounts)
    }

    fn issue_asset_uda(
        &self,
        ticker: String,
        name: String,
        details: Option<String>,
        precision: u8,
        media_file_path: Option<String>,
        attachments_file_paths: Vec<String>,
    ) -> Result<AssetUDA, RgbLibError> {
        self._get_wallet().issue_asset_uda(
            ticker,
            name,
            details,
            precision,
            media_file_path,
            attachments_file_paths,
        )
    }

    fn issue_asset_cfa(
        &self,
        name: String,
        details: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, RgbLibError> {
        self._get_wallet()
            .issue_asset_cfa(name, details, precision, amounts, file_path)
    }

    fn issue_asset_ifa(
        &self,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
        inflation_amounts: Vec<u64>,
        reject_list_url: Option<String>,
    ) -> Result<AssetIFA, RgbLibError> {
        self._get_wallet().issue_asset_ifa(
            ticker,
            name,
            precision,
            amounts,
            inflation_amounts,
            reject_list_url,
        )
    }

    fn list_assets(&self, filter_asset_schemas: Vec<AssetSchema>) -> Result<Assets, RgbLibError> {
        self._get_wallet().list_assets(filter_asset_schemas)
    }

    fn list_transactions(
        &self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<Vec<Transaction>, RgbLibError> {
        self._get_wallet().list_transactions(online, skip_sync)
    }

    fn list_transfers(&self, asset_id: Option<String>) -> Result<Vec<Transfer>, RgbLibError> {
        Ok(self
            ._get_wallet()
            .list_transfers(asset_id)?
            .into_iter()
            .map(|t| t.into())
            .collect())
    }

    fn list_unspents(
        &self,
        online: Option<Online>,
        settled_only: bool,
        skip_sync: bool,
    ) -> Result<Vec<Unspent>, RgbLibError> {
        Ok(self
            ._get_wallet()
            .list_unspents(online, settled_only, skip_sync)?
            .into_iter()
            .map(|u| u.into())
            .collect())
    }

    fn refresh(
        &self,
        online: Online,
        asset_id: Option<String>,
        filter: Vec<RefreshFilter>,
        skip_sync: bool,
    ) -> Result<HashMap<i32, RefreshedTransfer>, RgbLibError> {
        self._get_wallet()
            .refresh(online, asset_id, filter, skip_sync)
    }

    fn send(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        expiration_timestamp: Option<u64>,
        skip_sync: bool,
    ) -> Result<OperationResult, RgbLibError> {
        self._get_wallet().send(
            online,
            _convert_recipient_map(recipient_map),
            donation,
            fee_rate,
            min_confirmations,
            expiration_timestamp,
            skip_sync,
        )
    }

    fn send_begin(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        expiration_timestamp: Option<u64>,
    ) -> Result<SendBeginResult, RgbLibError> {
        self._get_wallet().send_begin(
            online,
            _convert_recipient_map(recipient_map),
            donation,
            fee_rate,
            min_confirmations,
            expiration_timestamp,
        )
    }

    fn send_end(
        &self,
        online: Online,
        signed_psbt: String,
        skip_sync: bool,
    ) -> Result<OperationResult, RgbLibError> {
        self._get_wallet().send_end(online, signed_psbt, skip_sync)
    }

    fn send_btc(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_btc(online, address, amount, fee_rate, skip_sync)
    }

    fn send_btc_begin(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_btc_begin(online, address, amount, fee_rate, skip_sync)
    }

    fn send_btc_end(
        &self,
        online: Online,
        signed_psbt: String,
        skip_sync: bool,
    ) -> Result<String, RgbLibError> {
        self._get_wallet()
            .send_btc_end(online, signed_psbt, skip_sync)
    }

    fn sync(&self, online: Online) -> Result<(), RgbLibError> {
        self._get_wallet().sync(online)
    }

    fn inspect_psbt(&self, psbt: String) -> Result<PsbtInspection, RgbLibError> {
        self._get_wallet().inspect_psbt(psbt)
    }

    fn inspect_rgb_transfer(
        &self,
        psbt: String,
        fascia_path: String,
        entropy: u64,
    ) -> Result<RgbInspection, RgbLibError> {
        Ok(self
            ._get_wallet()
            .inspect_rgb_transfer(psbt, fascia_path, entropy)?
            .into())
    }
}

fn _convert_recipient_map(
    recipient_map: HashMap<String, Vec<Recipient>>,
) -> HashMap<String, Vec<RgbLibRecipient>> {
    recipient_map
        .into_iter()
        .map(|(key, recipients)| {
            let new_recipients = recipients.into_iter().map(Into::into).collect();
            (key, new_recipients)
        })
        .collect()
}

uniffi::deps::static_assertions::assert_impl_all!(Wallet: Sync, Send);

struct MultisigWallet {
    wallet_mutex: Mutex<RgbLibMultisigWallet>,
}

impl MultisigWallet {
    fn new(wallet_data: WalletData, keys: MultisigKeys) -> Result<Self, RgbLibError> {
        Ok(MultisigWallet {
            wallet_mutex: Mutex::new(RgbLibMultisigWallet::new(wallet_data.try_into()?, keys)?),
        })
    }

    fn _get_wallet(&self) -> MutexGuard<'_, RgbLibMultisigWallet> {
        self.wallet_mutex.lock().expect("wallet")
    }

    fn get_wallet_data(&self) -> WalletData {
        self._get_wallet().get_wallet_data().into()
    }

    fn get_keys(&self) -> MultisigKeys {
        self._get_wallet().get_keys()
    }

    fn get_descriptors(&self) -> WalletDescriptors {
        self._get_wallet().get_descriptors()
    }

    fn get_wallet_dir(&self) -> String {
        self._get_wallet()
            .get_wallet_dir()
            .to_string_lossy()
            .to_string()
    }

    fn get_media_dir(&self) -> String {
        self._get_wallet()
            .get_media_dir()
            .to_string_lossy()
            .to_string()
    }

    fn backup(&self, backup_path: String, password: String) -> Result<(), RgbLibError> {
        self._get_wallet().backup(&backup_path, &password)
    }

    fn backup_info(&self) -> Result<bool, RgbLibError> {
        self._get_wallet().backup_info()
    }

    fn blind_receive(
        &self,
        online: Online,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, RgbLibError> {
        self._get_wallet().blind_receive(
            online,
            asset_id,
            assignment.into(),
            expiration_timestamp,
            transport_endpoints,
            min_confirmations,
        )
    }

    fn witness_receive(
        &self,
        online: Online,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, RgbLibError> {
        self._get_wallet().witness_receive(
            online,
            asset_id,
            assignment.into(),
            expiration_timestamp,
            transport_endpoints,
            min_confirmations,
        )
    }

    fn finalize_psbt(&self, signed_psbt: String) -> Result<String, RgbLibError> {
        self._get_wallet().finalize_psbt(signed_psbt, None)
    }

    fn create_utxos_init(
        &self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<InitOperationResult, RgbLibError> {
        self._get_wallet()
            .create_utxos_init(online, up_to, num, size, fee_rate, skip_sync)
    }

    fn delete_transfers(
        &self,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
    ) -> Result<bool, RgbLibError> {
        self._get_wallet()
            .delete_transfers(batch_transfer_idx, no_asset_only)
    }

    fn get_asset_balance(&self, asset_id: String) -> Result<Balance, RgbLibError> {
        self._get_wallet().get_asset_balance(asset_id)
    }

    fn get_btc_balance(
        &self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<BtcBalance, RgbLibError> {
        self._get_wallet().get_btc_balance(online, skip_sync)
    }

    fn get_asset_metadata(&self, asset_id: String) -> Result<Metadata, RgbLibError> {
        self._get_wallet().get_asset_metadata(asset_id)
    }

    fn get_fee_estimation(&self, online: Online, blocks: u16) -> Result<f64, RgbLibError> {
        self._get_wallet().get_fee_estimation(online, blocks)
    }

    fn go_online(
        &self,
        skip_consistency_check: bool,
        indexer_url: String,
        hub_url: String,
        hub_token: String,
    ) -> Result<Online, RgbLibError> {
        let mut wallet = self.wallet_mutex.lock().expect("wallet");
        wallet.go_online(skip_consistency_check, indexer_url, hub_url, hub_token)
    }

    fn hub_info(&self, online: Online) -> Result<HubInfo, RgbLibError> {
        self._get_wallet().hub_info(online)
    }

    fn inflate_init(
        &self,
        online: Online,
        asset_id: String,
        inflation_amounts: Vec<u64>,
        fee_rate: u64,
        min_confirmations: u8,
    ) -> Result<InitOperationResult, RgbLibError> {
        self._get_wallet().inflate_init(
            online,
            asset_id,
            inflation_amounts,
            fee_rate,
            min_confirmations,
        )
    }

    fn issue_asset_nia(
        &self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetNIA, RgbLibError> {
        self._get_wallet()
            .issue_asset_nia(online, ticker, name, precision, amounts)
    }

    fn issue_asset_uda(
        &self,
        online: Online,
        ticker: String,
        name: String,
        details: Option<String>,
        precision: u8,
        media_file_path: Option<String>,
        attachments_file_paths: Vec<String>,
    ) -> Result<AssetUDA, RgbLibError> {
        self._get_wallet().issue_asset_uda(
            online,
            ticker,
            name,
            details,
            precision,
            media_file_path,
            attachments_file_paths,
        )
    }

    fn issue_asset_cfa(
        &self,
        online: Online,
        name: String,
        details: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, RgbLibError> {
        self._get_wallet()
            .issue_asset_cfa(online, name, details, precision, amounts, file_path)
    }

    fn issue_asset_ifa(
        &self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
        inflation_amounts: Vec<u64>,
        reject_list_url: Option<String>,
    ) -> Result<AssetIFA, RgbLibError> {
        self._get_wallet().issue_asset_ifa(
            online,
            ticker,
            name,
            precision,
            amounts,
            inflation_amounts,
            reject_list_url,
        )
    }

    fn list_assets(&self, filter_asset_schemas: Vec<AssetSchema>) -> Result<Assets, RgbLibError> {
        self._get_wallet().list_assets(filter_asset_schemas)
    }

    fn list_transactions(
        &self,
        online: Option<Online>,
        skip_sync: bool,
    ) -> Result<Vec<Transaction>, RgbLibError> {
        self._get_wallet().list_transactions(online, skip_sync)
    }

    fn list_transfers(&self, asset_id: Option<String>) -> Result<Vec<Transfer>, RgbLibError> {
        Ok(self
            ._get_wallet()
            .list_transfers(asset_id)?
            .into_iter()
            .map(|t| t.into())
            .collect())
    }

    fn list_unspents(
        &self,
        online: Option<Online>,
        settled_only: bool,
        skip_sync: bool,
    ) -> Result<Vec<Unspent>, RgbLibError> {
        Ok(self
            ._get_wallet()
            .list_unspents(online, settled_only, skip_sync)?
            .into_iter()
            .map(|u| u.into())
            .collect())
    }

    fn refresh(
        &self,
        online: Online,
        asset_id: Option<String>,
        filter: Vec<RefreshFilter>,
        skip_sync: bool,
    ) -> Result<HashMap<i32, RefreshedTransfer>, RgbLibError> {
        self._get_wallet()
            .refresh(online, asset_id, filter, skip_sync)
    }

    fn send_init(
        &self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        expiration_timestamp: Option<u64>,
    ) -> Result<InitOperationResult, RgbLibError> {
        self._get_wallet().send_init(
            online,
            _convert_recipient_map(recipient_map),
            donation,
            fee_rate,
            min_confirmations,
            expiration_timestamp,
        )
    }

    fn send_btc_init(
        &self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<InitOperationResult, RgbLibError> {
        self._get_wallet()
            .send_btc_init(online, address, amount, fee_rate, skip_sync)
    }

    fn sync(&self, online: Online) -> Result<(), RgbLibError> {
        let mut wallet = self.wallet_mutex.lock().expect("wallet");
        wallet.sync(online)
    }

    fn get_address(&self, online: Online) -> Result<String, RgbLibError> {
        let mut wallet = self.wallet_mutex.lock().expect("wallet");
        wallet.get_address(online)
    }

    fn sync_with_hub(&self, online: Online) -> Result<Option<OperationInfo>, RgbLibError> {
        let mut wallet = self.wallet_mutex.lock().expect("wallet");
        Ok(wallet.sync_with_hub(online)?.map(|op_info| op_info.into()))
    }

    fn respond_to_operation(
        &self,
        online: Online,
        operation_idx: i32,
        respond_to_operation: RespondToOperation,
    ) -> Result<OperationInfo, RgbLibError> {
        let mut wallet = self.wallet_mutex.lock().expect("wallet");
        Ok(wallet
            .respond_to_operation(online, operation_idx, respond_to_operation.into())?
            .into())
    }

    fn inspect_psbt(&self, psbt: String) -> Result<PsbtInspection, RgbLibError> {
        self._get_wallet().inspect_psbt(psbt)
    }

    fn inspect_rgb_transfer(
        &self,
        psbt: String,
        fascia_path: String,
        entropy: u64,
    ) -> Result<RgbInspection, RgbLibError> {
        Ok(self
            ._get_wallet()
            .inspect_rgb_transfer(psbt, fascia_path, entropy)?
            .into())
    }
}

uniffi::deps::static_assertions::assert_impl_all!(MultisigWallet: Sync, Send);
