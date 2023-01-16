use super::m20220810_132253_create_coloring::Coloring;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const INDEX_NAME: &str = "idx-coloring-asset_transfer_idx-txo_idx";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                sea_query::Index::create()
                    .name(INDEX_NAME)
                    .table(Coloring::Table)
                    .col(Coloring::AssetTransferIdx)
                    .col(Coloring::TxoIdx)
                    .unique()
                    .clone(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                sea_query::Index::drop()
                    .name(INDEX_NAME)
                    .table(Coloring::Table)
                    .to_owned(),
            )
            .await
    }
}
