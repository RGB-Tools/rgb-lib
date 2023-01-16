use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20221219_125200_create_consignment_endpoint"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ConsignmentEndpoint::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ConsignmentEndpoint::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ConsignmentEndpoint::Protocol)
                            .tiny_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ConsignmentEndpoint::Endpoint)
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-ce-protocol-endpoint")
                    .table(ConsignmentEndpoint::Table)
                    .col(ConsignmentEndpoint::Protocol)
                    .col(ConsignmentEndpoint::Endpoint)
                    .unique()
                    .clone(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ConsignmentEndpoint::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum ConsignmentEndpoint {
    Table,
    Idx,
    Protocol,
    Endpoint,
}
