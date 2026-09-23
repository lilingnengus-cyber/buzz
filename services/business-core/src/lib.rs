#![forbid(unsafe_code)]

pub mod api;
pub mod b2;
pub mod b3;
pub mod b4;
mod bootstrap;
pub mod config;
pub mod crm;
pub mod document_approval;
pub mod master_command;
pub mod master_data;
pub mod master_data_api;
mod master_write_authority;
pub mod model;
pub mod numbering;
pub mod numbering_api;
pub mod operating_units;
pub mod product_master;
pub mod product_master_api;
pub mod s1;
pub mod security;
mod snapshot_transaction;
pub mod store;

pub use api::{router, AppState};
pub use config::Config;
pub use store::PgStore;
