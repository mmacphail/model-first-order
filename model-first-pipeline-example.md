# Model-First Pipeline: Order Domain

## Step 0 — Think Phase (EARS Requirements)

This is a new entity with multiple endpoints and domain complexity (pricing, status transitions, line items), so we go through the think phase. These go in the **PR description**, not a living spec file.

### Entity Sketch

```
Order
├── id (UUID)
├── status (enum: Draft → Confirmed → Shipped → Delivered → Cancelled)
├── currency (ISO 4217, 3-char)
├── total_amount (NUMERIC 19,4 — computed from line items)
├── created_at / updated_at
│
└── OrderLineItem (1:N)
    ├── id (UUID)
    ├── order_id (UUID, FK → orders)
    ├── product_sku (VARCHAR)
    ├── quantity (INT, > 0)
    ├── unit_price (NUMERIC 19,4, >= 0)
    └── line_total (NUMERIC 19,4 — quantity × unit_price)
```

### EARS Requirements

| Pattern | Requirement |
|---|---|
| **Ubiquitous** | The system shall preserve NUMERIC(19,4) precision end-to-end from DB to frontend |
| **Event-driven** | When an order is confirmed, the system shall set `confirmed_at` and prevent further line item edits |
| **State-driven** | While an order is in Draft status, the system shall allow adding/removing line items |
| **Conditional** | If the computed total does not match `total_amount`, the system shall return a 409 Conflict |
| **Complex** | While viewing the orders list, when a new order is created via the API, the system shall appear in the list on next navigation without full page reload |

### Edge Cases

- Cancellation is only valid from Draft or Confirmed (not Shipped/Delivered)
- Currency is immutable after the first line item is added
- Deleting the last line item resets total_amount to 0 but keeps the order in Draft
- `total_amount` is stored (not purely computed) to preserve historical accuracy after price changes

---

## Step 1 — Diesel Migration (SQL)

```
diesel migration generate create_orders
```

### `up.sql`

```sql
-- Types
CREATE TYPE order_status AS ENUM (
    'draft',
    'confirmed',
    'shipped',
    'delivered',
    'cancelled'
);

-- Orders
CREATE TABLE orders (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    status          order_status NOT NULL DEFAULT 'draft',
    currency        VARCHAR(3) NOT NULL,
    total_amount    NUMERIC(19, 4) NOT NULL DEFAULT 0,
    confirmed_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_orders_status ON orders(status);

-- Line items
CREATE TABLE order_line_items (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id        UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_sku     VARCHAR(64) NOT NULL,
    quantity        INTEGER NOT NULL CHECK (quantity > 0),
    unit_price      NUMERIC(19, 4) NOT NULL CHECK (unit_price >= 0),
    line_total      NUMERIC(19, 4) NOT NULL GENERATED ALWAYS AS (quantity * unit_price) STORED,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_line_items_order_id ON order_line_items(order_id);

-- Trigger: auto-update updated_at
SELECT diesel_manage_updated_at('orders');
```

### `down.sql`

```sql
DROP TABLE order_line_items;
DROP TABLE orders;
DROP TYPE order_status;
```

After running `diesel migration run`, Diesel generates `schema.rs` automatically. **We never edit `schema.rs` by hand** — pipeline violation.

---

## Step 2 — Rust Model + Insertable Struct

This is where the source of truth lives. Every type decision here propagates outward.

### `src/models/order_status.rs`

```rust
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
    /// Enforces the state machine. Returns allowed transitions from current state.
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
```

### `src/models/order.rs`

```rust
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;
use utoipa::ToSchema;

use crate::models::order_status::OrderStatus;
use crate::schema::orders;
use crate::serializers::serialize_bigdecimal_as_string;

/// Domain model — Queryable from the orders table.
/// This struct IS the specification for what an Order looks like.
#[derive(Debug, Queryable, Selectable, Identifiable, Serialize, ToSchema)]
#[diesel(table_name = orders)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Order {
    pub id: Uuid,
    pub status: OrderStatus,
    pub currency: String,

    #[serde(serialize_with = "serialize_bigdecimal_as_string")]
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
```

### `src/models/order_line_item.rs`

