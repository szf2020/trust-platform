mod common;
use common::*;

// Type Checking Tests
#[test]
fn test_constant_modification() {
    check_has_error(
        r#"
PROGRAM Test
    VAR CONSTANT
        PI : REAL := 3.14159;
    END_VAR
    PI := 3.0;
END_PROGRAM
"#,
        DiagnosticCode::ConstantModification,
    );
}

#[test]
fn test_constant_struct_field_modification() {
    check_has_error(
        r#"
TYPE
    MyStruct : STRUCT
        field : INT;
    END_STRUCT;
END_TYPE

PROGRAM Test
    VAR CONSTANT
        s : MyStruct;
    END_VAR
    s.field := 1;
END_PROGRAM
"#,
        DiagnosticCode::ConstantModification,
    );
}

#[test]
fn test_boolean_condition_required() {
    check_has_error(
        r#"
PROGRAM Test
    VAR x : DINT; END_VAR
    IF x THEN
        x := 1;
    END_IF;
END_PROGRAM
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_boolean_condition_ok() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : BOOL; END_VAR
    IF x THEN
        x := FALSE;
    END_IF;
END_PROGRAM
"#,
    );
}

#[test]
fn test_contextual_int_literal_assignment() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : INT; END_VAR
    x := 1;
END_PROGRAM
"#,
    );
}

#[test]
fn test_contextual_int_literal_return() {
    check_no_errors(
        r#"
FUNCTION Test : INT
    RETURN 1;
END_FUNCTION
"#,
    );
}

#[test]
fn test_contextual_real_literal_assignment() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : REAL; END_VAR
    x := 1.0;
END_PROGRAM
"#,
    );
}

#[test]
fn test_contextual_real_literal_return() {
    check_no_errors(
        r#"
FUNCTION Test : REAL
    RETURN 1.0;
END_FUNCTION
"#,
    );
}

#[test]
fn test_real_literal_in_real_arithmetic() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR sum : REAL; avg : REAL; END_VAR
    avg := sum / 4.0;
END_PROGRAM
"#,
    );
}

#[test]
fn test_real_literal_in_standard_numeric_function() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : REAL; y : REAL; END_VAR
    y := MIN(x, 4.0);
END_PROGRAM
"#,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 11 (subrange types)
fn test_subrange_assignment_out_of_range() {
    check_has_error(
        r#"
TYPE
    Percent : INT(0..100);
END_TYPE

PROGRAM Test
    VAR p : Percent; END_VAR
    p := INT#150;
END_PROGRAM
"#,
        DiagnosticCode::OutOfRange,
    );
}

#[test]
fn test_subrange_assignment_in_range() {
    check_no_errors(
        r#"
TYPE
    Percent : INT(0..100);
END_TYPE

PROGRAM Test
    VAR p : Percent; END_VAR
    p := INT#42;
END_PROGRAM
"#,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 11 (subrange bounds)
fn test_subrange_bounds_invalid_order() {
    check_has_error(
        r#"
TYPE
    BadRange : INT(10..5);
END_TYPE
"#,
        DiagnosticCode::OutOfRange,
    );
}

#[test]
fn test_subrange_bounds_non_constant() {
    check_has_error(
        r#"
TYPE
    BadRange : INT(A..B);
END_TYPE
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_unused_variable_warning() {
    let warnings = check_warnings(
        r#"
PROGRAM Test
    VAR x : INT; END_VAR
END_PROGRAM
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::UnusedVariable));
}

#[test]
fn test_var_config_marks_symbol_used_across_files() {
    let mut db = Database::new();
    db.set_source_text(
        FileId(0),
        r#"
CONFIGURATION Conf
RESOURCE R ON CPU
    TASK Fast (INTERVAL := T#100ms, PRIORITY := 1);
    PROGRAM P1 WITH Fast : Main;
END_RESOURCE
VAR_CONFIG
    P1.InSignal : BOOL;
END_VAR
END_CONFIGURATION
"#
        .to_string(),
    );
    db.set_source_text(
        FileId(1),
        r#"
PROGRAM Main
VAR
    InSignal : BOOL;
END_VAR
END_PROGRAM
"#
        .to_string(),
    );

    let warnings: Vec<DiagnosticCode> = db
        .diagnostics(FileId(1))
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Warning)
        .map(|d| d.code)
        .collect();

    assert!(
        !warnings.contains(&DiagnosticCode::UnusedVariable),
        "Unexpected unused variable warning: {warnings:?}"
    );
}

#[test]
fn test_unused_parameter_warning() {
    let warnings = check_warnings(
        r#"
FUNCTION Add : INT
    VAR_INPUT
        a : INT;
    END_VAR
    Add := 1;
END_FUNCTION
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::UnusedParameter));
}

#[test]
fn test_implicit_conversion_warning() {
    let warnings = check_warnings(
        r#"
PROGRAM Test
    VAR x : REAL; END_VAR
    x := 1;
END_PROGRAM
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::ImplicitConversion));
}

#[test]
fn test_cyclomatic_complexity_warning() {
    let mut body = String::new();
    for _ in 0..15 {
        body.push_str("    IF TRUE THEN\n        x := x + 1;\n    END_IF;\n");
    }
    let source = format!(
        r#"
PROGRAM Test
    VAR
        x : INT;
    END_VAR
{body}
END_PROGRAM
"#
    );
    let warnings = check_warnings(&source);
    assert!(warnings.contains(&DiagnosticCode::HighComplexity));
}

#[test]
fn test_unused_pou_warning() {
    let warnings = check_warnings(
        r#"
FUNCTION Foo : INT
    Foo := 1;
END_FUNCTION
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::UnusedPou));
}

