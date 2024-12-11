pub use sea_orm_migration::prelude::*;

mod m20230608_071249_init_db;mod m20241211_214243_m_test_issues_37;


pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20230608_071249_init_db::Migration)]
    }
}
