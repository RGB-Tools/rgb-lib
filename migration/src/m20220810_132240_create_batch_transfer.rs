use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220810_132240_create_batch_transfer"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BatchTransfer::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BatchTransfer::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BatchTransfer::Txid).string())
                    .col(
                        ColumnDef::new(BatchTransfer::Status)
                            .small_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BatchTransfer::CreatedAt)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BatchTransfer::UpdatedAt)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(ColumnDef::new(BatchTransfer::Expiration).big_unsigned())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(BatchTransfer::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum BatchTransfer {
    Table,
    Idx,
    Txid,
    Status,
    CreatedAt,
    UpdatedAt,
    Expiration,
}
