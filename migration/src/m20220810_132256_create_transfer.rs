use super::m20220810_132250_create_asset_transfer::AssetTransfer;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220810_132256_create_transfer"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Transfer::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Transfer::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Transfer::AssetTransferIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Transfer::Amount).string().not_null())
                    .col(ColumnDef::new(Transfer::BlindedUtxo).string())
                    .col(ColumnDef::new(Transfer::BlindingSecret).string())
                    .col(ColumnDef::new(Transfer::Ack).boolean())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-transfer-assettransfer")
                            .from(Transfer::Table, Transfer::AssetTransferIdx)
                            .to(AssetTransfer::Table, AssetTransfer::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Transfer::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum Transfer {
    Table,
    Idx,
    AssetTransferIdx,
    Amount,
    BlindedUtxo,
    BlindingSecret,
    Ack,
}
