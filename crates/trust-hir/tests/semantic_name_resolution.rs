mod common;
use common::*;

// Name Resolution Tests
#[test]
// IEC 61131-3 Ed.3 Section 6.5.2.2 (scope rules)
fn test_undefined_variable() {
    check_has_error(
        r#"
PROGRAM Test
    VAR x : INT; END_VAR
    y := 10;
END_PROGRAM
"#,
        DiagnosticCode::UndefinedVariable,
    );
}

#[test]
fn test_variable_in_scope() {
    // Use an explicit integer type in the declaration.
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : DINT; END_VAR
    x := 10;
END_PROGRAM
"#,
    );
}

#[test]
// IEC 61131-3 Ed.3 Section 6.5.2.2 (duplicate declarations)
fn test_duplicate_declaration() {
    check_has_error(
        r#"
PROGRAM Test
    VAR
        x : INT;
        x : BOOL;
    END_VAR
END_PROGRAM
"#,
        DiagnosticCode::DuplicateDeclaration,
    );
}

#[test]
fn test_invalid_identifier() {
    check_has_error(
        r#"
PROGRAM Test
    VAR __bad : INT; END_VAR
END_PROGRAM
"#,
        DiagnosticCode::InvalidIdentifier,
    );
}

#[test]
fn test_multiple_variables_same_type() {
    // Use an explicit integer type in the declaration.
    check_no_errors(
        r#"
PROGRAM Test
    VAR a, b, c : DINT; END_VAR
    a := 1;
    b := 2;
    c := 3;
END_PROGRAM
"#,
    );
}