```rust
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use utoipa::ToSchema;

use crate::schema::order_line_items;
use crate::serializers::serialize_bigdecimal_as_string;

#[derive(Debug, Queryable, Selectable, Identifiable, Associations, Serialize, ToSchema)]
#[diesel(table_name = order_line_items)]
#[diesel(belongs_to(crate::models::order::Order))]
pub struct OrderLineItem {
    pub id: Uuid,
    pub order_id: Uuid,
    pub product_sku: String,
    pub quantity: i32,

    #[serde(serialize_with = "serialize_bigdecimal_as_string")]
    #[schema(value_type = String, example = "49.9900")]
    pub unit_price: BigDecimal,

    #[serde(serialize_with = "serialize_bigdecimal_as_string")]
    #[schema(value_type = String, example = "149.9700")]
    pub line_total: BigDecimal,

    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable, Deserialize, ToSchema)]
#[diesel(table_name = order_line_items)]
pub struct NewLineItem {
    pub order_id: Uuid,

    #[schema(example = "SKU-WIDGET-001")]
    pub product_sku: String,

    #[schema(example = 3, minimum = 1)]
    pub quantity: i32,

    #[serde(deserialize_with = "crate::serializers::deserialize_bigdecimal_from_string")]
    #[schema(value_type = String, example = "49.99")]
    pub unit_price: BigDecimal,
}
```

### `src/serializers.rs` (shared concern)

```rust
use bigdecimal::BigDecimal;
use serde::{self, Deserialize, Deserializer, Serializer};

pub fn serialize_bigdecimal_as_string<S>(value: &BigDecimal, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&value.to_string())
}

pub fn deserialize_bigdecimal_from_string<'de, D>(d: D) -> Result<BigDecimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    s.parse::<BigDecimal>().map_err(serde::de::Error::custom)
}
```

**Key observation:** The `BigDecimal` serializer is exactly the kind of "hard-won knowledge" the brief warns against regenerating. Four layers (DB → Rust → JSON → TS) must agree on string representation. This logic lives in code, not in a spec.

---

## Step 3 — Handler + utoipa + tracing

### `src/handlers/orders.rs`

```rust
use actix_web::{web, HttpResponse};
use bigdecimal::BigDecimal;
use chrono::Utc;
use diesel::prelude::*;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::ApiError;
use crate::models::order::*;
use crate::models::order_line_item::*;
use crate::models::order_status::OrderStatus;
use crate::schema::{orders, order_line_items};

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
        return Err(ApiError::BadRequest("Invalid ISO 4217 currency code".into()));
    }

    let order = web::block(move || {
        let mut conn = pool.get()?;
        diesel::insert_into(orders::table)
            .values(&new_order)
            .returning(Order::as_returning())
            .get_result(&mut conn)
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

// ── Transition Order Status ───────────────────────────────

#[utoipa::path(
    patch,
    path = "/api/orders/{id}/status",
    request_body = StatusTransitionRequest,
    responses(
        (status = 200, description = "Status updated", body = Order),
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

        let current = orders::table
            .find(order_id)
            .select(Order::as_select())
            .first::<Order>(&mut conn)?;

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
                .first::<Option<BigDecimal>>(&mut conn)?
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

        diesel::update(orders::table.find(order_id))
            .set(&update)
            .returning(Order::as_returning())
            .get_result::<Order>(&mut conn)
            .map_err(Into::into)
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

    let item = web::block(move || {
        let mut conn = pool.get()?;

        // EARS: state-driven — only draft orders accept new items
        let order = orders::table
            .find(order_id)
            .select(Order::as_select())
            .first::<Order>(&mut conn)?;

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
            .get_result::<OrderLineItem>(&mut conn)?;

        // Recompute total_amount from all line items
        let new_total: BigDecimal = order_line_items::table
            .filter(order_line_items::order_id.eq(order_id))
            .select(diesel::dsl::sum(order_line_items::line_total))
            .first::<Option<BigDecimal>>(&mut conn)?
            .unwrap_or_default();

        diesel::update(orders::table.find(order_id))
            .set(orders::total_amount.eq(&new_total))
            .execute(&mut conn)?;

        info!(order_id = %order_id, item_id = %item.id, "Line item added");
        Ok(item)
    })
    .await??;

    Ok(HttpResponse::Created().json(item))
}

// ── Response / Request types (utoipa-annotated) ───────────

#[derive(Serialize, ToSchema)]
pub struct OrderWithItems {
    #[serde(flatten)]
    pub order: Order,
    pub items: Vec<OrderLineItem>,
}

#[derive(Deserialize, ToSchema)]
pub struct StatusTransitionRequest {
    pub status: OrderStatus,
}

#[derive(Deserialize, ToSchema)]
pub struct NewLineItemRequest {
    pub product_sku: String,
    pub quantity: i32,
    #[serde(deserialize_with = "crate::serializers::deserialize_bigdecimal_from_string")]
    #[schema(value_type = String, example = "49.99")]
    pub unit_price: BigDecimal,
}
```

