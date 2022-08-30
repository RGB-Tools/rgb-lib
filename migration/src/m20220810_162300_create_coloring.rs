use super::m20220810_130049_create_txo::Txo;
use super::m20220810_132256_create_transfer::Transfer;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220810_162300_create_coloring"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Coloring::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Coloring::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Coloring::TxoIdx).big_integer().not_null())
                    .col(
                        ColumnDef::new(Coloring::TransferIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Coloring::ColoringType)
                            .tiny_unsigned()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Coloring::Amount).string().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-coloring-transfer")
                            .from(Coloring::Table, Coloring::TransferIdx)
                            .to(Transfer::Table, Transfer::Idx)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Restrict),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-coloring-txo")
                            .from(Coloring::Table, Coloring::TxoIdx)
                            .to(Txo::Table, Txo::Idx)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Coloring::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum Coloring {
    Table,
    Idx,
    TxoIdx,
    TransferIdx,
    ColoringType,
    Amount,
}
