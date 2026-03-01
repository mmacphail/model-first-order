use actix_web::{web, HttpResponse};
use bigdecimal::BigDecimal;
use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize;
use tracing::{info, instrument, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::ApiError;
use crate::models::order::*;
use crate::models::order_line_item::*;
use crate::models::order_status::{OrderStatus, ORDER_CREATED, ORDER_UPDATED};
use crate::models::outbox::insert_outbox_event;
use crate::schema::{order_line_items, orders};

// ── Create Order ──────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/api/orders",
    request_body = NewOrder,
    responses(
        (status = 201, description = "Order created", body = Order),
        (status = 400, description = "Invalid input"),
    ),
    tag = "Orders"
)]
#[instrument(skip(pool))]
pub async fn create_order(
    pool: web::Data<DbPool>,
    body: web::Json<NewOrder>,
) -> Result<HttpResponse, ApiError> {
    let new_order = body.into_inner();

    // Validate ISO 4217 currency (EARS: conditional)
    if new_order.currency.len() != 3 || !new_order.currency.chars().all(|c| c.is_ascii_uppercase())
    {
        return Err(ApiError::BadRequest(
            "Invalid ISO 4217 currency code".into(),
        ));
    }

    let order = web::block(move || {
        let mut conn = pool.get()?;
        conn.transaction::<_, ApiError, _>(|conn| {
            let order = diesel::insert_into(orders::table)
                .values(&new_order)
                .returning(Order::as_returning())
                .get_result::<Order>(conn)?;

            insert_outbox_event(conn, order.id, ORDER_CREATED)?;

            Ok(order)
        })
    })
    .await??;

    info!(order_id = %order.id, "Order created");
    Ok(HttpResponse::Created().json(order))
}

// ── Get Order with Line Items ─────────────────────────────

#[utoipa::path(
    get,
    path = "/api/orders/{id}",
    params(("id" = Uuid, Path, description = "Order ID")),
    responses(
        (status = 200, description = "Order found", body = OrderWithItems),
        (status = 404, description = "Not found"),
    ),
    tag = "Orders"
)]
#[instrument(skip(pool))]
pub async fn get_order(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let order_id = path.into_inner();

    let (order, items) = web::block(move || {
        let mut conn = pool.get()?;

        let order = orders::table
            .find(order_id)
            .select(Order::as_select())
            .first::<Order>(&mut conn)
            .optional()?
            .ok_or(ApiError::NotFound)?;

        let items = OrderLineItem::belonging_to(&order)
            .select(OrderLineItem::as_select())
            .load::<OrderLineItem>(&mut conn)?;

        Ok::<_, ApiError>((order, items))
    })
    .await??;

    Ok(HttpResponse::Ok().json(OrderWithItems { order, items }))
}

// ── List Orders ───────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/api/orders",
    params(
        ("limit" = Option<i64>, Query, description = "Max items to return (default 50, max 100)"),
        ("offset" = Option<i64>, Query, description = "Number of items to skip (default 0)"),
    ),
    responses(
        (status = 200, description = "Orders list", body = Vec<Order>),
    ),
    tag = "Orders"
)]
#[instrument(skip(pool))]
pub async fn list_orders(
    pool: web::Data<DbPool>,
    query: web::Query<PaginationParams>,
) -> Result<HttpResponse, ApiError> {
    let params = query.into_inner();
    let limit = params.limit.unwrap_or(50).clamp(1, 100);
    let offset = params.offset.unwrap_or(0).max(0);

    let result = web::block(move || {
        let mut conn = pool.get()?;
        orders::table
            .select(Order::as_select())
            .order(orders::created_at.desc())
            .limit(limit)
            .offset(offset)
            .load::<Order>(&mut conn)
            .map_err(ApiError::from)
    })
    .await??;

    Ok(HttpResponse::Ok().json(result))
}

// ── Transition Order Status ───────────────────────────────

#[utoipa::path(
    patch,
    path = "/api/orders/{id}/status",
    request_body = StatusTransitionRequest,
    responses(
        (status = 200, description = "Status updated", body = Order),
        (status = 404, description = "Order not found"),
        (status = 409, description = "Invalid transition"),
    ),
    tag = "Orders"
)]
#[instrument(skip(pool))]
pub async fn transition_status(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<StatusTransitionRequest>,
) -> Result<HttpResponse, ApiError> {
    let order_id = path.into_inner();
    let target = body.into_inner().status;

    let order = web::block(move || {
        let mut conn = pool.get()?;
        conn.transaction::<_, ApiError, _>(|conn| {
            let current = orders::table
                .find(order_id)
                .select(Order::as_select())
                .for_update()
                .first::<Order>(conn)
                .optional()?
                .ok_or(ApiError::NotFound)?;

            // EARS: event-driven — state machine enforcement
            if !current.status.can_transition_to(target) {
                warn!(
                    order_id = %order_id,
                    from = ?current.status,
                    to = ?target,
                    "Invalid status transition"
                );
                return Err(ApiError::Conflict(format!(
                    "Cannot transition from {:?} to {:?}",
                    current.status, target
                )));
            }

            // EARS: event-driven — set confirmed_at on confirmation
            let confirmed_at = if target == OrderStatus::Confirmed {
                Some(Utc::now())
            } else {
                current.confirmed_at
            };

            // EARS: conditional — verify total before confirmation
            if target == OrderStatus::Confirmed {
                let item_sum: BigDecimal = order_line_items::table
                    .filter(order_line_items::order_id.eq(order_id))
                    .select(diesel::dsl::sum(order_line_items::line_total))
                    .first::<Option<BigDecimal>>(conn)?
                    .unwrap_or_default();

                if item_sum != current.total_amount {
                    return Err(ApiError::Conflict(
                        "total_amount does not match sum of line items".into(),
                    ));
                }
            }

            let update = OrderStatusUpdate {
                status: target,
                confirmed_at,
            };

            let order = diesel::update(orders::table.find(order_id))
                .set(&update)
                .returning(Order::as_returning())
                .get_result::<Order>(conn)?;

            insert_outbox_event(conn, order_id, target.as_event_type())?;

            Ok(order)
        })
    })
    .await??;

    info!(order_id = %order.id, new_status = ?order.status, "Order status transitioned");
    Ok(HttpResponse::Ok().json(order))
}

