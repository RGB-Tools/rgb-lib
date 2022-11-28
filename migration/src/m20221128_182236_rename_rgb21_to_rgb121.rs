use super::m20220810_131920_create_asset_rgb21::AssetRgb21;
use super::m20220810_132250_create_asset_transfer::AssetTransfer;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20221128_182236_rename_rgb21_to_rgb121"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AssetTransfer::Table)
                    .rename_column(Alias::new("asset_rgb21_id"), Alias::new("asset_rgb121_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .rename_table(
                Table::rename()
                    .table(AssetRgb21::Table, Alias::new("asset_rgb121"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AssetTransfer::Table)
                    .rename_column(Alias::new("asset_rgb121_id"), Alias::new("asset_rgb21_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .rename_table(
                Table::rename()
                    .table(AssetRgb21::Table, Alias::new("asset_rgb21"))
                    .to_owned(),
            )
            .await
    }
}
