use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220810_130049_create_txo"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Txo::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Txo::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Txo::Txid).string().not_null())
                    .col(ColumnDef::new(Txo::Vout).unsigned().not_null())
                    .col(ColumnDef::new(Txo::BtcAmount).string().not_null())
                    .col(ColumnDef::new(Txo::Colorable).boolean().not_null())
                    .col(ColumnDef::new(Txo::Spent).boolean().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-txo-txid-vout")
                    .table(Txo::Table)
                    .col(Txo::Txid)
                    .col(Txo::Vout)
                    .unique()
                    .clone(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Txo::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum Txo {
    Table,
    Idx,
    Txid,
    Vout,
    BtcAmount,
    Colorable,
    Spent,
}
