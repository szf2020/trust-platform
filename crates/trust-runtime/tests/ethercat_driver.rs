use trust_runtime::io::{EthercatIoDriver, IoDriver, IoDriverHealth};

#[test]
fn ethercat_mock_profile_maps_ek1100_elx008_process_image() {
    let params: toml::Value = toml::from_str(
        r#"
adapter = "mock"
mock_inputs = ["01"]
[[modules]]
model = "EK1100"
slot = 0
[[modules]]
model = "EL1008"
slot = 1
channels = 8
[[modules]]
model = "EL2008"
slot = 2
channels = 8
"#,
    )
    .expect("parse params");
    let mut driver = EthercatIoDriver::from_params(&params).expect("driver");

    let mut inputs = [0u8; 1];
    driver.read_inputs(&mut inputs).expect("read inputs");
    assert_eq!(inputs, [0x01], "input image should map from mock frame");

    driver.write_outputs(&[0xAA]).expect("write outputs");
    assert!(
        matches!(driver.health(), IoDriverHealth::Ok),
        "healthy cycle should keep status ok"
    );
}

#[test]
fn ethercat_cycle_warn_threshold_reports_degraded_health() {
    let params: toml::Value = toml::from_str(
        r#"
adapter = "mock"
cycle_warn_ms = 1
mock_latency_ms = 5
[[modules]]
model = "EK1100"
slot = 0
[[modules]]
model = "EL1008"
slot = 1
[[modules]]
model = "EL2008"
slot = 2
"#,
    )
    .expect("parse params");
    let mut driver = EthercatIoDriver::from_params(&params).expect("driver");

    let mut inputs = [0u8; 1];
    driver.read_inputs(&mut inputs).expect("read inputs");

    assert!(
        matches!(driver.health(), IoDriverHealth::Degraded { .. }),
        "latency above cycle_warn_ms should surface degraded health"
    );
}

#[test]
fn ethercat_warn_policy_does_not_fail_runtime_cycle() {
    let params: toml::Value = toml::from_str(
        r#"
adapter = "mock"
on_error = "warn"
mock_fail_write = true
[[modules]]
model = "EK1100"
slot = 0
[[modules]]
model = "EL1008"
slot = 1
[[modules]]
model = "EL2008"
slot = 2
"#,
    )
    .expect("parse params");
    let mut driver = EthercatIoDriver::from_params(&params).expect("driver");

    driver
        .write_outputs(&[0x01])
        .expect("warn policy should not fail write cycle");
    assert!(
        matches!(driver.health(), IoDriverHealth::Degraded { .. }),
        "write failure under warn policy should degrade health"
    );
}
