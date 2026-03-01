use order_api::openapi::ApiDoc;
use utoipa::OpenApi;

#[test]
fn openapi_json_is_up_to_date() {
    let expected = ApiDoc::openapi().to_pretty_json().unwrap();
    let path = format!("{}/openapi.json", env!("CARGO_MANIFEST_DIR"));
    let on_disk =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));

    assert_eq!(
        on_disk.trim(),
        expected.trim(),
        "openapi.json is stale — run `just gen` to regenerate"
    );
}
