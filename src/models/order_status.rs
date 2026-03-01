use diesel_derive_enum::DbEnum;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// Outbox event-type constants — single source of truth.
pub const ORDER_CREATED: &str = "ORDER_CREATED";
pub const ORDER_CONFIRMED: &str = "ORDER_CONFIRMED";
pub const ORDER_SHIPPED: &str = "ORDER_SHIPPED";
pub const ORDER_DELIVERED: &str = "ORDER_DELIVERED";
pub const ORDER_CANCELLED: &str = "ORDER_CANCELLED";
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
            OrderStatus::Confirmed => ORDER_CONFIRMED,
            OrderStatus::Shipped => ORDER_SHIPPED,
            OrderStatus::Delivered => ORDER_DELIVERED,
            OrderStatus::Cancelled => ORDER_CANCELLED,
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── as_event_type ──────────────────────────────────────────────────────────

    #[test]
    fn test_as_event_type_draft_returns_order_created() {
        assert_eq!(OrderStatus::Draft.as_event_type(), ORDER_CREATED);
    }

    #[test]
    fn test_as_event_type_confirmed_returns_order_confirmed() {
        assert_eq!(OrderStatus::Confirmed.as_event_type(), ORDER_CONFIRMED);
    }

    #[test]
    fn test_as_event_type_shipped_returns_order_shipped() {
        assert_eq!(OrderStatus::Shipped.as_event_type(), ORDER_SHIPPED);
    }

    #[test]
    fn test_as_event_type_delivered_returns_order_delivered() {
        assert_eq!(OrderStatus::Delivered.as_event_type(), ORDER_DELIVERED);
    }

    #[test]
    fn test_as_event_type_cancelled_returns_order_cancelled() {
        assert_eq!(OrderStatus::Cancelled.as_event_type(), ORDER_CANCELLED);
    }

    // ── can_transition_to — valid transitions ──────────────────────────────────

    #[test]
    fn test_transition_draft_to_confirmed_is_allowed() {
        assert!(OrderStatus::Draft.can_transition_to(OrderStatus::Confirmed));
    }

    #[test]
    fn test_transition_draft_to_cancelled_is_allowed() {
        assert!(OrderStatus::Draft.can_transition_to(OrderStatus::Cancelled));
    }

    #[test]
    fn test_transition_confirmed_to_shipped_is_allowed() {
        assert!(OrderStatus::Confirmed.can_transition_to(OrderStatus::Shipped));
    }

    #[test]
    fn test_transition_confirmed_to_cancelled_is_allowed() {
        assert!(OrderStatus::Confirmed.can_transition_to(OrderStatus::Cancelled));
    }

    #[test]
    fn test_transition_shipped_to_delivered_is_allowed() {
        assert!(OrderStatus::Shipped.can_transition_to(OrderStatus::Delivered));
    }

    // ── can_transition_to — invalid / backward transitions ────────────────────

    #[test]
    fn test_transition_draft_to_shipped_is_rejected() {
        assert!(!OrderStatus::Draft.can_transition_to(OrderStatus::Shipped));
    }

    #[test]
    fn test_transition_draft_to_delivered_is_rejected() {
        assert!(!OrderStatus::Draft.can_transition_to(OrderStatus::Delivered));
    }

    #[test]
    fn test_transition_draft_to_draft_is_rejected() {
        assert!(!OrderStatus::Draft.can_transition_to(OrderStatus::Draft));
    }

    #[test]
    fn test_transition_confirmed_to_draft_is_rejected() {
        assert!(!OrderStatus::Confirmed.can_transition_to(OrderStatus::Draft));
    }

    #[test]
    fn test_transition_confirmed_to_confirmed_is_rejected() {
        assert!(!OrderStatus::Confirmed.can_transition_to(OrderStatus::Confirmed));
    }

    #[test]
    fn test_transition_confirmed_to_delivered_is_rejected() {
        assert!(!OrderStatus::Confirmed.can_transition_to(OrderStatus::Delivered));
    }

    #[test]
    fn test_transition_shipped_to_draft_is_rejected() {
        assert!(!OrderStatus::Shipped.can_transition_to(OrderStatus::Draft));
    }

    #[test]
    fn test_transition_shipped_to_confirmed_is_rejected() {
        assert!(!OrderStatus::Shipped.can_transition_to(OrderStatus::Confirmed));
    }

    #[test]
    fn test_transition_shipped_to_cancelled_is_rejected() {
        assert!(!OrderStatus::Shipped.can_transition_to(OrderStatus::Cancelled));
    }

    #[test]
    fn test_transition_shipped_to_shipped_is_rejected() {
        assert!(!OrderStatus::Shipped.can_transition_to(OrderStatus::Shipped));
    }

    #[test]
    fn test_transition_delivered_to_any_is_rejected() {
        assert!(!OrderStatus::Delivered.can_transition_to(OrderStatus::Draft));
        assert!(!OrderStatus::Delivered.can_transition_to(OrderStatus::Confirmed));
        assert!(!OrderStatus::Delivered.can_transition_to(OrderStatus::Shipped));
        assert!(!OrderStatus::Delivered.can_transition_to(OrderStatus::Delivered));
        assert!(!OrderStatus::Delivered.can_transition_to(OrderStatus::Cancelled));
    }

    #[test]
    fn test_transition_cancelled_to_any_is_rejected() {
        assert!(!OrderStatus::Cancelled.can_transition_to(OrderStatus::Draft));
        assert!(!OrderStatus::Cancelled.can_transition_to(OrderStatus::Confirmed));
        assert!(!OrderStatus::Cancelled.can_transition_to(OrderStatus::Shipped));
        assert!(!OrderStatus::Cancelled.can_transition_to(OrderStatus::Delivered));
        assert!(!OrderStatus::Cancelled.can_transition_to(OrderStatus::Cancelled));
    }
}
