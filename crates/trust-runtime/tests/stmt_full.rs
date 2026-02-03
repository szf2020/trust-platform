use trust_runtime::harness::TestHarness;

#[test]
fn iec_table72() {
    let source = r#"
        FUNCTION Inc : INT
        VAR_INPUT
            x : INT;
        END_VAR
        Inc := x + INT#1;
        END_FUNCTION

        FUNCTION ReturnIfPositive : INT
        VAR_INPUT
            x : INT;
        END_VAR
        IF x > INT#0 THEN
            RETURN x;
        END_IF;
        RETURN INT#0;
        END_FUNCTION

        PROGRAM Test
        VAR
            x : INT := INT#0;
            y : INT := INT#0;
            i : INT := INT#0;
            tmp : INT := INT#0;
            out : INT := INT#0;
        END_VAR

        x := INT#1;

        IF x = INT#0 THEN
            y := INT#1;
        ELSIF x = INT#1 THEN
            y := INT#2;
        ELSE
            y := INT#3;
        END_IF;

        CASE y OF
            INT#1: x := INT#10;
            INT#2: x := INT#20;
        ELSE
            x := INT#30;
        END_CASE;

        FOR i := INT#0 TO INT#2 BY INT#1 DO
            x := x + INT#1;
        END_FOR;

        WHILE i < INT#2 DO
            i := i + INT#1;
        END_WHILE;

        REPEAT
            i := i - INT#1;
        UNTIL i = INT#0 END_REPEAT;

        FOR i := INT#0 TO INT#3 BY INT#1 DO
            IF i = INT#1 THEN
                CONTINUE;
            END_IF;
            IF i = INT#2 THEN
                EXIT;
            END_IF;
            x := x + INT#1;
        END_FOR;

        label1: x := x + INT#1;
        JMP label2;
        x := x + INT#100;
        label2: x := x + INT#1;

        Inc(x);
        tmp := Inc(x);
        out := ReturnIfPositive(tmp);
        ;
        END_PROGRAM
    "#;

    let mut harness = TestHarness::from_source(source).unwrap();
    harness.cycle();
    harness.assert_eq("x", 26i16);
    harness.assert_eq("y", 2i16);
    harness.assert_eq("i", 2i16);
    harness.assert_eq("tmp", 27i16);
    harness.assert_eq("out", 27i16);
}
