use super::m20220810_131915_create_asset_rgb20::AssetRgb20;
use super::m20220810_131920_create_asset_rgb21::AssetRgb21;
use super::m20220810_132240_create_batch_transfer::BatchTransfer;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220810_132250_create_asset_transfer"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AssetTransfer::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AssetTransfer::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AssetTransfer::UserDriven)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AssetTransfer::BatchTransferIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AssetTransfer::AssetRgb20Id).string())
                    .col(ColumnDef::new(AssetTransfer::AssetRgb21Id).string())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-assettransfer-batchtransfer")
                            .from(AssetTransfer::Table, AssetTransfer::BatchTransferIdx)
                            .to(BatchTransfer::Table, BatchTransfer::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-assettransfer-assetrgb20")
                            .from(AssetTransfer::Table, AssetTransfer::AssetRgb20Id)
                            .to(AssetRgb20::Table, AssetRgb20::AssetId)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-assettransfer-assetrgb21")
                            .from(AssetTransfer::Table, AssetTransfer::AssetRgb21Id)
                            .to(AssetRgb21::Table, AssetRgb21::AssetId)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AssetTransfer::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum AssetTransfer {
    Table,
    Idx,
    UserDriven,
    BatchTransferIdx,
    AssetRgb20Id,
    AssetRgb21Id,
}