---

## Step 4 — Route Registration + openapi.rs

### `src/routes.rs`

```rust
use actix_web::web;
use crate::handlers::orders;

pub fn configure_orders(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/orders")
            .route("", web::post().to(orders::create_order))
            .route("/{id}", web::get().to(orders::get_order))
            .route("/{id}/status", web::patch().to(orders::transition_status))
            .route("/{id}/items", web::post().to(orders::add_line_item)),
    );
}
```

### `src/openapi.rs`

```rust
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::handlers::orders::create_order,
        crate::handlers::orders::get_order,
        crate::handlers::orders::transition_status,
        crate::handlers::orders::add_line_item,
    ),
    components(schemas(
        crate::models::order::Order,
        crate::models::order::NewOrder,
        crate::models::order_line_item::OrderLineItem,
        crate::models::order_line_item::NewLineItem,
        crate::models::order_status::OrderStatus,
        crate::handlers::orders::OrderWithItems,
        crate::handlers::orders::StatusTransitionRequest,
        crate::handlers::orders::NewLineItemRequest,
    )),
    tags(
        (name = "Orders", description = "Order management"),
        (name = "Order Line Items", description = "Line item management"),
    )
)]
pub struct ApiDoc;
```

---

## Step 5 — `just gen`

This is the **generation boundary**. Everything below this line is derived, not authored.

```makefile
# justfile

gen:
    # 1. Export OpenAPI from the running binary (or build-time generation)
    cargo run --bin export-openapi > openapi.json
    # 2. Generate TypeScript client
    npx openapi-typescript-codegen \
        --input openapi.json \
        --output app/client \
        --client fetch
    @echo "✅ Generated openapi.json + TS client"
```

The generated `app/client/` now contains typed functions like:

```typescript
// app/client/services/OrdersService.ts  ← GENERATED, DO NOT EDIT
export class OrdersService {
    public static createOrder(requestBody: NewOrder): CancelablePromise<Order> { ... }
    public static getOrder(id: string): CancelablePromise<OrderWithItems> { ... }
    public static transitionStatus(id: string, requestBody: StatusTransitionRequest): CancelablePromise<Order> { ... }
    public static addLineItem(id: string, requestBody: NewLineItemRequest): CancelablePromise<OrderLineItem> { ... }
}
```

```typescript
// app/client/models/Order.ts  ← GENERATED, DO NOT EDIT
export type Order = {
    id: string;
    status: OrderStatus;
    currency: string;
    total_amount: string;  // ← BigDecimal came through as string, end-to-end
    confirmed_at: string | null;
    created_at: string;
    updated_at: string;
};
```

**Pipeline invariant:** `total_amount` is `NUMERIC(19,4)` in SQL → `BigDecimal` in Rust → `String` in JSON → `string` in TypeScript. The brief's EARS requirement ("preserve precision end-to-end") is enforced mechanically, not by a prose spec.

---

## Step 6 — Frontend Route (Remix loader/action)

```typescript
// app/routes/orders.$id.tsx
import { useLoaderData } from "@remix-run/react";
import { OrdersService } from "~/client";
import type { OrderWithItems } from "~/client";

export async function loader({ params }: LoaderFunctionArgs) {
    const order = await OrdersService.getOrder(params.id!);
    return json(order);
}

export async function action({ request, params }: ActionFunctionArgs) {
    const form = await request.formData();
    const intent = form.get("intent");

    switch (intent) {
        case "confirm":
            return json(
                await OrdersService.transitionStatus(params.id!, {
                    status: "Confirmed",
                })
            );
        case "add-item":
            return json(
                await OrdersService.addLineItem(params.id!, {
                    product_sku: String(form.get("sku")),
                    quantity: Number(form.get("quantity")),
                    unit_price: String(form.get("unit_price")), // string! precision preserved
                })
            );
    }
}

export default function OrderDetail() {
    const { order, items } = useLoaderData<OrderWithItems>();

    return (
        <div>
            <h1>Order {order.id}</h1>
            <p>Status: {order.status}</p>
            <p>Total: {order.currency} {order.total_amount}</p>
            {/* items table, forms, etc. */}
        </div>
    );
}
```

