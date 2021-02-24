//! Borsh utils
use borsh::schema::{BorshSchema, Declaration, Definition, Fields};
use std::collections::HashMap;

/// Get packed length for the given BorchSchema Declaration
fn get_declaration_packed_len(
    declaration: &str,
    definitions: &HashMap<Declaration, Definition>,
) -> usize {
    match definitions.get(declaration) {
        Some(Definition::Array { length, elements }) => {
            *length as usize * get_declaration_packed_len(elements, definitions)
        }
        Some(Definition::Enum { variants }) => {
            1 + variants
                .iter()
                .map(|(_, declaration)| get_declaration_packed_len(declaration, definitions))
                .max()
                .unwrap_or(0)
        }
        Some(Definition::Struct { fields }) => match fields {
            Fields::NamedFields(named_fields) => named_fields
                .iter()
                .map(|(_, declaration)| get_declaration_packed_len(declaration, definitions))
                .sum(),
            Fields::UnnamedFields(declarations) => declarations
                .iter()
                .map(|declaration| get_declaration_packed_len(declaration, definitions))
                .sum(),
            Fields::Empty => 0,
        },
        Some(Definition::Sequence {
            elements: _elements,
        }) => panic!("Missing support for Definition::Sequence"),
        Some(Definition::Tuple { elements }) => elements
            .iter()
            .map(|element| get_declaration_packed_len(element, definitions))
            .sum(),
        None => match declaration {
            "u8" | "i8" => 1,
            "u16" | "i16" => 2,
            "u32" | "i32" => 2,
            "u64" | "i64" => 8,
            "u128" | "i128" => 16,
            "nil" => 0,
            _ => panic!("Missing primitive type: {}", declaration),
        },
    }
}

/// Get the worst-case packed length for the given BorshSchema
pub fn get_packed_len<S: BorshSchema>() -> usize {
    let schema_container = S::schema_container();
    get_declaration_packed_len(&schema_container.declaration, &schema_container.definitions)
}
