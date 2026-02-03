mod common;

use common::*;

#[test]
fn iec_table64() {
    check_no_errors(
        r#"
NAMESPACE Lib
FUNCTION Inc : INT
VAR_INPUT
    x : INT;
END_VAR
Inc := x + INT#1;
END_FUNCTION
END_NAMESPACE

PROGRAM Main
VAR
    y : INT;
END_VAR
y := Lib.Inc(INT#1);
END_PROGRAM
"#,
    );
}

#[test]
fn iec_table66() {
    check_no_errors(
        r#"
NAMESPACE Lib
FUNCTION Inc : INT
VAR_INPUT
    x : INT;
END_VAR
Inc := x + INT#1;
END_FUNCTION
END_NAMESPACE

USING Lib;
PROGRAM Main
VAR
    y : INT;
END_VAR
y := Inc(INT#1);
END_PROGRAM
"#,
    );
}
