DROP INDEX IF EXISTS commerce_order_outbox_sequence_number_idx;
ALTER TABLE commerce_order_outbox DROP COLUMN sequence_number;
