pub use sea_orm_migration::prelude::*;

mod m20220810_130049_create_txo;
mod m20220810_131915_create_asset_rgb20;
mod m20220810_131920_create_asset_rgb21;
mod m20220810_132240_create_batch_transfer;
mod m20220810_132250_create_asset_transfer;
mod m20220810_132253_create_coloring;
mod m20220810_132256_create_transfer;
mod m20221128_182236_rename_rgb21_to_rgb121;
mod m20221130_152708_delete_zero_allocations;
mod m20221219_125200_create_consignment_endpoint;
mod m20221219_131407_create_transfer_consignment_endpoint;
mod m20230111_133557_add_coloring_constraint;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220810_130049_create_txo::Migration),
            Box::new(m20220810_131915_create_asset_rgb20::Migration),
            Box::new(m20220810_131920_create_asset_rgb21::Migration),
            Box::new(m20220810_132240_create_batch_transfer::Migration),
            Box::new(m20220810_132250_create_asset_transfer::Migration),
            Box::new(m20220810_132253_create_coloring::Migration),
            Box::new(m20220810_132256_create_transfer::Migration),
            Box::new(m20221128_182236_rename_rgb21_to_rgb121::Migration),
            Box::new(m20221130_152708_delete_zero_allocations::Migration),
            Box::new(m20221219_125200_create_consignment_endpoint::Migration),
            Box::new(m20221219_131407_create_transfer_consignment_endpoint::Migration),
            Box::new(m20230111_133557_add_coloring_constraint::Migration),
        ]
    }
}
