use trust_runtime::Runtime;

#[test]
fn loads_runtime() {
    let runtime = Runtime::new();
    let _profile = runtime.profile();
}
