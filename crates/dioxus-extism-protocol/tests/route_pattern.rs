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

#[test]
fn empty_pattern_does_not_match_rooted_paths() {
    // An empty pattern must not match any rooted path — that would silently
    // apply route transforms to every page.
    assert!(!RoutePattern("".into()).matches("/"));
    assert!(!RoutePattern("".into()).matches("/product/42"));
}

#[test]
fn root_pattern_does_not_match_non_root() {
    assert!(!RoutePattern("/".into()).matches("/product/42"));
    assert!(!RoutePattern("/".into()).matches("/a"));
}

#[test]
fn multi_param_pattern_rejects_short_path() {
    assert!(!RoutePattern("/shop/:shop/item/:id".into()).matches("/shop/acme"));
    assert_eq!(
        RoutePattern("/shop/:shop/item/:id".into()).extract_params("/shop/acme"),
        None
    );
}

#[test]
fn trailing_slash_on_pattern_only() {
    assert!(!RoutePattern("/product/:id/".into()).matches("/product/42"));
}
