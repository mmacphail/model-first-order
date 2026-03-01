use diesel_derive_enum::DbEnum;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// Outbox event-type constants — single source of truth.
pub const ORDER_CREATED: &str = "ORDER_CREATED";
pub const ORDER_UPDATED: &str = "ORDER_UPDATED";

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
    /// Returns the SCREAMING_SNAKE outbox event type for a status transition.
    pub fn as_event_type(&self) -> &'static str {
        match self {
            OrderStatus::Draft => ORDER_CREATED,
            OrderStatus::Confirmed => "ORDER_CONFIRMED",
            OrderStatus::Shipped => "ORDER_SHIPPED",
            OrderStatus::Delivered => "ORDER_DELIVERED",
            OrderStatus::Cancelled => "ORDER_CANCELLED",
        }
    }

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
