//! RGB storage tables, mirroring the reference rgb-ops SQLite schema - except for its foreign
//! keys, which are deliberately not declared here.
//!
//! Every `REFERENCES` in that schema is `DEFERRABLE INITIALLY DEFERRED`, which sea-query cannot
//! express, and sqlx turns `PRAGMA foreign_keys` on by default: a foreign key declared through
//! the migration builder would therefore be checked per statement rather than at commit. That
//! would reject every write, since `Stock` writes children before their parents - the witness
//! ord before its witness, the bundle-witness/op-bundle/op-input rows before their bundle - and
//! SQLite has no `ALTER TABLE ADD CONSTRAINT`, so getting the deferred form would mean building
//! these tables from raw DDL instead.
//!
//! What the foreign keys would protect is held elsewhere: the RGB data is written through the
//! same transaction as the rest of the wallet data, which commits whole or rolls back.
//!
//! Every other constraint of that schema is kept. The `CHECK`s are row-local, so the write
//! order above never trips them, and they are what keeps a row self-consistent.
//!
//! The unique keys and indexes do the rest: one row per schema, contract, bundle, witness,
//! seal, op and state entry.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RgbSchema::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbSchema::Idx))
                    .col(binary(RgbSchema::SchemaId).unique_key())
                    .col(binary(RgbSchema::SchemaBlob))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbGenesis::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbGenesis::Idx))
                    .col(binary(RgbGenesis::ContractId).unique_key())
                    .col(binary(RgbGenesis::SchemaId))
                    .col(binary(RgbGenesis::GenesisBlob))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbBundle::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbBundle::Idx))
                    .col(binary(RgbBundle::BundleId).unique_key())
                    .col(binary(RgbBundle::BundleBlob))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbWitness::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbWitness::Idx))
                    .col(binary(RgbWitness::Txid).unique_key())
                    .col(binary(RgbWitness::TxBlob))
                    .col(binary(RgbWitness::MpcMerkleBlockBlob))
                    .col(binary(RgbWitness::DbcProofBlob))
                    .col(binary_null(RgbWitness::SpvProofBlob))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbAluvmLib::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbAluvmLib::Idx))
                    .col(binary(RgbAluvmLib::AluvmLibId).unique_key())
                    .col(binary(RgbAluvmLib::AluvmLibBlob))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbTypeLib::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbTypeLib::Idx))
                    .col(binary(RgbTypeLib::TypeLibId).unique_key())
                    .col(binary(RgbTypeLib::TypeLibBlob))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(RgbSchemaTypeLib::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbSchemaTypeLib::Idx))
                    .col(binary(RgbSchemaTypeLib::SchemaId))
                    .col(binary(RgbSchemaTypeLib::TypeLibId))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_schema_type_lib-unique")
                    .table(RgbSchemaTypeLib::Table)
                    .col(RgbSchemaTypeLib::SchemaId)
                    .col(RgbSchemaTypeLib::TypeLibId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbSecretSeal::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbSecretSeal::Idx))
                    .col(binary(RgbSecretSeal::SecretSeal).unique_key())
                    .col(binary(RgbSecretSeal::GraphSealBlob))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbWitnessOrd::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbWitnessOrd::Idx))
                    .col(binary(RgbWitnessOrd::Txid).unique_key())
                    .col(tiny_unsigned(RgbWitnessOrd::Status))
                    .col(big_integer_null(RgbWitnessOrd::Height))
                    .col(big_integer_null(RgbWitnessOrd::Timestamp))
                    .col(tiny_unsigned_null(RgbWitnessOrd::Layer1))
                    // status 0 is `WitnessOrd::Mined`, the only variant carrying a position
                    .check(
                        Expr::col(RgbWitnessOrd::Status)
                            .eq(0)
                            .eq(Expr::col(RgbWitnessOrd::Height).is_not_null())
                            .and(
                                Expr::col(RgbWitnessOrd::Status)
                                    .eq(0)
                                    .eq(Expr::col(RgbWitnessOrd::Timestamp).is_not_null()),
                            )
                            .and(
                                Expr::col(RgbWitnessOrd::Status)
                                    .eq(0)
                                    .eq(Expr::col(RgbWitnessOrd::Layer1).is_not_null()),
                            ),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_witness_ord-status-height")
                    .table(RgbWitnessOrd::Table)
                    .col(RgbWitnessOrd::Status)
                    .col(RgbWitnessOrd::Height)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbInvalidOp::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbInvalidOp::Idx))
                    .col(binary(RgbInvalidOp::Opid).unique_key())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbGlobalState::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbGlobalState::Idx))
                    .col(binary(RgbGlobalState::ContractId))
                    .col(integer(RgbGlobalState::TypeId))
                    .col(binary(RgbGlobalState::Opid))
                    .col(integer(RgbGlobalState::OutIndex))
                    .col(string(RgbGlobalState::Nonce))
                    .col(binary_null(RgbGlobalState::BundleId))
                    .col(integer_null(RgbGlobalState::TransitionType))
                    .col(binary(RgbGlobalState::ValueBlob))
                    .check(
                        Expr::col(RgbGlobalState::BundleId)
                            .is_null()
                            .eq(Expr::col(RgbGlobalState::TransitionType).is_null()),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_global_state-unique")
                    .table(RgbGlobalState::Table)
                    .col(RgbGlobalState::ContractId)
                    .col(RgbGlobalState::TypeId)
                    .col(RgbGlobalState::Opid)
                    .col(RgbGlobalState::OutIndex)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbAssignment::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbAssignment::Idx))
                    .col(binary(RgbAssignment::ContractId))
                    .col(
                        string(RgbAssignment::StateType)
                            .check(Expr::col(RgbAssignment::StateType).is_in(["F", "S", "R"])),
                    )
                    .col(integer(RgbAssignment::TypeId))
                    .col(binary(RgbAssignment::Opid))
                    .col(integer(RgbAssignment::OutputNo))
                    .col(binary_null(RgbAssignment::SealTxid))
                    .col(big_integer(RgbAssignment::SealVout))
                    .col(binary_null(RgbAssignment::BundleId))
                    .col(binary(RgbAssignment::ValueBlob))
                    .check(
                        Expr::col(RgbAssignment::SealTxid)
                            .is_not_null()
                            .or(Expr::col(RgbAssignment::BundleId).is_not_null()),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_assignment-unique")
                    .table(RgbAssignment::Table)
                    .col(RgbAssignment::ContractId)
                    .col(RgbAssignment::Opid)
                    .col(RgbAssignment::TypeId)
                    .col(RgbAssignment::OutputNo)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_assignment-lookup")
                    .table(RgbAssignment::Table)
                    .col(RgbAssignment::ContractId)
                    .col(RgbAssignment::StateType)
                    .col(RgbAssignment::TypeId)
                    .col(RgbAssignment::SealTxid)
                    .col(RgbAssignment::SealVout)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_assignment-contract-seal")
                    .table(RgbAssignment::Table)
                    .col(RgbAssignment::ContractId)
                    .col(RgbAssignment::SealTxid)
                    .col(RgbAssignment::SealVout)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_assignment-seal")
                    .table(RgbAssignment::Table)
                    .col(RgbAssignment::SealTxid)
                    .col(RgbAssignment::SealVout)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_assignment-witness-seal")
                    .table(RgbAssignment::Table)
                    .col(RgbAssignment::BundleId)
                    .col(RgbAssignment::SealVout)
                    .and_where(Expr::col(RgbAssignment::SealTxid).is_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbBundleContract::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbBundleContract::Idx))
                    .col(binary(RgbBundleContract::BundleId).unique_key())
                    .col(binary(RgbBundleContract::ContractId))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbBundleWitness::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbBundleWitness::Idx))
                    .col(binary(RgbBundleWitness::BundleId))
                    .col(binary(RgbBundleWitness::Txid))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_bundle_witness-unique")
                    .table(RgbBundleWitness::Table)
                    .col(RgbBundleWitness::BundleId)
                    .col(RgbBundleWitness::Txid)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_bundle_witness-txid")
                    .table(RgbBundleWitness::Table)
                    .col(RgbBundleWitness::Txid)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbOpBundle::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbOpBundle::Idx))
                    .col(binary(RgbOpBundle::Opid).unique_key())
                    .col(binary(RgbOpBundle::BundleId))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_op_bundle-bundle")
                    .table(RgbOpBundle::Table)
                    .col(RgbOpBundle::BundleId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbOpInput::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbOpInput::Idx))
                    .col(binary(RgbOpInput::Opid))
                    .col(integer(RgbOpInput::TypeId))
                    .col(integer(RgbOpInput::OutputNo))
                    .col(binary(RgbOpInput::ChildOpid))
                    .col(binary(RgbOpInput::ChildBundleId))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_op_input-unique")
                    .table(RgbOpInput::Table)
                    .col(RgbOpInput::Opid)
                    .col(RgbOpInput::TypeId)
                    .col(RgbOpInput::OutputNo)
                    .col(RgbOpInput::ChildOpid)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_op_input-child_opid")
                    .table(RgbOpInput::Table)
                    .col(RgbOpInput::ChildOpid)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbOutpointOpout::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbOutpointOpout::Idx))
                    .col(binary(RgbOutpointOpout::ContractId))
                    .col(binary(RgbOutpointOpout::Txid))
                    .col(big_integer(RgbOutpointOpout::Vout))
                    .col(binary(RgbOutpointOpout::Opout))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_outpoint_opout-unique")
                    .table(RgbOutpointOpout::Table)
                    .col(RgbOutpointOpout::ContractId)
                    .col(RgbOutpointOpout::Txid)
                    .col(RgbOutpointOpout::Vout)
                    .col(RgbOutpointOpout::Opout)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_outpoint_opout-outpoint")
                    .table(RgbOutpointOpout::Table)
                    .col(RgbOutpointOpout::Txid)
                    .col(RgbOutpointOpout::Vout)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RgbSecretSealOpout::Table)
                    .if_not_exists()
                    .col(pk_auto(RgbSecretSealOpout::Idx))
                    .col(binary(RgbSecretSealOpout::ContractId))
                    .col(binary(RgbSecretSealOpout::SecretSeal))
                    .col(binary(RgbSecretSealOpout::Opout))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-rgb_secret_seal_opout-unique")
                    .table(RgbSecretSealOpout::Table)
                    .col(RgbSecretSealOpout::ContractId)
                    .col(RgbSecretSealOpout::SecretSeal)
                    .col(RgbSecretSealOpout::Opout)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            RgbSecretSealOpout::Table.into_iden(),
            RgbOutpointOpout::Table.into_iden(),
            RgbOpInput::Table.into_iden(),
            RgbOpBundle::Table.into_iden(),
            RgbBundleWitness::Table.into_iden(),
            RgbBundleContract::Table.into_iden(),
            RgbAssignment::Table.into_iden(),
            RgbGlobalState::Table.into_iden(),
            RgbInvalidOp::Table.into_iden(),
            RgbWitnessOrd::Table.into_iden(),
            RgbSecretSeal::Table.into_iden(),
            RgbSchemaTypeLib::Table.into_iden(),
            RgbTypeLib::Table.into_iden(),
            RgbAluvmLib::Table.into_iden(),
            RgbWitness::Table.into_iden(),
            RgbBundle::Table.into_iden(),
            RgbGenesis::Table.into_iden(),
            RgbSchema::Table.into_iden(),
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum RgbSchema {
    Table,
    Idx,
    SchemaId,
    SchemaBlob,
}

