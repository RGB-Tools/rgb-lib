use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220810_131915_create_asset_rgb20"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AssetRgb20::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AssetRgb20::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AssetRgb20::AssetId)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(AssetRgb20::Ticker).string().not_null())
                    .col(ColumnDef::new(AssetRgb20::Name).string().not_null())
                    .col(
                        ColumnDef::new(AssetRgb20::Precision)
                            .small_unsigned()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AssetRgb20::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum AssetRgb20 {
    Table,
    Idx,
    AssetId,
    Ticker,
    Name,
    Precision,
}