#[test]
fn test_unreachable_code_warning() {
    let warnings = check_warnings(
        r#"
FUNCTION Foo : INT
VAR
    x : INT;
END_VAR
    Foo := 0;
    RETURN;
    x := 1;
END_FUNCTION
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::UnreachableCode));
}

#[test]
fn test_unreachable_if_false_branch_warning() {
    let warnings = check_warnings(
        r#"
PROGRAM Test
VAR
    x : INT;
END_VAR
    IF FALSE THEN
        x := 1;
    ELSE
        x := 2;
    END_IF;
END_PROGRAM
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::UnreachableCode));
}

#[test]
fn test_unreachable_elsif_false_branch_warning() {
    let warnings = check_warnings(
        r#"
PROGRAM Test
VAR
    x : INT;
END_VAR
    IF FALSE THEN
        x := 1;
    ELSIF FALSE THEN
        x := 2;
    ELSE
        x := 3;
    END_IF;
END_PROGRAM
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::UnreachableCode));
}

#[test]
fn test_nondeterministic_time_date_warning() {
    let warnings = check_warnings(
        r#"
PROGRAM Test
    VAR
        t : TIME;
        d : DATE;
    END_VAR
END_PROGRAM
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::NondeterministicTimeDate));
}

#[test]
fn test_nondeterministic_io_warning() {
    let warnings = check_warnings(
        r#"
PROGRAM Test
    VAR
        input AT %IX0.0 : BOOL;
        output AT %QW1 : INT;
    END_VAR
END_PROGRAM
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::NondeterministicIo));
}