#[derive(DeriveIden)]
enum RgbGenesis {
    Table,
    Idx,
    ContractId,
    SchemaId,
    GenesisBlob,
}

#[derive(DeriveIden)]
enum RgbBundle {
    Table,
    Idx,
    BundleId,
    BundleBlob,
}

#[derive(DeriveIden)]
enum RgbWitness {
    Table,
    Idx,
    Txid,
    TxBlob,
    MpcMerkleBlockBlob,
    DbcProofBlob,
    SpvProofBlob,
}

#[derive(DeriveIden)]
enum RgbAluvmLib {
    Table,
    Idx,
    AluvmLibId,
    AluvmLibBlob,
}

#[derive(DeriveIden)]
enum RgbTypeLib {
    Table,
    Idx,
    TypeLibId,
    TypeLibBlob,
}

#[derive(DeriveIden)]
enum RgbSchemaTypeLib {
    Table,
    Idx,
    SchemaId,
    TypeLibId,
}

#[derive(DeriveIden)]
enum RgbSecretSeal {
    Table,
    Idx,
    SecretSeal,
    GraphSealBlob,
}

#[derive(DeriveIden)]
enum RgbWitnessOrd {
    Table,
    Idx,
    Txid,
    Status,
    Height,
    Timestamp,
    Layer1,
}

