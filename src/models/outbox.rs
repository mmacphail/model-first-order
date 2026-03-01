use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::order::Order;
use crate::models::order_line_item::OrderLineItem;
use crate::schema::{commerce_order_outbox, orders};

#[derive(Debug, Queryable, Selectable)]
#[diesel(table_name = commerce_order_outbox)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OutboxEvent {
    pub event_id: Uuid,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub event_date: DateTime<Utc>,
    pub event_data: serde_json::Value,
    pub sequence_number: i64,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = commerce_order_outbox)]
pub struct NewOutboxEvent {
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub event_data: serde_json::Value,
}

/// Loads the full OrderWithItems aggregate, serializes it, and inserts an outbox row.
/// Must be called inside the caller's transaction.
pub fn insert_outbox_event(
    conn: &mut PgConnection,
    order_id: Uuid,
    event_type: &str,
) -> Result<(), ApiError> {
    let order = orders::table
        .find(order_id)
        .select(Order::as_select())
        .first::<Order>(conn)?;

    let items = OrderLineItem::belonging_to(&order)
        .select(OrderLineItem::as_select())
        .load::<OrderLineItem>(conn)?;

    let aggregate = crate::models::order::OrderWithItems { order, items };
    let event_data = serde_json::to_value(&aggregate)
        .map_err(|e| ApiError::Internal(format!("Failed to serialize aggregate: {e}")))?;

    let new_event = NewOutboxEvent {
        aggregate_type: "order".into(),
        aggregate_id: order_id,
        event_type: event_type.into(),
        event_data,
    };

    diesel::insert_into(commerce_order_outbox::table)
        .values(&new_event)
        .execute(conn)?;

    Ok(())
}
