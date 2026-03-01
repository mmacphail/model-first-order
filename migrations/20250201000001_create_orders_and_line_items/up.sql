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
