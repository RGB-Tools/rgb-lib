pub use sea_orm_migration::prelude::*;

mod m20230608_071249_init_db;
mod m20251017_074408_asset_update;
mod m20251105_132121_asset_update;
mod m20251215_124959_backup_info_update;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20230608_071249_init_db::Migration),
            Box::new(m20251017_074408_asset_update::Migration),
            Box::new(m20251105_132121_asset_update::Migration),
            Box::new(m20251215_124959_backup_info_update::Migration),
        ]
    }
}
