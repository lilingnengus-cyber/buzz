use business_core::PgStore;
use serde_json::json;
use sqlx::PgPool;
#[path = "../../business-core/tests/support/adjustment_intent_fixture.rs"]
pub(crate) mod adjustments;
use uuid::Uuid;
#[path = "../../business-core/tests/support/b2_seed.rs"]
mod b2_seed;

pub(crate) struct Fixture {
    pub(crate) actor: Uuid,
    pub(crate) legal_entity: Uuid,
    pub(crate) business_unit: Uuid,
    pub(crate) warehouse: Uuid,
    pub(crate) customer: Uuid,
    pub(crate) brand: Uuid,
    pub(crate) uom: Uuid,
    pub(crate) sku: Uuid,
}

pub(crate) async fn seed(pool: &sqlx::PgPool) -> Fixture {
    b2_seed::seed(pool).await
}
