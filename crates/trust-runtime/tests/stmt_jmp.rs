use trust_runtime::harness::TestHarness;

#[test]
fn jmp_flow() {
    let source = r#"
        PROGRAM Demo
        VAR
            x: INT := 0;
        END_VAR
        x := INT#1;
        JMP L1;
        x := INT#2;
        L1: x := x + INT#3;
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();
    harness.assert_eq("x", 4i16);
}

#[test]
fn jmp_to_empty_label() {
    let source = r#"
        PROGRAM Demo
        VAR
            x: INT := 0;
        END_VAR
        JMP L1;
        x := INT#1;
        L1: ;
        x := INT#2;
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();
    harness.assert_eq("x", 2i16);
}
