use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ReusedScript::Table)
                    .if_not_exists()
                    .col(pk_auto(ReusedScript::Idx))
                    .col(tiny_unsigned(ReusedScript::Keychain))
                    .col(string_uniq(ReusedScript::Script))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(KeychainReuse::Table)
                    .if_not_exists()
                    .col(tiny_unsigned(KeychainReuse::Keychain).primary_key())
                    .col(string_null(KeychainReuse::PinnedScript))
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
        manager
            .alter_table(
                Table::alter()
                    .table(Transfer::Table)
                    .drop_column(Transfer::ReceiveDir)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(KeychainReuse::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(ReusedScript::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum ReusedScript {
    Table,
    Idx,
    Keychain,
    Script,
}

#[derive(DeriveIden)]
enum KeychainReuse {
    Table,
    Keychain,
    PinnedScript,
}

#[derive(DeriveIden)]
enum Transfer {
    Table,
    ReceiveDir,
}
