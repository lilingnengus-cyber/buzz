//! Business Core B2 transactional sales and settlement modules.

mod agent_inventory_counts;
mod agent_return_search;
mod agent_returns;
pub mod api;
pub(crate) mod common;
mod inventory;
mod inventory_count;
mod inventory_count_api;
mod inventory_count_scope;
pub mod model;
pub(crate) mod return_confirmation;
mod return_disposition;
mod return_disposition_api;
mod return_scope;
mod returns;
mod sales;
mod settlement;
pub(crate) mod stock_reversal_guard;

pub use common::DomainError;
pub use inventory::InventoryService;
pub use inventory_count::{
    CreateInventoryCount, InventoryCountDetail, InventoryCountOperation, InventoryCountOption,
    InventoryCountService, InventoryCountSummary, SubmitInventoryCount,
};
pub use return_disposition::{
    AcknowledgePurchaseReturn, DispatchPurchaseReturn, InspectSalesReturn, InspectionView,
    ReturnDispositionService,
};
pub use returns::{
    CancelReturnDraft, CreateReturn, ReplaceReturnDraft, ReturnOptions, ReturnService,
    ReturnSummary, ReverseReturn,
};
pub use sales::SalesService;
pub use settlement::SettlementService;