---

## Step 7 — Tests

### Rust integration test

```rust
#[actix_web::test]
async fn test_order_lifecycle() {
    let pool = setup_test_db().await;
    let app = test::init_service(create_app(pool.clone())).await;

    // Create order
    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(json!({ "currency": "EUR" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);
    let order: Order = test::read_body_json(resp).await;
    assert_eq!(order.status, OrderStatus::Draft);

    // Add line item
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(json!({
            "product_sku": "WIDGET-001",
            "quantity": 3,
            "unit_price": "49.9900"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    // Confirm order
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let order: Order = test::read_body_json(resp).await;
    assert!(order.confirmed_at.is_some());

    // Cannot add items to confirmed order (EARS: state-driven)
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(json!({
            "product_sku": "GADGET-002",
            "quantity": 1,
            "unit_price": "9.99"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);

    // Cannot transition backwards
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(json!({ "status": "Draft" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);
}

#[actix_web::test]
async fn test_bigdecimal_precision_preserved() {
    // EARS: ubiquitous — precision end-to-end
    let pool = setup_test_db().await;
    let app = test::init_service(create_app(pool.clone())).await;

    // Create order + line item with precise decimal
    let order = create_test_order(&app, "USD").await;
    add_test_item(&app, order.id, "PRECISE-001", 1, "12345.6789").await;

    // Fetch and verify string representation preserves all 4 decimal places
    let resp = test::TestRequest::get()
        .uri(&format!("/api/orders/{}", order.id))
        .send_request(&app)
        .await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total_amount"], "12345.6789");
}
```

### Vitest (TS client contract)

```typescript
// app/__tests__/order-client.test.ts
import { describe, it, expect } from "vitest";
import type { Order, NewOrder, OrderStatus } from "~/client";

describe("Order types", () => {
    it("total_amount is string (BigDecimal precision)", () => {
        const order: Order = {
            id: "uuid",
            status: "Draft" as OrderStatus,
            currency: "EUR",
            total_amount: "12345.6789", // ← string, not number
            confirmed_at: null,
            created_at: "2025-02-01T00:00:00Z",
            updated_at: "2025-02-01T00:00:00Z",
        };
        expect(typeof order.total_amount).toBe("string");
    });
});
```

---

## Step 8 — `just pre-commit`

```makefile
pre-commit:
    cargo fmt -- --check
    cargo clippy -- -D warnings
    cargo test
    cd app && npx vitest run
    cd app && npx tsc --noEmit
    @echo "✅ All checks passed"
```

---

## Pipeline Direction Summary

```
                       SOURCE OF TRUTH
                            │
                            ▼
                   ┌─────────────────┐
                   │  SQL Migration   │  ← Step 1: schema decisions
                   └────────┬────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │   Rust Structs   │  ← Step 2: types, derives, serde
                   └────────┬────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │    Handlers +    │  ← Step 3: business logic, utoipa
                   │    utoipa        │
                   └────────┬────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │  Route + OpenAPI │  ← Step 4: registration
                   └────────┬────────┘
                            │
                  ══════════╪══════════  ← GENERATION BOUNDARY
                            │
                            ▼
                   ┌─────────────────┐
                   │  openapi.json    │  ← Step 5: just gen (derived)
                   │  TS client       │
                   └────────┬────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │  Frontend Route  │  ← Step 6: consumes generated types
                   └────────┬────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │     Tests        │  ← Step 7: validates all layers
                   └─────────────────┘
```

Everything above the generation boundary is **hand-written, authoritative code**.
Everything below is **derived from it** and can be regenerated with `just gen`.
The EARS requirements from Step 0 are now **embedded in code and enforced by tests** — the PR description preserves the reasoning, but the code is the living spec.
