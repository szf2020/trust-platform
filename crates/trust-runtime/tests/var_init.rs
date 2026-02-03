use trust_runtime::harness::TestHarness;
use trust_runtime::value::Value;

#[test]
fn iec_table14() {
    let source = r#"
PROGRAM Main
VAR
    a : INT;
    b : INT := 4;
    c : INT := b + 1;
END_VAR
END_PROGRAM
"#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();

    assert_eq!(harness.get_output("a"), Some(Value::Int(0)));
    assert_eq!(harness.get_output("b"), Some(Value::Int(4)));
    assert_eq!(harness.get_output("c"), Some(Value::Int(5)));
}
