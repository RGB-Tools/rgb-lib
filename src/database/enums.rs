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
    /// IFA schema
    #[sea_orm(num_value = 4)]
    Ifa = 4,
}

impl fmt::Display for AssetSchema {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl TryFrom<String> for AssetSchema {
    type Error = Error;

    fn try_from(schema_id: String) -> Result<Self, Self::Error> {
        Ok(match &schema_id[..] {
            SCHEMA_ID_NIA => AssetSchema::Nia,
            SCHEMA_ID_UDA => AssetSchema::Uda,
            SCHEMA_ID_CFA => AssetSchema::Cfa,
            SCHEMA_ID_IFA => AssetSchema::Ifa,
            _ => return Err(Error::UnknownRgbSchema { schema_id }),
        })
    }
}

impl TryFrom<SchemaId> for AssetSchema {
    type Error = Error;

    fn try_from(schema_id: SchemaId) -> Result<Self, Self::Error> {
        schema_id.to_string().try_into()
    }
}

impl AssetSchema {
    pub(crate) const VALUES: [Self; NUM_KNOWN_SCHEMAS] =
        [Self::Nia, Self::Uda, Self::Cfa, Self::Ifa];

    fn from_schema_id_str(schema_id: String) -> Result<Self, Error> {
        Ok(match &schema_id[..] {
            SCHEMA_ID_NIA => AssetSchema::Nia,
            SCHEMA_ID_UDA => AssetSchema::Uda,
            SCHEMA_ID_CFA => AssetSchema::Cfa,
            SCHEMA_ID_IFA => AssetSchema::Ifa,
            _ => return Err(Error::UnknownRgbSchema { schema_id }),
        })
    }

    /// Get [`AssetSchema`] from [`SchemaId`].
    pub fn from_schema_id(schema_id: SchemaId) -> Result<Self, Error> {
        Self::from_schema_id_str(schema_id.to_string())
    }

    pub(crate) fn get_from_contract_id(
        contract_id: ContractId,
        runtime: &RgbRuntime,
    ) -> Result<Self, Error> {
        let schema_id = runtime.genesis(contract_id)?.schema_id;
        Self::from_schema_id(schema_id)
    }

    fn schema(&self) -> Schema {
        match self {
            Self::Nia => NonInflatableAsset::schema(),
            Self::Uda => UniqueDigitalAsset::schema(),
            Self::Cfa => CollectibleFungibleAsset::schema(),
            Self::Ifa => InflatableFungibleAsset::schema(),
        }
    }

    fn scripts(&self) -> Scripts {
        match self {
            Self::Nia => NonInflatableAsset::scripts(),
            Self::Uda => UniqueDigitalAsset::scripts(),
            Self::Cfa => CollectibleFungibleAsset::scripts(),
            Self::Ifa => InflatableFungibleAsset::scripts(),
        }
    }

    fn types(&self) -> TypeSystem {
        match self {
            Self::Nia => NonInflatableAsset::types(),
            Self::Uda => UniqueDigitalAsset::types(),
            Self::Cfa => CollectibleFungibleAsset::types(),
            Self::Ifa => InflatableFungibleAsset::types(),
        }
    }

    pub(crate) fn import_kit(&self, runtime: &mut RgbRuntime) -> Result<(), Error> {
        let schema = self.schema();
        let lib = self.scripts();
        let types = self.types();
        let mut kit = Kit::default();
        kit.schemata.push(schema).unwrap();
        kit.scripts.extend(lib.into_values()).unwrap();
        kit.types = types;
        let valid_kit = kit.validate().map_err(|_| InternalError::Unexpected)?;
        runtime.import_kit(valid_kit)?;
        Ok(())
    }
}

impl From<AssetSchema> for SchemaId {
    fn from(asset_schema: AssetSchema) -> Self {
        match asset_schema {
            AssetSchema::Cfa => SchemaId::from_str(SCHEMA_ID_CFA).unwrap(),
            AssetSchema::Ifa => SchemaId::from_str(SCHEMA_ID_IFA).unwrap(),
            AssetSchema::Nia => SchemaId::from_str(SCHEMA_ID_NIA).unwrap(),
            AssetSchema::Uda => SchemaId::from_str(SCHEMA_ID_UDA).unwrap(),
        }
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
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum RecipientTypeFull {
    /// Receive via blinded UTXO
    Blind { unblinded_utxo: Outpoint },
    /// Receive via witness TX
    Witness { vout: Option<u32> },
}

impl From<RecipientTypeFull> for Value {
    fn from(value: RecipientTypeFull) -> Self {
        Value::Json(Some(Box::new(serde_json::to_value(value).unwrap())))
    }
}

impl TryFrom<Value> for RecipientTypeFull {
    type Error = sea_orm::DbErr;

    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Json(Some(json)) => {
                serde_json::from_value(*json).map_err(|e| sea_orm::DbErr::Custom(e.to_string()))
            }
            _ => Err(sea_orm::DbErr::Type("Expected JSON value".into())),
        }
    }
}

impl ValueType for RecipientTypeFull {
    fn type_name() -> String {
        "json".to_string()
    }

