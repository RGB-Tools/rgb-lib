//! SeaORM Entity. Generated by sea-orm-codegen 0.8.0

use sea_orm::entity::prelude::*;

#[derive(Copy, Clone, Default, Debug, DeriveEntity)]
pub struct Entity;

impl EntityName for Entity {
    fn table_name(&self) -> &str {
        "asset_rgb21"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveModel, DeriveActiveModel)]
pub struct Model {
    pub idx: i64,
    pub asset_id: String,
    pub name: String,
    pub precision: u8,
    pub description: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveColumn)]
pub enum Column {
    Idx,
    AssetId,
    Name,
    Precision,
    Description,
    ParentId,
}

#[derive(Copy, Clone, Debug, EnumIter, DerivePrimaryKey)]
pub enum PrimaryKey {
    Idx,
}

impl PrimaryKeyTrait for PrimaryKey {
    type ValueType = i64;
    fn auto_increment() -> bool {
        true
    }
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    AssetTransfer,
}

impl ColumnTrait for Column {
    type EntityName = Entity;
    fn def(&self) -> ColumnDef {
        match self {
            Self::Idx => ColumnType::BigInteger.def(),
            Self::AssetId => ColumnType::String(None).def().unique(),
            Self::Name => ColumnType::String(None).def(),
            Self::Precision => ColumnType::SmallInteger.def(),
            Self::Description => ColumnType::String(None).def().null(),
            Self::ParentId => ColumnType::String(None).def().null(),
        }
    }
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::AssetTransfer => Entity::has_many(super::asset_transfer::Entity).into(),
        }
    }
}

impl Related<super::asset_transfer::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AssetTransfer.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
