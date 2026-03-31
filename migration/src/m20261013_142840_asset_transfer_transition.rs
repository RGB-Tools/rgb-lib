use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

// TypeOfTransition values (see wallet::objects::TypeOfTransition)
const TRANSITION_TRANSFER: i32 = 1;
const TRANSITION_INFLATE: i32 = 2;
const TRANSITION_BURN: i32 = 3;
// ColoringType::Issue (see database::enums::ColoringType)
const COLORING_ISSUE: i32 = 2;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AssetTransfer::Table)
                    .add_column(
                        ColumnDef::new(AssetTransfer::TransitionType)
                            .tiny_unsigned()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // backfill the outgoing asset transfers (incoming ones record no transition), telling the
        // transition apart as the transfer listing did before: a burn is the only one with a
        // transfer having no recipient, an inflation the only one with issue colorings. Mints
        // were never released before this migration
        manager
            .get_connection()
            .execute_unprepared(&format!(
                "UPDATE asset_transfer \
                 SET transition_type = CASE \
                     WHEN EXISTS ( \
                         SELECT 1 FROM transfer t \
                         WHERE t.asset_transfer_idx = asset_transfer.idx \
                         AND t.recipient_id IS NULL \
                     ) THEN {TRANSITION_BURN} \
                     WHEN EXISTS ( \
                         SELECT 1 FROM coloring c \
                         WHERE c.asset_transfer_idx = asset_transfer.idx \
                         AND c.type = {COLORING_ISSUE} \
                     ) THEN {TRANSITION_INFLATE} \
                     ELSE {TRANSITION_TRANSFER} \
                 END \
                 WHERE batch_transfer_idx IN ( \
                     SELECT b.idx FROM batch_transfer b WHERE b.incoming = false \
                 )"
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AssetTransfer::Table)
                    .drop_column(AssetTransfer::TransitionType)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AssetTransfer {
    Table,
    TransitionType,
}
