use super::*;

/// The schema of an asset.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, EnumIter, DeriveActiveEnum,
)]
#[sea_orm(rs_type = "u8", db_type = "TinyUnsigned")]
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

    pub(crate) fn types(&self) -> TypeSystem {
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

#[derive(Debug, Copy, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "u8", db_type = "TinyUnsigned")]
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

/// The type of an RGB recipient.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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

impl rgb_lib_migration::ValueType for RecipientTypeFull {
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Deserialize, Serialize)]
#[sea_orm(rs_type = "u8", db_type = "TinyUnsigned")]
pub enum TransportType {
    /// HTTP(s) JSON-RPC ([specification](https://github.com/RGB-Tools/rgb-http-json-rpc))
    #[sea_orm(num_value = 1)]
    JsonRpc = 1,
}

/// The status of a [`crate::wallet::Transfer`].
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    EnumIter,
    DeriveActiveEnum,
    Deserialize,
    Serialize,
)]
#[sea_orm(rs_type = "u8", db_type = "TinyUnsigned")]
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
    /// Transfer has been initiated (PSBT prepared) but not yet finalized
    #[sea_orm(num_value = 5)]
    Initiated = 5,
}

impl TransferStatus {
    pub(crate) fn failed(&self) -> bool {
        self == &TransferStatus::Failed
    }

    pub(crate) fn initiated(&self) -> bool {
        self == &TransferStatus::Initiated
    }

