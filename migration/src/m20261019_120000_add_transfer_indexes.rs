use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-assettransfer-batchtransferidx")
                    .table(AssetTransfer::Table)
                    .col(AssetTransfer::BatchTransferIdx)
                    .if_not_exists()
                    .clone(),
            )
            .await?;

        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-transfer-assettransferidx")
                    .table(Transfer::Table)
                    .col(Transfer::AssetTransferIdx)
                    .if_not_exists()
                    .clone(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                sea_query::Index::drop()
                    .name("idx-transfer-assettransferidx")
                    .table(Transfer::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                sea_query::Index::drop()
                    .name("idx-assettransfer-batchtransferidx")
                    .table(AssetTransfer::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum AssetTransfer {
    Table,
    BatchTransferIdx,
}

#[derive(DeriveIden)]
enum Transfer {
    Table,
    AssetTransferIdx,
}
