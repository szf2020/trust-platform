mod common;

use common::*;

#[test]
fn iec_table13() {
    check_no_errors(
        r#"
FUNCTION_BLOCK DemoFb
VAR_INPUT
    i : INT;
END_VAR
VAR_OUTPUT
    o : INT;
END_VAR
VAR_IN_OUT
    io : INT;
END_VAR
VAR_TEMP
    t : INT;
END_VAR
VAR
    v : INT;
END_VAR
END_FUNCTION_BLOCK

PROGRAM Main
VAR_EXTERNAL
    g : INT;
END_VAR
END_PROGRAM

CONFIGURATION Conf
VAR_GLOBAL
    g : INT;
END_VAR
END_CONFIGURATION
"#,
    );
}
