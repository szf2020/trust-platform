use trust_runtime::harness::TestHarness;

#[test]
fn namespace_resolution() {
    let library = r#"
        NAMESPACE Utilities
        FUNCTION Helper : INT
        VAR_INPUT
            x: INT;
        END_VAR
        Helper := x;
        END_FUNCTION
        END_NAMESPACE
    "#;

    let program = r#"
        USING Utilities;
        PROGRAM Multi
        VAR
            count: DINT := 0;
        END_VAR
        count := count + 1;
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_sources(&[library, program]).unwrap();
    harness.cycle();
    harness.assert_eq("count", 1i32);
}

#[test]
fn duplicate_program_name_errors() {
    let first = r#"
        PROGRAM Demo
        VAR
            count: DINT := 0;
        END_VAR
        END_PROGRAM
    "#;
    let second = r#"
        PROGRAM demo
        VAR
            count: DINT := 1;
        END_VAR
        END_PROGRAM
    "#;

    let err = TestHarness::from_sources(&[first, second])
        .err()
        .expect("expected duplicate program error");
    assert!(err.to_string().contains("duplicate PROGRAM name"));
}
