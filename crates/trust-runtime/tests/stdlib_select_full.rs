use trust_runtime::stdlib::StandardLibrary;
use trust_runtime::value::{EnumValue, Value};

#[test]
fn selection_full() {
    let lib = StandardLibrary::new();

    assert_eq!(
        lib.call("SEL", &[Value::Bool(true), Value::Int(4), Value::Int(7)])
            .unwrap(),
        Value::Int(7)
    );

    assert_eq!(
        lib.call("MIN", &[Value::Int(3), Value::Int(7), Value::Int(2)])
            .unwrap(),
        Value::Int(2)
    );

    assert_eq!(
        lib.call("MAX", &[Value::Int(3), Value::Int(7), Value::Int(2)])
            .unwrap(),
        Value::Int(7)
    );

    assert_eq!(
        lib.call("LIMIT", &[Value::Int(0), Value::Int(5), Value::Int(3)])
            .unwrap(),
        Value::Int(3)
    );

    assert_eq!(
        lib.call(
            "MUX",
            &[
                Value::Int(1),
                Value::Int(10),
                Value::Int(20),
                Value::Int(30)
            ]
        )
        .unwrap(),
        Value::Int(20)
    );

    let red = Value::Enum(EnumValue {
        type_name: "Color".into(),
        variant_name: "RED".into(),
        numeric_value: 0,
    });
    let green = Value::Enum(EnumValue {
        type_name: "Color".into(),
        variant_name: "GREEN".into(),
        numeric_value: 1,
    });

    assert_eq!(
        lib.call("SEL", &[Value::Bool(true), red.clone(), green.clone()])
            .unwrap(),
        green
    );
}
