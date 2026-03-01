use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::models::order_line_item::OrderLineItem;
use crate::models::order_status::OrderStatus;
use crate::schema::orders;
use crate::serializers::serialize_bigdecimal_as_string;

/// Domain model — Queryable from the orders table.
/// This struct IS the specification for what an Order looks like.
#[derive(Debug, Queryable, Selectable, Identifiable, Serialize, Deserialize, ToSchema)]
#[diesel(table_name = orders)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Order {
    pub id: Uuid,
    pub status: OrderStatus,
    pub currency: String,

    #[serde(
        serialize_with = "serialize_bigdecimal_as_string",
        deserialize_with = "crate::serializers::deserialize_bigdecimal_from_string"
    )]
    #[schema(value_type = String, example = "1299.9900")]
    pub total_amount: BigDecimal,

    pub confirmed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Used for INSERT — only the fields the caller controls.
#[derive(Debug, Insertable, Deserialize, ToSchema)]
#[diesel(table_name = orders)]
pub struct NewOrder {
    #[schema(example = "EUR", min_length = 3, max_length = 3)]
    pub currency: String,
}

/// Used for status transitions — PATCH semantics.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = orders)]
pub struct OrderStatusUpdate {
    pub status: OrderStatus,
    pub confirmed_at: Option<DateTime<Utc>>,
}

/// Full aggregate view: order + its line items.
#[derive(Debug, Serialize, ToSchema)]
pub struct OrderWithItems {
    #[serde(flatten)]
    pub order: Order,
    pub items: Vec<OrderLineItem>,
}
