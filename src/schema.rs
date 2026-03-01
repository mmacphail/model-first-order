// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "order_status"))]
    pub struct OrderStatus;
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::*;

    commerce_order_outbox (event_id) {
        event_id -> Uuid,
        #[max_length = 100]
        aggregate_type -> Varchar,
        aggregate_id -> Uuid,
        #[max_length = 100]
        event_type -> Varchar,
        event_date -> Timestamptz,
        event_data -> Jsonb,
        sequence_number -> Int8,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::*;

    order_line_items (id) {
        id -> Uuid,
        order_id -> Uuid,
        #[max_length = 64]
        product_sku -> Varchar,
        quantity -> Int4,
        unit_price -> Numeric,
        line_total -> Numeric,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::*;
    use super::sql_types::OrderStatus;

    orders (id) {
        id -> Uuid,
        status -> OrderStatus,
        #[max_length = 3]
        currency -> Varchar,
        total_amount -> Numeric,
        confirmed_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::joinable!(order_line_items -> orders (order_id));

diesel::allow_tables_to_appear_in_same_query!(commerce_order_outbox, order_line_items, orders,);
