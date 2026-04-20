use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ReservedTxo::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum ReservedTxo {
    Table,
    Idx,
    Txid,
    Vout,
    ReservedFor,
}

#[derive(DeriveIden)]
pub enum WalletTransaction {
    Table,
    Idx,
}
