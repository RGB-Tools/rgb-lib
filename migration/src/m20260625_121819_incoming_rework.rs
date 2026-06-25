use sea_orm_migration::{prelude::*, sea_orm::ConnectionTrait};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // move `incoming` from the transfer table up to the batch_transfer table: all transfers
        // belonging to the same batch transfer always share the same `incoming` value, so the flag
        // is really a property of the batch transfer

        // add the new column to the batch_transfer table
        manager
            .alter_table(
                Table::alter()
                    .table(BatchTransfer::Table)
                    .add_column(
                        ColumnDef::new(BatchTransfer::Incoming)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        // backfill from any child transfer
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE batch_transfer \
                 SET incoming = COALESCE( \
                     ( \
                         SELECT t.incoming \
                         FROM transfer t \
                         JOIN asset_transfer a ON t.asset_transfer_idx = a.idx \
                         WHERE a.batch_transfer_idx = batch_transfer.idx \
                         LIMIT 1 \
                     ), \
                     true \
                 )",
            )
            .await?;

        // drop the old column from the transfer table
        manager
            .alter_table(
                Table::alter()
                    .table(Transfer::Table)
                    .drop_column(Transfer::Incoming)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // re-add the column on the transfer table
        manager
            .alter_table(
                Table::alter()
                    .table(Transfer::Table)
                    .add_column(
                        ColumnDef::new(Transfer::Incoming)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        // backfill each transfer from its batch transfer
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE transfer \
                 SET incoming = ( \
                     SELECT b.incoming \
                     FROM batch_transfer b \
                     JOIN asset_transfer a ON a.batch_transfer_idx = b.idx \
                     WHERE a.idx = transfer.asset_transfer_idx \
                     LIMIT 1 \
                 )",
            )
            .await?;

        // drop the column from the batch_transfer table
        manager
            .alter_table(
                Table::alter()
                    .table(BatchTransfer::Table)
                    .drop_column(BatchTransfer::Incoming)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum BatchTransfer {
    Table,
    Incoming,
}

#[derive(DeriveIden)]
enum Transfer {
    Table,
    Incoming,
}
