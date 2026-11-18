use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-txo-pendingwitness")
                    .table(Txo::Table)
                    .col(Txo::PendingWitness)
                    .and_where(Expr::col(Txo::PendingWitness).eq(true))
                    .clone(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-coloring-txoidx")
                    .table(Coloring::Table)
                    .col(Coloring::TxoIdx)
                    .clone(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                sea_query::Index::drop()
                    .name("idx-coloring-txoidx")
                    .table(Coloring::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                sea_query::Index::drop()
                    .name("idx-txo-pendingwitness")
                    .table(Txo::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Txo {
    Table,
    PendingWitness,
}

#[derive(DeriveIden)]
enum Coloring {
    Table,
    TxoIdx,
}
