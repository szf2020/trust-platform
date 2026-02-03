use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use trust_runtime::debug::{DebugBreakpoint, DebugStopReason, SourceLocation};
use trust_runtime::harness::{CompileSession, SourceFile};

fn line_index(source: &str, needle: &str) -> u32 {
    source
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing line for {needle}")) as u32
}

fn resolve_location(
    runtime: &trust_runtime::Runtime,
    source: &str,
    file_id: u32,
    needle: &str,
) -> SourceLocation {
    let line = line_index(source, needle);
    runtime
        .resolve_breakpoint_location(source, file_id, line, 0)
        .unwrap_or_else(|| panic!("failed to resolve breakpoint for {needle}"))
}

#[test]
fn step_in_enters_callee_on_first_statement() {
    let main = r#"PROGRAM Main
VAR
    Count : INT := 0;
END_VAR
    Count := AddTwo(Count);
    Count := Count + 1;
END_PROGRAM
"#;

    let lib = r#"FUNCTION AddTwo : INT
VAR_INPUT
    Value : INT;
END_VAR
    AddTwo := Value + 2;
END_FUNCTION
"#;

    let session = CompileSession::from_sources(vec![
        SourceFile::with_path("main.st", main),
        SourceFile::with_path("lib.st", lib),
    ]);
    let mut runtime = session.build_runtime().unwrap();
    let call_location = resolve_location(&runtime, main, 0, "Count := AddTwo");
    let expected_callee = resolve_location(&runtime, lib, 1, "AddTwo := Value + 2");

    let control = runtime.enable_debug();
    let (stop_tx, stop_rx) = channel();
    control.set_stop_sender(stop_tx);
    control.set_breakpoints_for_file(0, vec![DebugBreakpoint::new(call_location)]);

    let runtime = Arc::new(Mutex::new(runtime));
    let runtime_thread = runtime.clone();
    let handle = thread::spawn(move || {
        let mut runtime = runtime_thread.lock().expect("runtime lock poisoned");
        runtime.execute_cycle().unwrap();
    });

    let stop = stop_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    assert_eq!(stop.reason, DebugStopReason::Breakpoint);
    let thread_id = stop.thread_id.unwrap_or(1);

    control.step_thread(thread_id);
    let step_stop = stop_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    assert_eq!(step_stop.reason, DebugStopReason::Step);
    let location = step_stop.location.expect("step location");
    assert_eq!(location.file_id, 1);
    assert_eq!(location.start, expected_callee.start);

    control.continue_run();
    handle.join().unwrap();
}

#[test]
fn step_over_stops_in_caller_after_call() {
    let main = r#"PROGRAM Main
VAR
    Count : INT := 0;
END_VAR
    Count := AddTwo(Count);
    Count := Count + 1;
END_PROGRAM
"#;

    let lib = r#"FUNCTION AddTwo : INT
VAR_INPUT
    Value : INT;
END_VAR
    AddTwo := Value + 2;
END_FUNCTION
"#;

    let session = CompileSession::from_sources(vec![
        SourceFile::with_path("main.st", main),
        SourceFile::with_path("lib.st", lib),
    ]);
    let mut runtime = session.build_runtime().unwrap();
    let call_location = resolve_location(&runtime, main, 0, "Count := AddTwo");
    let expected_next = resolve_location(&runtime, main, 0, "Count := Count + 1");

    let control = runtime.enable_debug();
    let (stop_tx, stop_rx) = channel();
    control.set_stop_sender(stop_tx);
    control.set_breakpoints_for_file(0, vec![DebugBreakpoint::new(call_location)]);

    let runtime = Arc::new(Mutex::new(runtime));
    let runtime_thread = runtime.clone();
    let handle = thread::spawn(move || {
        let mut runtime = runtime_thread.lock().expect("runtime lock poisoned");
        runtime.execute_cycle().unwrap();
    });

    let stop = stop_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    assert_eq!(stop.reason, DebugStopReason::Breakpoint);
    let thread_id = stop.thread_id.unwrap_or(1);

    control.step_over_thread(thread_id);
    let step_stop = stop_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    assert_eq!(step_stop.reason, DebugStopReason::Step);
    let location = step_stop.location.expect("step location");
    assert_eq!(location.file_id, 0);
    assert_eq!(location.start, expected_next.start);

    control.continue_run();
    handle.join().unwrap();
}

