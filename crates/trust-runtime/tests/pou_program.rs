use trust_runtime::eval::expr::{Expr, LValue};
use trust_runtime::eval::ops::BinaryOp;
use trust_runtime::eval::stmt::Stmt;
use trust_runtime::task::ProgramDef;
use trust_runtime::value::Value;
use trust_runtime::Runtime;

#[test]
fn program_cycle() {
    let mut runtime = Runtime::new();
    runtime.storage_mut().set_global("count", Value::Int(0));

    let program = ProgramDef {
        name: "TestProg".into(),
        vars: Vec::new(),
        temps: Vec::new(),
        using: Vec::new(),
        body: vec![Stmt::Assign {
            target: LValue::Name("count".into()),
            value: Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Name("count".into())),
                right: Box::new(Expr::Literal(Value::Int(1))),
            },
            location: None,
        }],
    };

    runtime.execute_program(&program).unwrap();
    assert_eq!(
        runtime.storage_mut().get_global("count"),
        Some(&Value::Int(1))
    );
}
