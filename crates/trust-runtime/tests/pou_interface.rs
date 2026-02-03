use trust_runtime::harness::TestHarness;

#[test]
fn interface_conformance() {
    let source = r#"
INTERFACE ICounter
METHOD Inc : INT
VAR_INPUT
    delta : INT;
END_VAR
END_METHOD
END_INTERFACE

CLASS Counter IMPLEMENTS ICounter
VAR PUBLIC
    value : INT := INT#0;
END_VAR
METHOD PUBLIC Inc : INT
VAR_INPUT
    delta : INT;
END_VAR
value := value + delta;
Inc := value;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    c : Counter;
    i : ICounter;
    out1 : INT := INT#0;
    out2 : INT := INT#0;
END_VAR
i := c;
out1 := i.Inc(INT#1);
out2 := c.Inc(INT#2);
END_PROGRAM
"#;

    let mut harness = TestHarness::from_source(source).unwrap();
    let result = harness.cycle();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    harness.assert_eq("out1", 1i16);
    harness.assert_eq("out2", 3i16);
}