    pub(crate) fn pending(&self) -> bool {
        [
            TransferStatus::Initiated,
            TransferStatus::WaitingCounterparty,
            TransferStatus::WaitingConfirmations,
        ]
        .contains(self)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn waiting(&self) -> bool {
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

#[derive(Debug, Copy, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "u8", db_type = "TinyUnsigned")]
pub enum WalletTransactionType {
    #[sea_orm(num_value = 1)]
    CreateUtxos = 1,
    #[sea_orm(num_value = 2)]
    Drain = 2,
}

/// An RGB assignment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum Assignment {
    /// Fungible value in RGB units (not considering precision)
    Fungible(u64),
    /// Non-fungible value
    NonFungible,
    /// Inflation right
    InflationRight(u64),
    /// Any assignment
    Any,
}

impl Assignment {
    pub(crate) fn from_opout_and_state(opout: Opout, state: &AllocatedState) -> Self {
        match state {
            AllocatedState::Amount(amt) if opout.ty == OS_ASSET => Self::Fungible(amt.as_u64()),
            AllocatedState::Amount(amt) if opout.ty == OS_INFLATION => {
                Self::InflationRight(amt.as_u64())
            }
            AllocatedState::Data(_) => Self::NonFungible,
            _ => unreachable!(),
        }
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn add_to_assignments(&self, assignments: &mut AssignmentsCollection) {
        match self {
            Self::Fungible(amt) => assignments.fungible += amt,
            Self::NonFungible => assignments.non_fungible = true,
            Self::InflationRight(amt) => assignments.inflation += amt,
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

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn inflation_amount(&self) -> u64 {
        if let Self::InflationRight(amt) = self {
            *amt
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

impl rgb_lib_migration::ValueType for Assignment {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_id() {
        // roundtrip
        for asset_schema in AssetSchema::VALUES {
            let schema_id = asset_schema.into();
            let asset_schema_from_schema_id = AssetSchema::from_schema_id(schema_id).unwrap();
            assert_eq!(asset_schema, asset_schema_from_schema_id);
        }

        // unknown
        let err = AssetSchema::try_from(s!("unknown")).unwrap_err();
        assert_matches!(err, Error::UnknownRgbSchema { schema_id: _ });
        let err = AssetSchema::from_schema_id(SchemaId::strict_dumb()).unwrap_err();
        assert_matches!(err, Error::UnknownRgbSchema { schema_id: _ });

        // display
        assert_eq!(AssetSchema::Nia.to_string(), "Nia");
        assert_eq!(AssetSchema::Uda.to_string(), "Uda");
        assert_eq!(AssetSchema::Cfa.to_string(), "Cfa");
        assert_eq!(AssetSchema::Ifa.to_string(), "Ifa");
    }

    #[test]
    fn test_coloring_type_into_active_value() {
        let variants = [
            ColoringType::Receive,
            ColoringType::Issue,
            ColoringType::Input,
            ColoringType::Change,
        ];
        for variant in variants {
            let active_value = IntoActiveValue::into_active_value(variant);
            assert_eq!(active_value, ActiveValue::Set(variant));
        }
    }

    #[test]
    fn test_recipient_type_full() {
        // roundtrip
        let blind = RecipientTypeFull::Blind {
            unblinded_utxo: Outpoint {
                txid: s!("0000000000000000000000000000000000000000000000000000000000000001"),
                vout: 0,
            },
        };
        let witness = RecipientTypeFull::Witness { vout: Some(1) };
        let witness_none = RecipientTypeFull::Witness { vout: None };
        for recipient in [blind, witness, witness_none] {
            let value: Value = recipient.clone().into();
            let recovered = RecipientTypeFull::try_from(value).unwrap();
            assert_eq!(recipient, recovered);
        }

        // try from value: not JSON
        let value = Value::Int(Some(42));
        let err = RecipientTypeFull::try_from(value).unwrap_err();
        assert!(matches!(err, sea_orm::DbErr::Type(_)));

        // try from value: invalid JSON
        let bad_json = serde_json::json!({"bad": "data"});
        let value = Value::Json(Some(Box::new(bad_json)));
        let err = RecipientTypeFull::try_from(value).unwrap_err();
        assert!(matches!(err, sea_orm::DbErr::Custom(_)));

        // value type
        assert_eq!(
            <RecipientTypeFull as rgb_lib_migration::ValueType>::type_name(),
            "json"
        );
        assert_eq!(
            <RecipientTypeFull as rgb_lib_migration::ValueType>::column_type(),
            ColumnType::Json
        );
        assert_eq!(
            <RecipientTypeFull as rgb_lib_migration::ValueType>::array_type(),
            ArrayType::Json
        );

        // value type try from
        let blind = RecipientTypeFull::Blind {
            unblinded_utxo: Outpoint {
                txid: s!("0000000000000000000000000000000000000000000000000000000000000001"),
                vout: 0,
            },
        };
        let json_val = serde_json::to_value(&blind).unwrap();
        let value = Value::Json(Some(Box::new(json_val)));
        let recovered =
            <RecipientTypeFull as rgb_lib_migration::ValueType>::try_from(value).unwrap();
        assert_eq!(blind, recovered);

        // value type try from: not JSON
        let value = Value::Int(Some(42));
        let err = <RecipientTypeFull as rgb_lib_migration::ValueType>::try_from(value);
        assert!(err.is_err());

        // value type try from: invalid JSON
        let bad_json = serde_json::json!({"bad": "data"});
        let value = Value::Json(Some(Box::new(bad_json)));
        let err = <RecipientTypeFull as rgb_lib_migration::ValueType>::try_from(value);
        assert!(err.is_err());

        // nullable
        let null_val = <RecipientTypeFull as Nullable>::null();
        assert_eq!(null_val, Value::Json(None));
    }

    #[test]
    fn test_assignment() {
        // roundtrip
        let assignments = [
            Assignment::Fungible(100),
            Assignment::NonFungible,
            Assignment::InflationRight(500),
            Assignment::Any,
        ];
        for assignment in assignments {
            // test From<Assignment> for Value (already covered, but needed for roundtrip)
            let value: Value = assignment.clone().into();
            // test TryFrom<Value> for Assignment
            let recovered = Assignment::try_from(value).unwrap();
            assert_eq!(assignment, recovered);
        }

        // try from value: not JSON
        let value = Value::Int(Some(42));
        let err = Assignment::try_from(value).unwrap_err();
        assert!(matches!(err, sea_orm::DbErr::Type(_)));

        // try from value: invalid JSON
        let bad_json = serde_json::json!({"InvalidVariant": 123});
        let value = Value::Json(Some(Box::new(bad_json)));
        let err = Assignment::try_from(value).unwrap_err();
        assert!(matches!(err, sea_orm::DbErr::Custom(_)));

        // value type
        assert_eq!(
            <Assignment as rgb_lib_migration::ValueType>::type_name(),
            "json"
        );
        assert_eq!(
            <Assignment as rgb_lib_migration::ValueType>::column_type(),
            ColumnType::Json
        );
        assert_eq!(
            <Assignment as rgb_lib_migration::ValueType>::array_type(),
            ArrayType::Json
        );

        // value type try from
        let assignment = Assignment::Fungible(42);
        let json_val = serde_json::to_value(&assignment).unwrap();
        let value = Value::Json(Some(Box::new(json_val)));
        let recovered = <Assignment as rgb_lib_migration::ValueType>::try_from(value).unwrap();
        assert_eq!(assignment, recovered);

        // value type try from: not JSON
        let value = Value::Int(Some(42));
        let err = <Assignment as rgb_lib_migration::ValueType>::try_from(value);
        assert!(err.is_err());

        // value type try from: invalid JSON
        let bad_json = serde_json::json!({"InvalidVariant": 123});
        let value = Value::Json(Some(Box::new(bad_json)));
        let err = <Assignment as rgb_lib_migration::ValueType>::try_from(value);
        assert!(err.is_err());

        // nullable
        let null_val = <Assignment as Nullable>::null();
        assert_eq!(null_val, Value::Json(None));

        // main amount
        assert_eq!(Assignment::Fungible(100).main_amount(), 100);
        assert_eq!(Assignment::Fungible(0).main_amount(), 0);
        assert_eq!(Assignment::NonFungible.main_amount(), 1);
        assert_eq!(Assignment::InflationRight(500).main_amount(), 0);
        assert_eq!(Assignment::Any.main_amount(), 0);
    }

    #[test]
    fn test_transfer_status_methods() {
        assert!(TransferStatus::Failed.failed());
        assert!(!TransferStatus::Settled.failed());
        assert!(!TransferStatus::WaitingCounterparty.failed());
        assert!(!TransferStatus::WaitingConfirmations.failed());
        assert!(!TransferStatus::Initiated.failed());

        assert!(TransferStatus::Initiated.initiated());
        assert!(!TransferStatus::WaitingCounterparty.initiated());
        assert!(!TransferStatus::WaitingConfirmations.initiated());
        assert!(!TransferStatus::Settled.initiated());
        assert!(!TransferStatus::Failed.initiated());

        assert!(TransferStatus::Initiated.pending());
        assert!(TransferStatus::WaitingCounterparty.pending());
        assert!(TransferStatus::WaitingConfirmations.pending());
        assert!(!TransferStatus::Settled.pending());
        assert!(!TransferStatus::Failed.pending());

        assert!(TransferStatus::Settled.settled());
        assert!(!TransferStatus::Failed.settled());

        assert!(TransferStatus::WaitingConfirmations.waiting_confirmations());
        assert!(!TransferStatus::WaitingCounterparty.waiting_confirmations());

        assert!(TransferStatus::WaitingCounterparty.waiting_counterparty());
        assert!(!TransferStatus::WaitingConfirmations.waiting_counterparty());
    }
}
