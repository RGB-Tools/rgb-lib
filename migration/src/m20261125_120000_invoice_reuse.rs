use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Invoice::Table)
                    .if_not_exists()
                    .col(pk_auto(Invoice::Idx))
                    .col(unsigned(Invoice::Quota))
                    .col(string_null(Invoice::AssetId))
                    .col(big_integer(Invoice::Deadline))
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Transfer::Table)
                    .add_column(ColumnDef::new(Transfer::InvoiceIdx).integer().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Transfer::Table)
                    .add_column(ColumnDef::new(Transfer::ReceiveDir).string().null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for col in [Transfer::InvoiceIdx, Transfer::ReceiveDir] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Transfer::Table)
                        .drop_column(col)
                        .to_owned(),
                )
                .await?;
        }
        manager
            .drop_table(Table::drop().table(Invoice::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Invoice {
    Table,
    Idx,
    Quota,
    AssetId,
    Deadline,
}

#[derive(DeriveIden)]
enum Transfer {
    Table,
    InvoiceIdx,
    ReceiveDir,
}
