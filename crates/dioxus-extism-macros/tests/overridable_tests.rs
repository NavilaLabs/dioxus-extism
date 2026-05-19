#[test]
fn compile_tests() {
    let t = trybuild::TestCases::new();
    t.pass("tests/overridable/pass/*.rs");
    t.compile_fail("tests/overridable/fail/*.rs");
}
