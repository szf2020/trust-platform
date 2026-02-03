use trust_runtime::stdlib::StandardLibrary;
use trust_runtime::value::{EnumValue, Value};

#[test]
fn enum_and_validate() {
    let lib = StandardLibrary::new();

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
        lib.call("EQ", &[red.clone(), red.clone()]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        lib.call("NE", &[red.clone(), green.clone()]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        lib.call("IS_VALID", &[Value::Real(1.0)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        lib.call("IS_VALID", &[Value::Real(f32::NAN)]).unwrap(),
        Value::Bool(false)
    );

    assert_eq!(
        lib.call("IS_VALID_BCD", &[Value::Word(0x1234)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        lib.call("IS_VALID_BCD", &[Value::Word(0x12FA)]).unwrap(),
        Value::Bool(false)
    );
}
