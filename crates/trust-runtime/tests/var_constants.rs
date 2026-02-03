use trust_runtime::harness::TestHarness;

#[test]
fn iec_6_5_4() {
    let source = r#"
PROGRAM Main
VAR CONSTANT
    c : INT := 1;
END_VAR
c := 2;
END_PROGRAM
"#;

    let err = TestHarness::from_source(source)
        .err()
        .expect("expected constant modification error");
    let _ = err;
}
