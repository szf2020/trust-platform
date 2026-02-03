mod bytecode_helpers;

use bytecode_helpers::{base_module, module_with_debug};
use trust_runtime::bytecode::{BytecodeError, BytecodeModule, SectionData, SectionId};

#[test]
fn opcode_validation() {
    let mut module = base_module();
    if let Some(SectionData::PouBodies(bodies)) = module.section_mut(SectionId::PouBodies) {
        *bodies = vec![0xFF];
    }
    let bytes = module.encode().expect("encode");
    let decoded = BytecodeModule::decode(&bytes).expect("decode");
    let err = decoded.validate().unwrap_err();
    assert!(matches!(err, BytecodeError::InvalidOpcode(0xFF)));
}

#[test]
fn opcode_validation_extended() {
    let mut module = base_module();
    if let Some(SectionData::PouBodies(bodies)) = module.section_mut(SectionId::PouBodies) {
        *bodies = vec![0x14, 0x15, 0x16, 0x02, 0x4C, 0x4D, 0x4E];
    }
    if let Some(SectionData::PouIndex(index)) = module.section_mut(SectionId::PouIndex) {
        index.entries[0].code_length = 7;
    }
    let bytes = module.encode().expect("encode");
    let decoded = BytecodeModule::decode(&bytes).expect("decode");
    decoded.validate().expect("validate");
}

#[test]
fn jump_validation() {
    let mut module = base_module();
    if let Some(SectionData::PouBodies(bodies)) = module.section_mut(SectionId::PouBodies) {
        let mut code = vec![0x02];
        code.extend_from_slice(&100i32.to_le_bytes());
        *bodies = code;
    }
    if let Some(SectionData::PouIndex(index)) = module.section_mut(SectionId::PouIndex) {
        index.entries[0].code_length = 5;
    }
    let bytes = module.encode().expect("encode");
    let decoded = BytecodeModule::decode(&bytes).expect("decode");
    let err = decoded.validate().unwrap_err();
    assert!(matches!(err, BytecodeError::InvalidJumpTarget(_)));
}

#[test]
fn call_validation() {
    let mut module = base_module();
    if let Some(SectionData::PouBodies(bodies)) = module.section_mut(SectionId::PouBodies) {
        let mut code = vec![0x05];
        code.extend_from_slice(&99u32.to_le_bytes());
        *bodies = code;
    }
    if let Some(SectionData::PouIndex(index)) = module.section_mut(SectionId::PouIndex) {
        index.entries[0].code_length = 5;
    }
    let bytes = module.encode().expect("encode");
    let decoded = BytecodeModule::decode(&bytes).expect("decode");
    let err = decoded.validate().unwrap_err();
    assert!(matches!(err, BytecodeError::InvalidPouId(99)));
}

#[test]
fn debug_map_validation() {
    let mut module = module_with_debug();
    if let Some(SectionData::DebugMap(map)) = module.section_mut(SectionId::DebugMap) {
        map.entries[0].code_offset = 2;
    }
    let bytes = module.encode().expect("encode");
    let decoded = BytecodeModule::decode(&bytes).expect("decode");
    let err = decoded.validate().unwrap_err();
    assert!(matches!(err, BytecodeError::InvalidSection(_)));
}
