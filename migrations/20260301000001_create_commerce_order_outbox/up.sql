CREATE TABLE commerce_order_outbox (
    event_id        UUID          PRIMARY KEY DEFAULT gen_random_uuid(),
    aggregate_type  VARCHAR(100)  NOT NULL,
    aggregate_id    UUID          NOT NULL,
    event_type      VARCHAR(100)  NOT NULL,
    event_date      TIMESTAMPTZ   NOT NULL DEFAULT now(),
    event_data      JSONB         NOT NULL
);

CREATE INDEX ON commerce_order_outbox (aggregate_id);
CREATE INDEX ON commerce_order_outbox (event_date);
