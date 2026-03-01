use order_api::openapi::ApiDoc;
use utoipa::OpenApi;

fn main() {
    let doc = ApiDoc::openapi();
    println!("{}", doc.to_pretty_json().unwrap());
}
