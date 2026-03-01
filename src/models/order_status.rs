use diesel_derive_enum::DbEnum;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum, Serialize, Deserialize, ToSchema)]
#[ExistingTypePath = "crate::schema::sql_types::OrderStatus"]
pub enum OrderStatus {
    #[db_rename = "draft"]
    Draft,
    #[db_rename = "confirmed"]
    Confirmed,
    #[db_rename = "shipped"]
    Shipped,
    #[db_rename = "delivered"]
    Delivered,
    #[db_rename = "cancelled"]
    Cancelled,
}

impl OrderStatus {
    /// Enforces the state machine. Returns whether the transition is allowed.
    pub fn can_transition_to(&self, target: OrderStatus) -> bool {
        use OrderStatus::*;
        matches!(
            (self, target),
            (Draft, Confirmed)
                | (Draft, Cancelled)
                | (Confirmed, Shipped)
                | (Confirmed, Cancelled)
                | (Shipped, Delivered)
        )
    }
}
