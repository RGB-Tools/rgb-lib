use sea_orm_migration::{prelude::*, schema::*, sea_orm::ConnectionTrait};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // reserved TXOs can now also be reserved for a batch transfer: rename `reserved_for` to
        // `reserved_for_vanilla` (referring to a wallet transaction) and add
        // `reserved_for_colored` (referring to a batch transfer), exactly one of them being set

        // recreate the table, as SQLite can't rename columns referenced by a foreign key nor add
        // a foreign key or a check constraint to an existing table
        manager
            .rename_table(
                Table::rename()
                    .table(ReservedTxo::Table, ReservedTxoOld::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                sea_query::Index::drop()
                    .name("idx-reservedtxo-txid-vout")
                    .table(ReservedTxoOld::Table)
                    .to_owned(),
            )
            .await?;
        create_reserved_txo_table(manager).await?;

        // all existing rows are reserved for a wallet transaction
        manager
            .get_connection()
            .execute_unprepared(
                "INSERT INTO reserved_txo (txid, vout, reserved_for_vanilla) \
                 SELECT txid, vout, reserved_for \
                 FROM reserved_txo_old \
                 WHERE reserved_for IS NOT NULL",
            )
            .await?;

        manager
            .drop_table(Table::drop().table(ReservedTxoOld::Table).to_owned())
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .rename_table(
                Table::rename()
                    .table(ReservedTxo::Table, ReservedTxoOld::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                sea_query::Index::drop()
                    .name("idx-reservedtxo-txid-vout")
                    .table(ReservedTxoOld::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(ReservedTxo::Table)
                    .if_not_exists()
                    .col(pk_auto(ReservedTxo::Idx))
                    .col(string(ReservedTxo::Txid))
                    .col(big_unsigned(ReservedTxo::Vout))
                    .col(integer_null(ReservedTxo::ReservedFor))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-reservedtxo-wallettransaction")
                            .from(ReservedTxo::Table, ReservedTxo::ReservedFor)
                            .to(WalletTransaction::Table, WalletTransaction::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;
        create_reserved_txo_index(manager).await?;

        // reservations for batch transfers can't be represented anymore
        manager
            .get_connection()
            .execute_unprepared(
                "INSERT INTO reserved_txo (txid, vout, reserved_for) \
                 SELECT txid, vout, reserved_for_vanilla \
                 FROM reserved_txo_old \
                 WHERE reserved_for_vanilla IS NOT NULL",
            )
            .await?;

        manager
            .drop_table(Table::drop().table(ReservedTxoOld::Table).to_owned())
            .await?;

        Ok(())
    }
}

async fn create_reserved_txo_table(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(ReservedTxo::Table)
                .if_not_exists()
                .col(pk_auto(ReservedTxo::Idx))
                .col(string(ReservedTxo::Txid))
                .col(big_unsigned(ReservedTxo::Vout))
                .col(integer_null(ReservedTxo::ReservedForVanilla))
                .col(integer_null(ReservedTxo::ReservedForColored))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk-reservedtxo-wallettransaction")
                        .from(ReservedTxo::Table, ReservedTxo::ReservedForVanilla)
                        .to(WalletTransaction::Table, WalletTransaction::Idx)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Restrict),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk-reservedtxo-batchtransfer")
                        .from(ReservedTxo::Table, ReservedTxo::ReservedForColored)
                        .to(BatchTransfer::Table, BatchTransfer::Idx)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Restrict),
                )
                // a TXO is reserved either for a wallet transaction or for a batch transfer
                .check(
                    Expr::col(ReservedTxo::ReservedForVanilla)
                        .is_null()
                        .and(Expr::col(ReservedTxo::ReservedForColored).is_not_null())
                        .or(Expr::col(ReservedTxo::ReservedForVanilla)
                            .is_not_null()
                            .and(Expr::col(ReservedTxo::ReservedForColored).is_null())),
                )
                .to_owned(),
        )
        .await?;
    create_reserved_txo_index(manager).await
}

async fn create_reserved_txo_index(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_index(
            sea_query::Index::create()
                .name("idx-reservedtxo-txid-vout")
                .table(ReservedTxo::Table)
                .col(ReservedTxo::Txid)
                .col(ReservedTxo::Vout)
                .unique()
                .clone(),
        )
        .await
}

#[derive(DeriveIden)]
enum ReservedTxo {
    Table,
    Idx,
    Txid,
    Vout,
    ReservedFor,
    ReservedForVanilla,
    ReservedForColored,
}

#[derive(DeriveIden)]
enum ReservedTxoOld {
    Table,
}

#[derive(DeriveIden)]
pub enum WalletTransaction {
    Table,
    Idx,
}

#[derive(DeriveIden)]
pub enum BatchTransfer {
    Table,
    Idx,
}
