use super::m20220810_132256_create_transfer::Transfer;
use super::m20221219_125200_create_consignment_endpoint::ConsignmentEndpoint;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20221219_131407_create_transfer_consignment_endpoint"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TransferConsignmentEndpoint::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TransferConsignmentEndpoint::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TransferConsignmentEndpoint::TransferIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransferConsignmentEndpoint::ConsignmentEndpointIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransferConsignmentEndpoint::Used)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-transferconsignmentendpoint-transfer")
                            .from(
                                TransferConsignmentEndpoint::Table,
                                TransferConsignmentEndpoint::TransferIdx,
                            )
                            .to(Transfer::Table, Transfer::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-transferconsignmentendpoint-consignmentendpoint")
                            .from(
                                TransferConsignmentEndpoint::Table,
                                TransferConsignmentEndpoint::ConsignmentEndpointIdx,
                            )
                            .to(ConsignmentEndpoint::Table, ConsignmentEndpoint::Idx)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-tce-transfer_idx-consignment_endpoint_idx")
                    .table(TransferConsignmentEndpoint::Table)
                    .col(TransferConsignmentEndpoint::TransferIdx)
                    .col(TransferConsignmentEndpoint::ConsignmentEndpointIdx)
                    .unique()
                    .clone(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(TransferConsignmentEndpoint::Table)
                    .to_owned(),
            )
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum TransferConsignmentEndpoint {
    Table,
    Idx,
    TransferIdx,
    ConsignmentEndpointIdx,
    Used,
}
