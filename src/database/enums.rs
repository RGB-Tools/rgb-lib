use super::*;

/// The schema of an asset.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, EnumIter, DeriveActiveEnum,
)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum AssetSchema {
    /// NIA schema
    #[sea_orm(num_value = 1)]
    Nia = 1,
    /// UDA schema
    #[sea_orm(num_value = 2)]
    Uda = 2,
    /// CFA schema
    #[sea_orm(num_value = 3)]
    Cfa = 3,
}

impl fmt::Display for AssetSchema {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl AssetSchema {
    pub(crate) const VALUES: [Self; NUM_KNOWN_SCHEMAS] = [Self::Nia, Self::Uda, Self::Cfa];

    /// Get the AssetSchema given a schema ID.
    pub fn from_schema_id(schema_id: String) -> Result<AssetSchema, Error> {
        Ok(match &schema_id[..] {
            SCHEMA_ID_NIA => AssetSchema::Nia,
            SCHEMA_ID_UDA => AssetSchema::Uda,
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

/// The type of an RGB recipient
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

/// The type of an RGB transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Deserialize, Serialize)]
#[sea_orm(rs_type = "u16", db_type = "Integer")]
pub enum TransportType {
    /// HTTP(s) JSON-RPC ([specification](https://github.com/RGB-Tools/rgb-http-json-rpc))
    #[sea_orm(num_value = 1)]
    JsonRpc = 1,
}

/// The status of a [`crate::wallet::Transfer`].
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
    /// Waiting for the transfer transaction to reach the required number of confirmations
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
