//! `SeaORM` Entity, @generated by sea-orm-codegen 1.1.12

use sea_orm::entity::prelude::*;

use crate::database::enums::{Assignment, ColoringType};

#[derive(Copy, Clone, Default, Debug, DeriveEntity)]
pub struct Entity;

impl EntityName for Entity {
    fn table_name(&self) -> &str {
        "coloring"
    }
}

#[derive(Clone, Debug, PartialEq, DeriveModel, DeriveActiveModel, Eq)]
pub struct Model {
    pub idx: i32,
    pub txo_idx: i32,
    pub asset_transfer_idx: i32,
    pub r#type: ColoringType,
    pub assignment: Assignment,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveColumn)]
pub enum Column {
    Idx,
    TxoIdx,
    AssetTransferIdx,
    Type,
    Assignment,
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
    AssetTransfer,
    Txo,
}

impl ColumnTrait for Column {
    type EntityName = Entity;
    fn def(&self) -> ColumnDef {
        match self {
            Self::Idx => ColumnType::Integer.def(),
            Self::TxoIdx => ColumnType::Integer.def(),
            Self::AssetTransferIdx => ColumnType::Integer.def(),
            Self::Type => ColumnType::SmallInteger.def(),
            Self::Assignment => ColumnType::Json.def(),
        }
    }
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::AssetTransfer => Entity::belongs_to(super::asset_transfer::Entity)
                .from(Column::AssetTransferIdx)
                .to(super::asset_transfer::Column::Idx)
                .into(),
            Self::Txo => Entity::belongs_to(super::txo::Entity)
                .from(Column::TxoIdx)
                .to(super::txo::Column::Idx)
                .into(),
        }
    }
}

impl Related<super::asset_transfer::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AssetTransfer.def()
    }
}

impl Related<super::txo::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Txo.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
