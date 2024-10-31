//! `SeaORM` Entity, @generated by sea-orm-codegen 1.1.0

use sea_orm::entity::prelude::*;

#[derive(Copy, Clone, Default, Debug, DeriveEntity)]
pub struct Entity;

impl EntityName for Entity {
    fn table_name(&self) -> &str {
        "token"
    }
}

#[derive(Clone, Debug, PartialEq, DeriveModel, DeriveActiveModel, Eq)]
pub struct Model {
    pub idx: i32,
    pub asset_idx: i32,
    pub index: u32,
    pub ticker: Option<String>,
    pub name: Option<String>,
    pub details: Option<String>,
    pub embedded_media: bool,
    pub reserves: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveColumn)]
pub enum Column {
    Idx,
    AssetIdx,
    Index,
    Ticker,
    Name,
    Details,
    EmbeddedMedia,
    Reserves,
}

#[derive(Copy, Clone, Debug, EnumIter, DerivePrimaryKey)]
pub enum PrimaryKey {
    Idx,
}

impl PrimaryKeyTrait for PrimaryKey {
    type ValueType = i32;
    fn auto_increment() -> bool {
        true
    }
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Asset,
    TokenMedia,
}

impl ColumnTrait for Column {
    type EntityName = Entity;
    fn def(&self) -> ColumnDef {
        match self {
            Self::Idx => ColumnType::Integer.def(),
            Self::AssetIdx => ColumnType::Integer.def(),
            Self::Index => ColumnType::BigInteger.def(),
            Self::Ticker => ColumnType::String(StringLen::None).def().null(),
            Self::Name => ColumnType::String(StringLen::None).def().null(),
            Self::Details => ColumnType::String(StringLen::None).def().null(),
            Self::EmbeddedMedia => ColumnType::Boolean.def(),
            Self::Reserves => ColumnType::Boolean.def(),
        }
    }
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Asset => Entity::belongs_to(super::asset::Entity)
                .from(Column::AssetIdx)
                .to(super::asset::Column::Idx)
                .into(),
            Self::TokenMedia => Entity::has_many(super::token_media::Entity).into(),
        }
    }
}

impl Related<super::asset::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Asset.def()
    }
}

impl Related<super::token_media::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TokenMedia.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
