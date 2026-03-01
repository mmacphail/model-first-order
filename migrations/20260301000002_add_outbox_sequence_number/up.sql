ALTER TABLE commerce_order_outbox
    ADD COLUMN sequence_number BIGSERIAL NOT NULL;

CREATE INDEX ON commerce_order_outbox (sequence_number);
