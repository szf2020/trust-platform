use trust_runtime::harness::TestHarness;
use trust_runtime::value::Value;

#[test]
fn access_path_mapping() {
    let source = r#"
TYPE S :
STRUCT
    x : INT;
END_STRUCT
END_TYPE

PROGRAM Main
VAR
    out AT %Q* : BOOL;
    arr : ARRAY[0..1] OF INT;
    st : S;
    out_arr : INT;
    out_st : INT;
END_VAR
out := TRUE;
A1 := INT#10;
out_arr := arr[1];
out_st := st.x;
END_PROGRAM

CONFIGURATION Conf
PROGRAM P1 : Main;
VAR_ACCESS
    A1 : P1.arr[1] : INT READ_WRITE;
END_VAR
VAR_CONFIG
    out AT %QX0.1 : BOOL;
    P1.st.x : INT := INT#42;
END_VAR
END_CONFIGURATION
"#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();

    assert_eq!(harness.get_output("out_arr"), Some(Value::Int(10)));
    assert_eq!(harness.get_output("out_st"), Some(Value::Int(42)));
    assert_eq!(
        harness.get_direct_output("%QX0.1").unwrap(),
        Value::Bool(true)
    );
}