#[test]
fn step_out_returns_to_caller_after_function_body() {
    let main = r#"PROGRAM Main
VAR
    Count : INT := 0;
END_VAR
    Count := AddTwo(Count);
    Count := Count + 1;
END_PROGRAM
"#;

    let lib = r#"FUNCTION AddTwo : INT
VAR_INPUT
    Value : INT;
END_VAR
VAR
    Temp : INT;
END_VAR
    Temp := Value + 1;
    AddTwo := Temp + 1;
END_FUNCTION
"#;

    let session = CompileSession::from_sources(vec![
        SourceFile::with_path("main.st", main),
        SourceFile::with_path("lib.st", lib),
    ]);
    let mut runtime = session.build_runtime().unwrap();
    let breakpoint_location = resolve_location(&runtime, lib, 1, "Temp := Value + 1");
    let expected_next = resolve_location(&runtime, main, 0, "Count := Count + 1");

    let control = runtime.enable_debug();
    let (stop_tx, stop_rx) = channel();
    control.set_stop_sender(stop_tx);
    control.set_breakpoints_for_file(1, vec![DebugBreakpoint::new(breakpoint_location)]);

    let runtime = Arc::new(Mutex::new(runtime));
    let runtime_thread = runtime.clone();
    let handle = thread::spawn(move || {
        let mut runtime = runtime_thread.lock().expect("runtime lock poisoned");
        runtime.execute_cycle().unwrap();
    });

    let stop = stop_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    assert_eq!(stop.reason, DebugStopReason::Breakpoint);
    let thread_id = stop.thread_id.unwrap_or(1);

    control.step_out_thread(thread_id);
    let step_stop = stop_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    assert_eq!(step_stop.reason, DebugStopReason::Step);
    let location = step_stop.location.expect("step location");
    assert_eq!(location.file_id, 0);
    assert_eq!(location.start, expected_next.start);

    control.continue_run();
    handle.join().unwrap();
}

#[test]
fn breakpoint_only_triggers_for_taken_branch() {
    let source = r#"PROGRAM Main
VAR
    Flag : BOOL := FALSE;
END_VAR
    IF Flag THEN
        Flag := TRUE;
    ELSE
        Flag := FALSE;
    END_IF;
END_PROGRAM
"#;

    let session = CompileSession::from_sources(vec![SourceFile::new(source)]);
    let mut runtime = session.build_runtime().unwrap();
    let then_location = resolve_location(&runtime, source, 0, "Flag := TRUE");

    let control = runtime.enable_debug();
    let (stop_tx, stop_rx) = channel();
    control.set_stop_sender(stop_tx);
    control.set_breakpoints_for_file(0, vec![DebugBreakpoint::new(then_location)]);

    let runtime = Arc::new(Mutex::new(runtime));
    let runtime_thread = runtime.clone();
    let handle = thread::spawn(move || {
        let mut runtime = runtime_thread.lock().expect("runtime lock poisoned");
        runtime.execute_cycle().unwrap();
    });

    assert!(stop_rx.recv_timeout(Duration::from_millis(200)).is_err());
    handle.join().unwrap();
}

#[test]
fn breakpoint_triggers_for_executed_branch() {
    let source = r#"PROGRAM Main
VAR
    Flag : BOOL := TRUE;
END_VAR
    IF Flag THEN
        Flag := FALSE;
    ELSE
        Flag := TRUE;
    END_IF;
END_PROGRAM
"#;

    let session = CompileSession::from_sources(vec![SourceFile::new(source)]);
    let mut runtime = session.build_runtime().unwrap();
    let then_location = resolve_location(&runtime, source, 0, "Flag := FALSE");

    let control = runtime.enable_debug();
    let (stop_tx, stop_rx) = channel();
    control.set_stop_sender(stop_tx);
    control.set_breakpoints_for_file(0, vec![DebugBreakpoint::new(then_location)]);

    let runtime = Arc::new(Mutex::new(runtime));
    let runtime_thread = runtime.clone();
    let handle = thread::spawn(move || {
        let mut runtime = runtime_thread.lock().expect("runtime lock poisoned");
        runtime.execute_cycle().unwrap();
    });

    let stop = stop_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    assert_eq!(stop.reason, DebugStopReason::Breakpoint);
    control.continue_run();
    handle.join().unwrap();
}
