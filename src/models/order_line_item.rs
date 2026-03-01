use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::schema::order_line_items;
use crate::serializers::serialize_bigdecimal_as_string;

#[derive(
    Debug, Queryable, Selectable, Identifiable, Associations, Serialize, Deserialize, ToSchema,
)]
#[diesel(table_name = order_line_items)]
#[diesel(belongs_to(crate::models::order::Order))]
pub struct OrderLineItem {
    pub id: Uuid,
    pub order_id: Uuid,
    pub product_sku: String,
    pub quantity: i32,

    #[serde(
        serialize_with = "serialize_bigdecimal_as_string",
        deserialize_with = "crate::serializers::deserialize_bigdecimal_from_string"
    )]
    #[schema(value_type = String, example = "49.9900")]
    pub unit_price: BigDecimal,

    #[serde(
        serialize_with = "serialize_bigdecimal_as_string",
        deserialize_with = "crate::serializers::deserialize_bigdecimal_from_string"
    )]
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
