use trust_runtime::harness::TestHarness;
use trust_runtime::value::Value;

#[test]
fn hot_reload_preserves_retained_globals() {
    let source = r#"
CONFIGURATION Conf
VAR_GLOBAL RETAIN
    Retained : INT := 1;
END_VAR
VAR_GLOBAL
    Regular : INT := 2;
END_VAR
PROGRAM P1 : Main;
END_CONFIGURATION

PROGRAM Main
VAR
    temp : INT;
END_VAR
temp := Regular;
END_PROGRAM
"#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.set_input("Retained", Value::Int(42));
    harness.set_input("Regular", Value::Int(99));

    let updated = format!("\n{source}");
    harness.reload_source(&updated).unwrap();

    assert_eq!(harness.get_output("Retained"), Some(Value::Int(42)));
    assert_eq!(harness.get_output("Regular"), Some(Value::Int(2)));
}