// ── Add Line Item ─────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/api/orders/{id}/items",
    request_body = NewLineItemRequest,
    responses(
        (status = 201, description = "Line item added", body = OrderLineItem),
        (status = 400, description = "Invalid input"),
        (status = 404, description = "Order not found"),
        (status = 409, description = "Order not in draft status"),
    ),
    tag = "Order Line Items"
)]
#[instrument(skip(pool))]
pub async fn add_line_item(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<NewLineItemRequest>,
) -> Result<HttpResponse, ApiError> {
    let order_id = path.into_inner();
    let req = body.into_inner();

    // Validate line item fields
    if req.quantity <= 0 {
        return Err(ApiError::BadRequest(
            "quantity must be greater than 0".into(),
        ));
    }
    if req.unit_price < 0_i32 {
        return Err(ApiError::BadRequest(
            "unit_price must be non-negative".into(),
        ));
    }

    let item = web::block(move || {
        let mut conn = pool.get()?;
        conn.transaction::<_, ApiError, _>(|conn| {
            // EARS: state-driven — only draft orders accept new items
            let order = orders::table
                .find(order_id)
                .select(Order::as_select())
                .for_update()
                .first::<Order>(conn)
                .optional()?
                .ok_or(ApiError::NotFound)?;

            if order.status != OrderStatus::Draft {
                return Err(ApiError::Conflict(
                    "Can only add items to draft orders".into(),
                ));
            }

            let new_item = NewLineItem {
                order_id,
                product_sku: req.product_sku,
                quantity: req.quantity,
                unit_price: req.unit_price,
            };

            let item = diesel::insert_into(order_line_items::table)
                .values(&new_item)
                .returning(OrderLineItem::as_returning())
                .get_result::<OrderLineItem>(conn)?;

            // Recompute total_amount from all line items
            let new_total: BigDecimal = order_line_items::table
                .filter(order_line_items::order_id.eq(order_id))
                .select(diesel::dsl::sum(order_line_items::line_total))
                .first::<Option<BigDecimal>>(conn)?
                .unwrap_or_default();

            diesel::update(orders::table.find(order_id))
                .set(orders::total_amount.eq(&new_total))
                .execute(conn)?;

            insert_outbox_event(conn, order_id, ORDER_UPDATED)?;

            info!(order_id = %order_id, item_id = %item.id, "Line item added");
            Ok(item)
        })
    })
    .await??;

    Ok(HttpResponse::Created().json(item))
}

// ── Delete Line Item ──────────────────────────────────────

#[utoipa::path(
    delete,
    path = "/api/orders/{order_id}/items/{item_id}",
    params(
        ("order_id" = Uuid, Path, description = "Order ID"),
        ("item_id" = Uuid, Path, description = "Line item ID"),
    ),
    responses(
        (status = 204, description = "Line item deleted"),
        (status = 404, description = "Order or line item not found"),
        (status = 409, description = "Order not in draft status"),
    ),
    tag = "Order Line Items"
)]
#[instrument(skip(pool))]
pub async fn delete_line_item(
    pool: web::Data<DbPool>,
    path: web::Path<(Uuid, Uuid)>,
) -> Result<HttpResponse, ApiError> {
    let (order_id, item_id) = path.into_inner();

    web::block(move || {
        let mut conn = pool.get()?;
        conn.transaction::<_, ApiError, _>(|conn| {
            // EARS: state-driven — only draft orders allow item removal
            let order = orders::table
                .find(order_id)
                .select(Order::as_select())
                .for_update()
                .first::<Order>(conn)
                .optional()?
                .ok_or(ApiError::NotFound)?;

            if order.status != OrderStatus::Draft {
                return Err(ApiError::Conflict(
                    "Can only remove items from draft orders".into(),
                ));
            }

            let affected = diesel::delete(
                order_line_items::table
                    .filter(order_line_items::id.eq(item_id))
                    .filter(order_line_items::order_id.eq(order_id)),
            )
            .execute(conn)?;

            if affected == 0 {
                return Err(ApiError::NotFound);
            }

            // Recompute total_amount (edge case: deleting last item resets to 0)
            let new_total: BigDecimal = order_line_items::table
                .filter(order_line_items::order_id.eq(order_id))
                .select(diesel::dsl::sum(order_line_items::line_total))
                .first::<Option<BigDecimal>>(conn)?
                .unwrap_or_default();

            diesel::update(orders::table.find(order_id))
                .set(orders::total_amount.eq(&new_total))
                .execute(conn)?;

            insert_outbox_event(conn, order_id, ORDER_UPDATED)?;

            info!(order_id = %order_id, item_id = %item_id, "Line item deleted");
            Ok(())
        })
    })
    .await??;

    Ok(HttpResponse::NoContent().finish())
}

// ── Response / Request types (utoipa-annotated) ───────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct StatusTransitionRequest {
    pub status: OrderStatus,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct NewLineItemRequest {
    pub product_sku: String,
    pub quantity: i32,
    #[serde(deserialize_with = "crate::serializers::deserialize_bigdecimal_from_string")]
    #[schema(value_type = String, example = "49.99")]
    pub unit_price: BigDecimal,
}

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
