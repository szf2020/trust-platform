//! Small bytecode helpers.

#![allow(missing_docs)]

pub(crate) fn align4(value: usize) -> usize {
    (value + 3) & !3
}

pub(crate) fn pad_to(bytes: &mut Vec<u8>, target: usize) {
    if bytes.len() < target {
        bytes.resize(target, 0);
    }
}
