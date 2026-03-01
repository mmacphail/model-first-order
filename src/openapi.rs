use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::handlers::orders::create_order,
        crate::handlers::orders::get_order,
        crate::handlers::orders::list_orders,
        crate::handlers::orders::transition_status,
        crate::handlers::orders::add_line_item,
        crate::handlers::orders::delete_line_item,
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