#[test]
fn test_shared_global_task_hazard_warning() {
    let warnings = check_warnings(
        r#"
CONFIGURATION Conf
VAR_GLOBAL
    Shared : INT;
END_VAR
RESOURCE R ON CPU
    TASK Fast (INTERVAL := T#10ms, PRIORITY := 1);
    TASK Slow (INTERVAL := T#20ms, PRIORITY := 2);
    PROGRAM P1 WITH Fast : Writer;
    PROGRAM P2 WITH Slow : Reader;
END_RESOURCE
END_CONFIGURATION

PROGRAM Writer
    Shared := Shared + 1;
END_PROGRAM

PROGRAM Reader
    VAR x : INT; END_VAR
    x := Shared;
END_PROGRAM
"#,
    );
    assert!(warnings.contains(&DiagnosticCode::SharedGlobalTaskHazard));
}

#[test]
fn test_shared_global_task_hazard_single_task_no_warning() {
    let warnings = check_warnings(
        r#"
CONFIGURATION Conf
VAR_GLOBAL
    Shared : INT;
END_VAR
RESOURCE R ON CPU
    TASK Fast (INTERVAL := T#10ms, PRIORITY := 1);
    PROGRAM P1 WITH Fast : Writer;
    PROGRAM P2 WITH Fast : Reader;
END_RESOURCE
END_CONFIGURATION

PROGRAM Writer
    Shared := Shared + 1;
END_PROGRAM

PROGRAM Reader
    VAR x : INT; END_VAR
    x := Shared;
END_PROGRAM
"#,
    );
    assert!(!warnings.contains(&DiagnosticCode::SharedGlobalTaskHazard));
}

#[test]
fn test_used_function_no_unused_pou_warning() {
    let warnings = check_warnings(
        r#"
CONFIGURATION Conf
RESOURCE R ON CPU
    TASK Fast (INTERVAL := T#10ms, PRIORITY := 1);
    PROGRAM P1 WITH Fast : Main;
END_RESOURCE
END_CONFIGURATION

FUNCTION Foo : INT
    Foo := 1;
END_FUNCTION

PROGRAM Main
VAR
    x : INT;
END_VAR
    x := Foo();
END_PROGRAM
"#,
    );
    assert!(
        !warnings.contains(&DiagnosticCode::UnusedPou),
        "Unexpected unused POU warning: {warnings:?}"
    );
}

#[test]
fn test_function_block_used_as_type_no_unused_pou_warning() {
    let warnings = check_warnings(
        r#"
CONFIGURATION Conf
RESOURCE R ON CPU
    TASK Fast (INTERVAL := T#10ms, PRIORITY := 1);
    PROGRAM P1 WITH Fast : Main;
END_RESOURCE
END_CONFIGURATION

FUNCTION_BLOCK FB
VAR
    x : INT;
END_VAR
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    inst : FB;
END_VAR
END_PROGRAM
"#,
    );
    assert!(
        !warnings.contains(&DiagnosticCode::UnusedPou),
        "Unexpected unused POU warning: {warnings:?}"
    );
}

#[test]
fn test_direct_address_usage() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : BOOL; END_VAR
    x := %IX0.0;
END_PROGRAM
"#,
    );
}

#[test]
fn test_direct_address_type_mismatch() {
    check_has_error(
        r#"
PROGRAM Test
    VAR x : BOOL; END_VAR
    x := %IW0;
END_PROGRAM
"#,
        DiagnosticCode::IncompatibleAssignment,
    );
}

#[test]
fn test_direct_address_binding_recorded() {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(
        file,
        r#"
PROGRAM Test
    VAR x AT %IX0.0 : BOOL; END_VAR
END_PROGRAM
"#
        .to_string(),
    );

    let symbols = db.file_symbols(file);
    let x = symbols.iter().find(|s| s.name == "x").unwrap();
    assert_eq!(x.direct_address.as_deref(), Some("%IX0.0"));
}

#[test]
fn test_invalid_assignment_target_field_of_call() {
    check_has_error(
        r#"
TYPE
    MyStruct : STRUCT
        field : INT;
    END_STRUCT;
END_TYPE

FUNCTION GetStruct : MyStruct
END_FUNCTION

PROGRAM Test
    GetStruct().field := 1;
END_PROGRAM
"#,
        DiagnosticCode::InvalidAssignmentTarget,
    );
}

#[test]
fn test_var_input_assignment_error() {
    check_has_error(
        r#"
FUNCTION FB_Test : INT
    VAR_INPUT
        InVal : INT;
    END_VAR
    InVal := 1;
    FB_Test := InVal;
END_FUNCTION
"#,
        DiagnosticCode::InvalidAssignmentTarget,
    );
}

#[test]
fn test_assignment_to_function_name_error() {
    check_has_error(
        r#"
FUNCTION Add : DINT
    Add := 1;
END_FUNCTION

PROGRAM Test
    Add := 2;
END_PROGRAM
"#,
        DiagnosticCode::InvalidAssignmentTarget,
    );
}

#[test]
fn test_assignment_to_this_error() {
    check_has_error(
        r#"
CLASS Example
    METHOD SetValue
        THIS := 1;
    END_METHOD
END_CLASS
"#,
        DiagnosticCode::InvalidAssignmentTarget,
    );
}

#[test]
fn test_property_without_setter_assignment_error() {
    check_has_error(
        r#"
FUNCTION_BLOCK FB_Test
    PROPERTY Value : INT
    GET
        RETURN 1;
    END_GET
    END_PROPERTY

    METHOD Update
        Value := 2;
    END_METHOD
END_FUNCTION_BLOCK
"#,
        DiagnosticCode::InvalidAssignmentTarget,
    );
}

#[test]
fn test_property_get_return_type_checked() {
    check_has_error(
        r#"
FUNCTION_BLOCK FB_Test
    PROPERTY Value : INT
    GET
        RETURN TRUE;
    END_GET
    END_PROPERTY
END_FUNCTION_BLOCK
"#,
        DiagnosticCode::InvalidReturnType,
    );
}

#[test]
fn test_property_set_rejects_return_value() {
    check_has_error(
        r#"
FUNCTION_BLOCK FB_Test
    PROPERTY Value : INT
    SET
        RETURN 1;
    END_SET
    END_PROPERTY
END_FUNCTION_BLOCK
"#,
        DiagnosticCode::InvalidReturnType,
    );
}

#[test]
fn test_function_missing_return_value() {
    check_has_error(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
    END_VAR
END_FUNCTION
"#,
        DiagnosticCode::MissingReturn,
    );
}

#[test]
fn test_function_assignment_sets_return_value() {
    check_no_errors(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
    END_VAR
    Add := a;
END_FUNCTION
"#,
    );
}

#[test]
fn test_function_return_expr_sets_return_value() {
    check_no_errors(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
    END_VAR
    RETURN a;
END_FUNCTION
"#,
    );
}

#[test]
fn test_array_bounds_constant_expression() {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(
        file,
        r#"
FUNCTION_BLOCK FB_Test
    VAR CONSTANT
        Max : DINT := 5;
    END_VAR
    VAR
        arr : ARRAY[0..Max + 1] OF INT;
    END_VAR
END_FUNCTION_BLOCK
"#
        .to_string(),
    );

    let symbols = db.file_symbols(file);
    let arr = symbols.iter().find(|s| s.name == "arr").unwrap();
    let type_id = symbols.resolve_alias_type(arr.type_id);
    let Type::Array { dimensions, .. } = symbols.type_by_id(type_id).unwrap() else {
        panic!("expected array type");
    };
    assert_eq!(dimensions, &vec![(0, 6)]);
}

#[test]
fn test_array_bounds_enum_values() {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(
        file,
        r#"
TYPE Level : (Low := 1, High := 3)
END_TYPE

PROGRAM Test
    VAR
        arr : ARRAY[Low..High] OF INT;
    END_VAR
END_PROGRAM
"#
        .to_string(),
    );

    let symbols = db.file_symbols(file);
    let arr = symbols.iter().find(|s| s.name == "arr").unwrap();
    let type_id = symbols.resolve_alias_type(arr.type_id);
    let Type::Array { dimensions, .. } = symbols.type_by_id(type_id).unwrap() else {
        panic!("expected array type");
    };
    assert_eq!(dimensions, &vec![(1, 3)]);
}

#[test]
fn test_array_index_literal_out_of_bounds() {
    check_has_error(
        r#"
PROGRAM Test
    VAR arr : ARRAY[0..3] OF DINT; END_VAR
    arr[4] := 1;
END_PROGRAM
"#,
        DiagnosticCode::OutOfRange,
    );
}

#[test]
// IEC 61131-3 Ed.3 Tables 11, 15-16 (array bounds and indexing)
fn test_array_index_subrange_out_of_bounds() {
    check_has_error(
        r#"
TYPE Idx : INT(0..5);
END_TYPE

PROGRAM Test
    VAR i : Idx; arr : ARRAY[0..3] OF DINT; END_VAR
    arr[i] := 1;
END_PROGRAM
"#,
        DiagnosticCode::OutOfRange,
    );
}

#[test]
fn test_array_index_subrange_within_bounds() {
    check_no_errors(
        r#"
TYPE Idx : INT(1..3);
END_TYPE

PROGRAM Test
    VAR i : Idx; arr : ARRAY[1..3] OF DINT; END_VAR
    arr[i] := 1;
END_PROGRAM
"#,
    );
}

#[test]
fn test_array_index_dimension_too_many() {
    check_has_error(
        r#"
PROGRAM Test
    VAR arr : ARRAY[0..3] OF DINT; END_VAR
    arr[1, 2] := 1;
END_PROGRAM
"#,
        DiagnosticCode::InvalidArrayIndex,
    );
}

#[test]
fn test_array_index_dimension_too_few() {
    check_has_error(
        r#"
PROGRAM Test
    VAR arr : ARRAY[0..3, 1..2] OF DINT; END_VAR
    arr[1] := 1;
END_PROGRAM
"#,
        DiagnosticCode::InvalidArrayIndex,
    );
}

#[test]
fn test_array_index_requires_integer() {
    check_has_error(
        r#"
PROGRAM Test
    VAR arr : ARRAY[0..3] OF DINT; idx : REAL; END_VAR
    arr[idx] := 1;
END_PROGRAM
"#,
        DiagnosticCode::InvalidArrayIndex,
    );
}

#[test]
// IEC 61131-3 Ed.3 Tables 13-16 (VAR_ACCESS typing)
fn test_var_access_type_mismatch() {
    check_has_error(
        r#"
CONFIGURATION Conf
VAR_GLOBAL
    G : INT;
END_VAR
VAR_ACCESS
    A : G : DINT READ_WRITE;
END_VAR
END_CONFIGURATION
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_var_access_read_only_rejects_assignment() {
    check_has_error(
        r#"
CONFIGURATION Conf
VAR_GLOBAL
    G : INT;
END_VAR
VAR_ACCESS
    A : G : INT READ_ONLY;
END_VAR
END_CONFIGURATION

PROGRAM Test
    A := 1;
END_PROGRAM
"#,
        DiagnosticCode::ConstantModification,
    );
}

#[test]
fn test_var_config_type_mismatch() {
    check_has_error(
        r#"
CONFIGURATION Conf
VAR_GLOBAL
    G : INT;
END_VAR
VAR_CONFIG
    G : DINT := 1;
END_VAR
END_CONFIGURATION
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_var_config_rejects_constant_init() {
    check_has_error(
        r#"
CONFIGURATION Conf
VAR_GLOBAL CONSTANT
    G : INT := 1;
END_VAR
VAR_CONFIG
    G : INT := 2;
END_VAR
END_CONFIGURATION
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_task_missing_priority_error() {
    check_has_error(
        r#"
CONFIGURATION Conf
RESOURCE R ON CPU
    TASK Fast (INTERVAL := T#10ms);
    PROGRAM P1 WITH Fast : Main;
END_RESOURCE
END_CONFIGURATION
"#,
        DiagnosticCode::InvalidTaskConfig,
    );
}

#[test]
fn test_task_single_requires_bool_literal() {
    check_has_error(
        r#"
CONFIGURATION Conf
RESOURCE R ON CPU
    TASK Event (SINGLE := 1, PRIORITY := 1);
    PROGRAM P1 WITH Event : Main;
END_RESOURCE
END_CONFIGURATION
"#,
        DiagnosticCode::InvalidTaskConfig,
    );
}

#[test]
fn test_task_interval_requires_time_literal() {
    check_has_error(
        r#"
CONFIGURATION Conf
RESOURCE R ON CPU
    TASK Fast (INTERVAL := 1, PRIORITY := 1);
    PROGRAM P1 WITH Fast : Main;
END_RESOURCE
END_CONFIGURATION
"#,
        DiagnosticCode::InvalidTaskConfig,
    );
}

#[test]
fn test_program_with_unknown_task_error() {
    check_has_error(
        r#"
CONFIGURATION Conf
RESOURCE R ON CPU
    TASK Fast (INTERVAL := T#10ms, PRIORITY := 1);
    PROGRAM P1 WITH Missing : Main;
END_RESOURCE
END_CONFIGURATION
"#,
        DiagnosticCode::UnknownTask,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 13 (VAR_EXTERNAL linkage)
fn test_var_external_missing_global() {
    check_has_error(
        r#"
PROGRAM Test
VAR_EXTERNAL
    G : INT;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::UndefinedVariable,
    );
}

#[test]
fn test_var_external_type_mismatch() {
    check_has_error(
        r#"
CONFIGURATION Conf
VAR_GLOBAL
    G : INT;
END_VAR
END_CONFIGURATION

PROGRAM Test
VAR_EXTERNAL
    G : DINT;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_var_external_requires_constant_for_global_constant() {
    check_has_error(
        r#"
CONFIGURATION Conf
VAR_GLOBAL CONSTANT
    G : INT := 1;
END_VAR
END_CONFIGURATION

PROGRAM Test
VAR_EXTERNAL
    G : INT;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_var_external_rejects_initializer() {
    check_has_error(
        r#"
CONFIGURATION Conf
VAR_GLOBAL
    G : INT;
END_VAR
END_CONFIGURATION

PROGRAM Test
VAR_EXTERNAL
    G : INT := 1;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
// IEC 61131-3 Ed.3 Section 6.5.6 (RETAIN/NON_RETAIN qualifiers)
fn test_var_retain_non_retain_conflict() {
    check_has_error(
        r#"
PROGRAM Test
VAR RETAIN NON_RETAIN
    X : INT;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_var_retain_not_allowed_in_in_out() {
    check_has_error(
        r#"
FUNCTION_BLOCK FB
VAR_IN_OUT RETAIN
    X : INT;
END_VAR
END_FUNCTION_BLOCK
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_var_constant_retain_conflict() {
    check_has_error(
        r#"
PROGRAM Test
VAR CONSTANT RETAIN
    X : INT := 1;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_var_persistent_allowed() {
    check_no_errors(
        r#"
PROGRAM Test
VAR PERSISTENT
    X : INT := 1;
END_VAR
END_PROGRAM
"#,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 16 (AT binding restrictions)
fn test_at_wildcard_not_allowed_in_var_input() {
    check_has_error(
        r#"
PROGRAM Test
VAR_INPUT
    Inp AT %I*: BOOL;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_at_wildcard_requires_var_config() {
    check_has_error(
        r#"
PROGRAM Test
VAR
    Out AT %Q*: BOOL;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_at_wildcard_var_config_requires_full_address() {
    check_has_error(
        r#"
CONFIGURATION Conf
VAR_CONFIG
    Out AT %Q*: BOOL;
END_VAR
END_CONFIGURATION

PROGRAM Test
VAR
    Out AT %Q*: BOOL;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_at_wildcard_var_config_mapping_ok() {
    check_no_errors(
        r#"
CONFIGURATION Conf
VAR_CONFIG
    Out AT %QW0: BOOL;
END_VAR
END_CONFIGURATION

PROGRAM Test
VAR
    Out AT %Q*: BOOL;
END_VAR
END_PROGRAM
"#,
    );
}

#[test]
fn test_var_config_nested_access() {
    check_no_errors(
        r#"
CONFIGURATION Conf
VAR_CONFIG
    P1.fb.out AT %QX0.1 : BOOL;
END_VAR
PROGRAM P1 : Main;
END_CONFIGURATION

FUNCTION_BLOCK FB
VAR_OUTPUT
    out AT %Q*: BOOL;
END_VAR
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    fb : FB;
END_VAR
END_PROGRAM
"#,
    );
}

#[test]
fn test_string_length_constant_expression() {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(
        file,
        r#"
FUNCTION_BLOCK FB_Test
    VAR CONSTANT
        Len : DINT := 4;
    END_VAR
    VAR
        name : STRING[Len + 1];
    END_VAR
END_FUNCTION_BLOCK
"#
        .to_string(),
    );

    let symbols = db.file_symbols(file);
    let name = symbols.iter().find(|s| s.name == "name").unwrap();
    let type_id = symbols.resolve_alias_type(name.type_id);
    let Type::String { max_len } = symbols.type_by_id(type_id).unwrap() else {
        panic!("expected string type");
    };
    assert_eq!(*max_len, Some(5));
}

#[test]
fn test_string_literal_length_in_initializer() {
    check_has_error(
        r#"
PROGRAM Test
    VAR
        s : STRING[3] := 'ABCD';
    END_VAR
END_PROGRAM
"#,
        DiagnosticCode::OutOfRange,
    );
}

#[test]
fn test_string_literal_length_in_assignment() {
    check_has_error(
        r#"
PROGRAM Test
    VAR s : STRING[2]; END_VAR
    s := 'ABC';
END_PROGRAM
"#,
        DiagnosticCode::OutOfRange,
    );
}

#[test]
fn test_string_length_assignment_between_lengths() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR
        short : STRING[5];
        long : STRING[20];
    END_VAR
    short := long;
END_PROGRAM
"#,
    );
}

#[test]
fn test_type_alias_numeric_ops() {
    check_no_errors(
        r#"
TYPE MyInt : DINT;
END_TYPE

PROGRAM Test
    VAR x : MyInt; END_VAR
    x := 1;
    x := x + 1;
END_PROGRAM
"#,
    );
}

#[test]
fn test_sizeof_expression() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR size : DINT; x : DINT; END_VAR
    size := SIZEOF(x);
    size := SIZEOF(DINT);
END_PROGRAM
"#,
    );
}

#[test]
fn test_method_call_on_instance() {
    check_no_errors(
        r#"
FUNCTION_BLOCK Counter
    METHOD Get : DINT
        Get := 1;
    END_METHOD
END_FUNCTION_BLOCK

PROGRAM Test
    VAR fb : Counter; value : DINT; END_VAR
    value := fb.Get();
END_PROGRAM
"#,
    );
}

#[test]
fn test_adr_requires_lvalue() {
    check_has_error(
        r#"
PROGRAM Test
    VAR p : POINTER TO DINT; END_VAR
    p := ADR(1);
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 12 (reference operators)
fn test_ref_returns_reference() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : DINT; r : REF_TO DINT; END_VAR
    r := REF(x);
    r^ := 10;
END_PROGRAM
"#,
    );
}

#[test]
fn test_ref_requires_lvalue() {
    check_has_error(
        r#"
PROGRAM Test
    VAR r : REF_TO INT; END_VAR
    r := REF(1);
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_ref_rejects_constant() {
    check_has_error(
        r#"
PROGRAM Test
    VAR CONSTANT c : INT := 1; END_VAR
    VAR r : REF_TO INT; END_VAR
    r := REF(c);
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_ref_rejects_temp_variable() {
    check_has_error(
        r#"
PROGRAM Test
    VAR_TEMP
        t : INT;
    END_VAR
    VAR
        r : REF_TO INT;
    END_VAR
    r := REF(t);
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_ref_rejects_function_local_variable() {
    check_has_error(
        r#"
FUNCTION Foo : INT
    VAR
        x : INT;
        r : REF_TO INT;
    END_VAR
    r := REF(x);
    Foo := x;
END_FUNCTION
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_null_assignment_to_reference() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR r : REF_TO INT; END_VAR
    r := NULL;
END_PROGRAM
"#,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 12 (reference assignment)
fn test_ref_assignment_requires_reference_target() {
    check_has_error(
        r#"
PROGRAM Test
    VAR x : INT; y : INT; END_VAR
    x ?= y;
END_PROGRAM
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_ref_assignment_requires_reference_source() {
    check_has_error(
        r#"
PROGRAM Test
    VAR r : REF_TO INT; x : INT; END_VAR
    r ?= x;
END_PROGRAM
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_ref_assignment_allows_reference_source() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : INT; r1 : REF_TO INT; r2 : REF_TO INT; END_VAR
    r1 := REF(x);
    r2 ?= r1;
END_PROGRAM
"#,
    );
}

#[test]
fn test_null_comparison_reference() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR r : REF_TO INT; END_VAR
    IF r = NULL THEN
    END_IF;
END_PROGRAM
"#,
    );
}

#[test]
fn test_for_loop_bounds_integer() {
    check_has_error(
        r#"
PROGRAM Test
    VAR i : INT; x : REAL; END_VAR
    FOR i := x TO 10 DO
    END_FOR;
END_PROGRAM
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_case_selector_requires_elementary() {
    check_has_error(
        r#"
TYPE S : STRUCT
    x : INT;
END_STRUCT
END_TYPE

PROGRAM Test
    VAR s : S; END_VAR
    CASE s OF
        1: s.x := 1;
    END_CASE;
END_PROGRAM
"#,
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn test_case_label_requires_literal_or_constant() {
    check_has_error(
        r#"
PROGRAM Test
    VAR x : INT; y : INT; END_VAR
    CASE x OF
        y: x := 1;
    END_CASE;
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_case_subrange_requires_literal_bounds() {
    check_has_error(
        r#"
PROGRAM Test
    VAR x : INT; y : INT; END_VAR
    CASE x OF
        y..5: x := 1;
    END_CASE;
END_PROGRAM
"#,
        DiagnosticCode::InvalidOperation,
    );
}

#[test]
fn test_case_enum_label_ok() {
    check_no_errors(
        r#"
TYPE Color : (Red, Green, Blue)
END_TYPE

PROGRAM Test
    VAR c : Color; END_VAR
    CASE c OF
        Red: c := Green;
        Blue: c := Red;
        ELSE
            c := Blue;
    END_CASE;
END_PROGRAM
"#,
    );
}

#[test]
fn test_case_missing_else_warning() {
    let warnings = check_warnings(
        r#"
PROGRAM Test
    VAR x : INT; END_VAR
    CASE x OF
        1: x := 1;
    END_CASE;
END_PROGRAM
"#,
    );
    assert!(
        warnings.contains(&DiagnosticCode::MissingElse),
        "Expected MissingElse warning, got: {:?}",
        warnings
    );
}

#[test]
fn test_case_enum_exhaustive_no_warning() {
    let warnings = check_warnings(
        r#"
TYPE Mode : (Off, Manual, Auto)
END_TYPE

PROGRAM Test
    VAR m : Mode; END_VAR
    CASE m OF
        Mode#Off: m := Mode#Manual;
        Mode#Manual: m := Mode#Auto;
        Mode#Auto: m := Mode#Off;
    END_CASE;
END_PROGRAM
"#,
    );
    assert!(
        !warnings.contains(&DiagnosticCode::MissingElse),
        "Expected no MissingElse warning, got: {:?}",
        warnings
    );
}

#[test]
fn test_named_argument_order() {
    check_has_error(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
        b : DINT;
    END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Test
    VAR result : DINT; END_VAR
    result := Add(a := 1, 2);
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
fn test_named_argument_order_allows_positional_first() {
    check_no_errors(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
        b : DINT;
    END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Test
    VAR result : DINT; END_VAR
    result := Add(1, b := 2);
END_PROGRAM
"#,
    );
}

#[test]
fn test_output_parameter_connection_ok() {
    check_no_errors(
        r#"
FUNCTION WithOut : DINT
    VAR_INPUT
        a : DINT;
    END_VAR
    VAR_OUTPUT
        out1 : DINT;
    END_VAR
    out1 := a + 1;
    WithOut := out1;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; out1 : DINT; END_VAR
    WithOut(a := a, out1 => out1);
END_PROGRAM
"#,
    );
}

#[test]
fn test_output_parameter_requires_arrow() {
    check_has_error(
        r#"
FUNCTION WithOut : DINT
    VAR_INPUT
        a : DINT;
    END_VAR
    VAR_OUTPUT
        out1 : DINT;
    END_VAR
    out1 := a + 1;
    WithOut := out1;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; out1 : DINT; END_VAR
    WithOut(a := a, out1 := out1);
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
fn test_output_connection_rejects_input_parameter() {
    check_has_error(
        r#"
FUNCTION WithOut : DINT
    VAR_INPUT
        a : DINT;
    END_VAR
    VAR_OUTPUT
        out1 : DINT;
    END_VAR
    out1 := a + 1;
    WithOut := out1;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; out1 : DINT; END_VAR
    WithOut(a => out1);
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
fn test_formal_call_allows_missing_arguments() {
    check_no_errors(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
        b : DINT;
    END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; res : DINT; END_VAR
    res := Add(a := a);
END_PROGRAM
"#,
    );
}

#[test]
fn test_formal_call_requires_in_out_binding() {
    check_has_error(
        r#"
FUNCTION UseInOut : DINT
    VAR_IN_OUT
        x : DINT;
    END_VAR
    UseInOut := x;
END_FUNCTION

PROGRAM Test
    VAR res : DINT; END_VAR
    res := UseInOut();
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
fn test_formal_call_duplicate_parameter_error() {
    check_has_error(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
        b : DINT;
    END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Test
    VAR res : DINT; END_VAR
    res := Add(a := 1, a := 2);
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
fn test_formal_call_unknown_parameter_error() {
    check_has_error(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
        b : DINT;
    END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Test
    VAR res : DINT; END_VAR
    res := Add(c := 1, a := 2);
END_PROGRAM
"#,
        DiagnosticCode::CannotResolve,
    );
}

#[test]
fn test_non_formal_call_requires_complete_arguments() {
    check_has_error(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
        b : DINT;
    END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; res : DINT; END_VAR
    res := Add(a);
END_PROGRAM
"#,
        DiagnosticCode::WrongArgumentCount,
    );
}

#[test]
fn test_non_formal_call_skips_en_eno() {
    check_no_errors(
        r#"
FUNCTION WithEn : DINT
    VAR_INPUT
        EN : BOOL;
        a : DINT;
        b : DINT;
    END_VAR
    VAR_OUTPUT
        ENO : BOOL;
    END_VAR
    WithEn := a + b;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; b : DINT; res : DINT; END_VAR
    res := WithEn(a, b);
END_PROGRAM
"#,
    );
}

#[test]
fn test_non_formal_call_rejects_en_eno_positional() {
    check_has_error(
        r#"
FUNCTION WithEn : DINT
    VAR_INPUT
        EN : BOOL;
        a : DINT;
        b : DINT;
    END_VAR
    VAR_OUTPUT
        ENO : BOOL;
    END_VAR
    WithEn := a + b;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; b : DINT; res : DINT; END_VAR
    res := WithEn(TRUE, a, b);
END_PROGRAM
"#,
        DiagnosticCode::WrongArgumentCount,
    );
}

#[test]
fn test_non_formal_call_allows_output_positional() {
    check_no_errors(
        r#"
FUNCTION WithOut : DINT
    VAR_INPUT
        a : DINT;
    END_VAR
    VAR_OUTPUT
        out1 : DINT;
    END_VAR
    out1 := a;
    WithOut := out1;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; out1 : DINT; END_VAR
    WithOut(a, out1);
END_PROGRAM
"#,
    );
}

#[test]
fn test_call_rejects_ref_assign_argument() {
    check_has_error(
        r#"
FUNCTION Add : DINT
    VAR_INPUT
        a : DINT;
    END_VAR
    Add := a;
END_FUNCTION

PROGRAM Test
    VAR a : DINT; b : DINT; END_VAR
    Add(a ?= b);
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
fn test_function_block_instance_call() {
    check_no_errors(
        r#"
FUNCTION_BLOCK Counter
    VAR_INPUT
        Enable : BOOL;
    END_VAR
END_FUNCTION_BLOCK

PROGRAM Test
    VAR fb : Counter; END_VAR
    fb(Enable := TRUE);
END_PROGRAM
"#,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 46 + Figure 15 (timer function blocks)
fn test_standard_timer_function_block_call() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR timer : TON; done : BOOL; elapsed : TIME; END_VAR
    timer(IN := TRUE, PT := T#1s);
    timer(IN := FALSE);
    done := timer.Q;
    elapsed := timer.ET;
END_PROGRAM
"#,
    );
}

#[test]
fn test_standard_timer_function_block_ltime_overload() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR timer : TON; elapsed : LTIME; q : BOOL; END_VAR
    timer(IN := TRUE, PT := LTIME#1s, Q => q, ET => elapsed);
END_PROGRAM
"#,
    );
}

#[test]
fn test_standard_timer_function_block_ltime_variant() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR timer : TON_LTIME; elapsed : LTIME; q : BOOL; END_VAR
    timer(IN := TRUE, PT := LTIME#1s, Q => q, ET => elapsed);
END_PROGRAM
"#,
    );
}

#[test]
fn test_standard_timer_function_block_ltime_type_error() {
    check_has_error(
        r#"
PROGRAM Test
    VAR timer : TON_LTIME; elapsed : LTIME; q : BOOL; END_VAR
    timer(IN := TRUE, PT := T#1s, Q => q, ET => elapsed);
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 43 (bistable function blocks)
fn test_standard_bistable_function_blocks() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR rs : RS; sr : SR; q1 : BOOL; q2 : BOOL; END_VAR
    rs(S := TRUE, R1 := FALSE, Q1 => q1);
    rs(S := TRUE, RESET1 := FALSE, Q1 => q1);
    sr(S1 := TRUE, R := FALSE, Q1 => q2);
    sr(SET1 := TRUE, RESET := FALSE, Q1 => q2);
END_PROGRAM
"#,
    );
}

#[test]
fn test_standard_bistable_function_block_type_error() {
    check_has_error(
        r#"
PROGRAM Test
    VAR rs : RS; q1 : BOOL; END_VAR
    rs(S := 1, R1 := FALSE, Q1 => q1);
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 44 (edge detection function blocks)
fn test_standard_edge_function_blocks() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR rtrig : R_TRIG; ftrig : F_TRIG; q1 : BOOL; q2 : BOOL; END_VAR
    rtrig(CLK := TRUE, Q => q1);
    ftrig(CLK := FALSE, Q => q2);
END_PROGRAM
"#,
    );
}

#[test]
// IEC 61131-3 Ed.3 Table 45 (counter function blocks)
fn test_standard_counter_function_blocks() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR
        ctu : CTU;
        ctd : CTD;
        ctud : CTUD;
        ctu_int : CTU_INT;
        pv_dint : DINT;
        cv_dint : DINT;
        pv_int : INT;
        cv_int : INT;
        qu : BOOL;
        qd : BOOL;
        q : BOOL;
    END_VAR
    ctu(CU := TRUE, R := FALSE, PV := pv_dint, Q => q, CV => cv_dint);
    ctd(CD := TRUE, LD := FALSE, PV := pv_dint, Q => q, CV => cv_dint);
    ctud(CU := TRUE, CD := FALSE, R := FALSE, LD := FALSE, PV := pv_dint, QU => qu, QD => qd, CV => cv_dint);
    ctu_int(CU := TRUE, R := FALSE, PV := pv_int, Q => q, CV => cv_int);
END_PROGRAM
"#,
    );
}

#[test]
fn test_standard_counter_function_block_type_error() {
    check_has_error(
        r#"
PROGRAM Test
    VAR ctu : CTU; q : BOOL; cv : INT; END_VAR
    ctu(CU := TRUE, R := FALSE, PV := 1.0, Q => q, CV => cv);
END_PROGRAM
"#,
        DiagnosticCode::InvalidArgumentType,
    );
}

#[test]
fn test_typed_literal_prefix() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : INT; END_VAR
    x := INT#1;
END_PROGRAM
"#,
    );
}

#[test]
fn test_binary_operator_precedence() {
    check_no_errors(
        r#"
PROGRAM Test
    VAR x : BOOL; END_VAR
    IF 1 * 2 < 3 THEN
        x := TRUE;
    END_IF;
END_PROGRAM
"#,
    );
}
