pub use super::core::{
    document_highlight, goto_declaration, goto_definition, goto_implementation,
    goto_type_definition, prepare_rename, references_with_progress, rename, selection_range,
};

#[cfg(test)]
pub use super::core::references;
#[cfg(test)]
pub(crate) use super::core::{references_with_ticket_for_tests, rename_with_ticket_for_tests};
