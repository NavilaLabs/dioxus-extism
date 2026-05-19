use dioxus_extism_protocol::RoutePattern;

#[test]
fn matches_single_param() {
    assert!(RoutePattern("/product/:id".into()).matches("/product/42"));
}
#[test]
fn rejects_extra_segments() {
    assert!(!RoutePattern("/product/:id".into()).matches("/product/42/reviews"));
}
#[test]
fn rejects_wrong_prefix() {
    assert!(!RoutePattern("/product/:id".into()).matches("/products/42"));
}
#[test]
fn rejects_trailing_slash() {
    assert!(!RoutePattern("/product/:id".into()).matches("/product/42/"));
}
#[test]
fn extracts_single_param() {
    let p = RoutePattern("/product/:id".into());
    assert_eq!(p.extract_params("/product/42").unwrap()["id"], "42");
}
#[test]
fn extracts_multiple_params() {
    let p = RoutePattern("/shop/:shop/item/:id".into());
    let params = p.extract_params("/shop/acme/item/99").unwrap();
    assert_eq!(params["shop"], "acme");
    assert_eq!(params["id"], "99");
}
#[test]
fn root_matches_root() {
    assert!(RoutePattern("/".into()).matches("/"));
}
