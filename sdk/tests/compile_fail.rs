mod test_program;

#[test]
fn compile_test() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/account_desc_name.rs");
    t.compile_fail("tests/compile-fail/accounts_list.rs");
    t.compile_fail("tests/compile-fail/accounts_list_missing.rs");
    t.compile_fail("tests/compile-fail/account_optional_multiple.rs");
    t.compile_fail("tests/compile-fail/account_tag.rs");
    t.compile_fail("tests/compile-fail/unnamed_fields.rs");
}
