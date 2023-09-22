#![allow(missing_docs)]

use sea_orm::{ActiveValue, DeriveActiveEnum, EnumIter, IntoActiveValue};
use serde::{Deserialize, Serialize};

use crate::{
    wallet::{SCHEMA_ID_CFA, SCHEMA_ID_NIA},
    Error,
};

/// The schema of an asset
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, EnumIter, DeriveActiveEnum,
)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum AssetSchema {
    /// NIA schema
    #[sea_orm(num_value = 1)]
    Nia = 1,
    /// CFA schema
    #[sea_orm(num_value = 2)]
    Cfa = 2,
}

impl AssetSchema {
    /// Get the AssetSchema given a schema ID
    pub fn from_schema_id(schema_id: String) -> Result<AssetSchema, Error> {
        Ok(match &schema_id[..] {
            SCHEMA_ID_NIA => AssetSchema::Nia,
            SCHEMA_ID_CFA => AssetSchema::Cfa,
            _ => return Err(Error::UnknownRgbSchema { schema_id }),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum ColoringType {
    #[sea_orm(num_value = 1)]
    Receive = 1,
    #[sea_orm(num_value = 2)]
    Issue = 2,
    #[sea_orm(num_value = 3)]
    Input = 3,
    #[sea_orm(num_value = 4)]
    Change = 4,
}

impl IntoActiveValue<ColoringType> for ColoringType {
    fn into_active_value(self) -> ActiveValue<ColoringType> {
        ActiveValue::Set(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Deserialize, Serialize)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum RecipientType {
    /// Receive via blinded UTXO
    #[sea_orm(num_value = 1)]
    Blind = 1,
    /// Receive via witness TX
    #[sea_orm(num_value = 2)]
    Witness = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Deserialize, Serialize)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum TransportType {
    #[sea_orm(num_value = 1)]
    JsonRpc = 1,
}

/// The status of a [`crate::wallet::Transfer`]
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Ord,
    PartialOrd,
    EnumIter,
    DeriveActiveEnum,
    Deserialize,
    Serialize,
)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum TransferStatus {
    /// Waiting for the counterparty to take action
    #[sea_orm(num_value = 1)]
    WaitingCounterparty = 1,
    /// Waiting for the transfer transcation to be confirmed
    #[sea_orm(num_value = 2)]
    WaitingConfirmations = 2,
    /// Settled transfer, this status is final
    #[sea_orm(num_value = 3)]
    Settled = 3,
    /// Failed transfer, this status is final
    #[sea_orm(num_value = 4)]
    Failed = 4,
}

impl TransferStatus {
    pub(crate) fn failed(&self) -> bool {
        self == &TransferStatus::Failed
    }

    pub(crate) fn pending(&self) -> bool {
        [
            TransferStatus::WaitingCounterparty,
            TransferStatus::WaitingConfirmations,
        ]
        .contains(self)
    }

    pub(crate) fn settled(&self) -> bool {
        self == &TransferStatus::Settled
    }

    pub(crate) fn waiting_confirmations(&self) -> bool {
        self == &TransferStatus::WaitingConfirmations
    }

    pub(crate) fn waiting_counterparty(&self) -> bool {
        self == &TransferStatus::WaitingCounterparty
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum WalletTransactionType {
    #[sea_orm(num_value = 1)]
    CreateUtxos = 1,
    #[sea_orm(num_value = 2)]
    Drain = 2,
}
