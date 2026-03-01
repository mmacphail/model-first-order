use actix_web::web;

use crate::handlers::{health, orders};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health::health)).service(
        web::scope("/api/orders")
            .route("", web::post().to(orders::create_order))
            .route("", web::get().to(orders::list_orders))
            .route("/{id}", web::get().to(orders::get_order))
            .route("/{id}/status", web::patch().to(orders::transition_status))
            .route("/{id}/items", web::post().to(orders::add_line_item))
            .route(
                "/{order_id}/items/{item_id}",
                web::delete().to(orders::delete_line_item),
            ),
    );
}