#[derive(DeriveIden)]
enum RgbInvalidOp {
    Table,
    Idx,
    Opid,
}

#[derive(DeriveIden)]
enum RgbGlobalState {
    Table,
    Idx,
    ContractId,
    TypeId,
    Opid,
    OutIndex,
    Nonce,
    BundleId,
    TransitionType,
    ValueBlob,
}

#[derive(DeriveIden)]
enum RgbAssignment {
    Table,
    Idx,
    ContractId,
    StateType,
    TypeId,
    Opid,
    OutputNo,
    SealTxid,
    SealVout,
    BundleId,
    ValueBlob,
}

#[derive(DeriveIden)]
enum RgbBundleContract {
    Table,
    Idx,
    BundleId,
    ContractId,
}

#[derive(DeriveIden)]
enum RgbBundleWitness {
    Table,
    Idx,
    BundleId,
    Txid,
}

#[derive(DeriveIden)]
enum RgbOpBundle {
    Table,
    Idx,
    Opid,
    BundleId,
}

#[derive(DeriveIden)]
enum RgbOpInput {
    Table,
    Idx,
    Opid,
    TypeId,
    OutputNo,
    ChildOpid,
    ChildBundleId,
}

#[derive(DeriveIden)]
enum RgbOutpointOpout {
    Table,
    Idx,
    ContractId,
    Txid,
    Vout,
    Opout,
}

#[derive(DeriveIden)]
enum RgbSecretSealOpout {
    Table,
    Idx,
    ContractId,
    SecretSeal,
    Opout,
}