    fn column_type() -> ColumnType {
        ColumnType::Json
    }

    fn array_type() -> ArrayType {
        ArrayType::Json
    }

    fn try_from(v: Value) -> Result<Self, ValueTypeErr> {
        match v {
            Value::Json(Some(json)) => serde_json::from_value(*json).map_err(|_| ValueTypeErr),
            _ => Err(ValueTypeErr),
        }
    }
}

impl TryGetable for RecipientTypeFull {
    fn try_get_by<I: sea_orm::ColIdx>(res: &QueryResult, index: I) -> Result<Self, TryGetError> {
        let json_value_opt: Option<JsonValue> = res.try_get_by(index)?;
        match json_value_opt {
            Some(json_value) => serde_json::from_value(json_value)
                .map_err(|e| TryGetError::DbErr(DbErr::Type(e.to_string()))),
            None => Err(TryGetError::Null(
                "Null value for RecipientTypeFull".to_string(),
            )),
        }
    }
}

impl Nullable for RecipientTypeFull {
    fn null() -> Value {
        Value::Json(None)
    }
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

/// An RGB assignment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
pub enum Assignment {
    /// Fungible value in RGB units (not considering precision)
    Fungible(u64),
    /// Non-fungible value
    NonFungible,
    /// Inflation right
    InflationRight(u64),
    /// Replace right
    ReplaceRight,
    /// Any assignment
    Any,
}

impl Assignment {
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn add_to_assignments(&self, assignments: &mut AssignmentsCollection) {
        match self {
            Self::Fungible(amt) => assignments.fungible += amt,
            Self::NonFungible => assignments.non_fungible = true,
            Self::InflationRight(amt) => assignments.inflation += amt,
            Self::ReplaceRight => assignments.replace += 1,
            _ => unreachable!("when using this method we should know the assignment type"),
        }
    }

    pub(crate) fn main_amount(&self) -> u64 {
        if let Self::Fungible(amt) = self {
            *amt
        } else if let Self::NonFungible = self {
            1
        } else {
            0
        }
    }
}

impl From<Assignment> for Value {
    fn from(value: Assignment) -> Self {
        Value::Json(Some(Box::new(serde_json::to_value(value).unwrap())))
    }
}

impl TryFrom<Value> for Assignment {
    type Error = sea_orm::DbErr;

    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Json(Some(json)) => {
                serde_json::from_value(*json).map_err(|e| sea_orm::DbErr::Custom(e.to_string()))
            }
            _ => Err(sea_orm::DbErr::Type("Expected JSON value".into())),
        }
    }
}

impl ValueType for Assignment {
    fn type_name() -> String {
        "json".to_string()
    }

    fn column_type() -> ColumnType {
        ColumnType::Json
    }

    fn array_type() -> ArrayType {
        ArrayType::Json
    }

    fn try_from(v: Value) -> Result<Self, ValueTypeErr> {
        match v {
            Value::Json(Some(json)) => serde_json::from_value(*json).map_err(|_| ValueTypeErr),
            _ => Err(ValueTypeErr),
        }
    }
}

impl TryGetable for Assignment {
    fn try_get_by<I: sea_orm::ColIdx>(res: &QueryResult, index: I) -> Result<Self, TryGetError> {
        let json_value_opt: Option<JsonValue> = res.try_get_by(index)?;
        match json_value_opt {
            Some(json_value) => serde_json::from_value(json_value)
                .map_err(|e| TryGetError::DbErr(DbErr::Type(e.to_string()))),
            None => Err(TryGetError::Null("Null value for Assignment".to_string())),
        }
    }
}

impl Nullable for Assignment {
    fn null() -> Value {
        Value::Json(None)
    }
}
