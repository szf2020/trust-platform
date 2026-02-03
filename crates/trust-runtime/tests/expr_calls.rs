use trust_runtime::harness::TestHarness;

#[test]
fn function_call_expr() {
    let source = r#"
        FUNCTION Add : INT
        VAR_INPUT
            a : INT;
            b : INT;
        END_VAR
        Add := a + b;
        END_FUNCTION

        PROGRAM Test
        VAR
            res : INT := 0;
        END_VAR
        res := Add(INT#2, INT#3);
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();
    harness.assert_eq("res", 5i16);
}

#[test]
fn function_call_named_args() {
    let source = r#"
        FUNCTION Add : INT
        VAR_INPUT
            a : INT;
            b : INT;
        END_VAR
        Add := a + b;
        END_FUNCTION

        PROGRAM Test
        VAR
            res : INT := 0;
        END_VAR
        res := Add(b := INT#2, a := INT#3);
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();
    harness.assert_eq("res", 5i16);
}

#[test]
fn function_call_output_positional() {
    let source = r#"
        FUNCTION WithOut : INT
        VAR_INPUT
            a : INT;
        END_VAR
        VAR_OUTPUT
            out1 : INT;
        END_VAR
        out1 := a;
        WithOut := out1;
        END_FUNCTION

        PROGRAM Test
        VAR
            a : INT := INT#4;
            out1 : INT := 0;
        END_VAR
        WithOut(a, out1);
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();
    harness.assert_eq("out1", 4i16);
}

#[test]
fn function_block_call() {
    let source = r#"
        FUNCTION_BLOCK Counter
        VAR_INPUT
            inc : BOOL;
        END_VAR
        VAR_OUTPUT
            value : INT;
        END_VAR
        VAR
            count : INT := INT#0;
        END_VAR
        IF inc THEN
            count := count + INT#1;
        END_IF;
        value := count;
        END_FUNCTION_BLOCK

        PROGRAM Test
        VAR
            fb : Counter;
            out : INT := 0;
        END_VAR
        fb(inc := TRUE, value => out);
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();
    harness.assert_eq("out", 1i16);
}

#[test]
fn stdlib_named_args() {
    let source = r#"
        PROGRAM Test
        VAR
            out : INT := 0;
        END_VAR
        out := SEL(G := TRUE, IN0 := INT#4, IN1 := INT#7);
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();
    harness.assert_eq("out", 7i16);
}
