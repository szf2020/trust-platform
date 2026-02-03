use super::super::*;

#[test]
fn test_database_basic() {
    let mut db = Database::new();
    let file = FileId(0);

    db.set_source_text(file, "PROGRAM Test END_PROGRAM".to_string());

    let source = db.source_text(file);
    assert!(source.contains("PROGRAM"));
}

#[test]
fn test_expr_id_type_of() {
    let mut db = Database::new();
    let file = FileId(0);
    let source = "PROGRAM Test VAR x : DINT; END_VAR x := 1 + 2; END_PROGRAM";
    db.set_source_text(file, source.to_string());

    let plus_offset = source.find('+').unwrap() as u32;
    let expr_id = db.expr_id_at_offset(file, plus_offset).unwrap();
    let expr_type = db.type_of(file, expr_id);
    assert_eq!(expr_type, TypeId::SINT);
}

#[test]
fn test_expr_id_type_of_based_literal() {
    let mut db = Database::new();
    let file = FileId(0);
    let source = "PROGRAM Test VAR x : UINT; END_VAR x := 16#FF; END_PROGRAM";
    db.set_source_text(file, source.to_string());

    let hash_offset = source.find('#').unwrap() as u32;
    let expr_id = db.expr_id_at_offset(file, hash_offset).unwrap();
    let expr_type = db.type_of(file, expr_id);
    assert_eq!(expr_type, TypeId::USINT);
}

#[test]
fn test_type_of_cache_invalidates_on_change() {
    let mut db = Database::new();
    let file = FileId(0);
    let source = "PROGRAM Test VAR x : DINT; END_VAR x := 1 + 2; END_PROGRAM";
    db.set_source_text(file, source.to_string());

    let plus_offset = source.find('+').unwrap() as u32;
    let expr_id = db.expr_id_at_offset(file, plus_offset).unwrap();
    let expr_type = db.type_of(file, expr_id);
    assert_eq!(expr_type, TypeId::SINT);

    let updated = "PROGRAM Test VAR x : DINT; END_VAR x := REAL#1.0 + REAL#2.0; END_PROGRAM";
    db.set_source_text(file, updated.to_string());
    let plus_offset = updated.find('+').unwrap() as u32;
    let expr_id = db.expr_id_at_offset(file, plus_offset).unwrap();
    let expr_type = db.type_of(file, expr_id);
    assert_eq!(expr_type, TypeId::REAL);
}
