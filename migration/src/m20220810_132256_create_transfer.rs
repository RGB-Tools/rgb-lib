use super::m20220810_131915_create_asset::Asset;
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
                        ColumnDef::new(Transfer::CreatedAt)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Transfer::UpdatedAt)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Transfer::Status).small_unsigned().not_null())
                    .col(ColumnDef::new(Transfer::UserDriven).boolean().not_null())
                    .col(ColumnDef::new(Transfer::AssetId).string())
                    .col(ColumnDef::new(Transfer::Txid).string())
                    .col(ColumnDef::new(Transfer::BlindedUtxo).string())
                    .col(ColumnDef::new(Transfer::BlindingSecret).string())
                    .col(ColumnDef::new(Transfer::Expiration).big_unsigned())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-asset-transfer")
                            .from(Transfer::Table, Transfer::AssetId)
                            .to(Asset::Table, Asset::AssetId)
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
    CreatedAt,
    UpdatedAt,
    Status,
    UserDriven,
    AssetId,
    Txid,
    BlindedUtxo,
    BlindingSecret,
    Expiration,
}
