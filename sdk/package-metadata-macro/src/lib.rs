//! Macro to access data from the `package.metadata` section of Cargo.toml

extern crate proc_macro;

use {
    proc_macro::TokenStream,
    quote::quote,
    std::{env, fs},
    syn::parse_macro_input,
    toml::value::{Array, Value},
};

/// Macro for accessing data from the `package.metadata` section of the Cargo manifest
///
/// # Arguments
/// * `key` - A string slice of a dot-separated path to the TOML key of interest
///
/// # Example
/// Given the following `Cargo.toml`:
/// ```ignore
/// [package]
/// name = "MyApp"
/// version = "0.1.0"
///
/// [package.metadata]
/// copyright = "Copyright (c) 2024 ACME Inc."
/// ```
///
/// You can fetch the copyright with the following:
/// ```ignore
/// use solana_sdk_macro::package_metadata;
///
/// pub fn main() {
///     let copyright = package_metadata!("copyright");
///     assert_eq!(copyright, "Copyright (c) 2024 ACME Inc.");
/// }
/// ```
///
/// ## TOML Support
/// This macro only supports static data:
/// * Strings
/// * Integers
/// * Floating-point numbers
/// * Booleans
/// * Datetimes
/// * Arrays
///
/// ## Array Example
/// Given the following Cargo manifest:
/// ```ignore
/// [package.metadata.arrays]
/// some_array = [ 1, 2, 3 ]
/// ```
///
/// This is legal:
/// ```ignore
/// static ARR: [i64; 3] = package_metadata!("arrays.some_array");
/// ```
///
/// It does *not* currently support accessing TOML array elements directly.
/// TOML tables are not supported.
#[proc_macro]
pub fn package_metadata(input: TokenStream) -> TokenStream {
    let key = parse_macro_input!(input as syn::LitStr);
    let full_key = &key.value();
    let path = format!("{}/Cargo.toml", env::var("CARGO_MANIFEST_DIR").unwrap());
    let manifest = load_manifest(&path);
    let value = package_metadata_value(&manifest, full_key);
    toml_value_codegen(value).into()
}

fn package_metadata_value<'a>(manifest: &'a Value, full_key: &str) -> &'a Value {
    let error_message =
        format!("Key `package.metadata.{full_key}` must be present in the Cargo manifest");
    manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| {
            let mut table = metadata
                .as_table()
                .expect("TOML property `package.metadata` must be a table");
            let mut value = None;
            for key in full_key.split('.') {
                match table.get(key).expect(&error_message) {
                    Value::Table(t) => {
                        table = t;
                    }
                    v => {
                        value = Some(v);
                    }
                }
            }
            value
        })
        .expect(&error_message)
}

fn toml_value_codegen(value: &Value) -> proc_macro2::TokenStream {
    match value {
        Value::String(s) => quote! {{ #s }},
        Value::Integer(i) => quote! {{ #i }},
        Value::Float(f) => quote! {{ #f }},
        Value::Boolean(b) => quote! {{ #b }},
        Value::Array(a) => toml_array_codegen(a),
        Value::Datetime(d) => {
            let date_str = toml::ser::to_string(d).unwrap();
            quote! {{
                #date_str
            }}
        }
        Value::Table(_) => {
            panic!("Tables are not supported");
        }
    }
}

fn toml_array_codegen(array: &Array) -> proc_macro2::TokenStream {
    let statements = array
        .iter()
        .flat_map(|val| {
            let val = toml_value_codegen(val);
            quote! {
                #val,
            }
        })
        .collect::<proc_macro2::TokenStream>();
    quote! {{
        [
            #statements
        ]
    }}
}

fn load_manifest(path: &str) -> Value {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("error occurred reading Cargo manifest {path}: {err}"));
    toml::from_str(&contents)
        .unwrap_or_else(|err| panic!("error occurred parsing Cargo manifest {path}: {err}"))
}

#[cfg(test)]
mod tests {
    use {super::*, std::str::FromStr};

    #[test]
    fn package_metadata_string() {
        let copyright = "Copyright (c) 2024 ACME Inc.";
        let manifest = toml::from_str(&format!(
            r#"
            [package.metadata]
            copyright = "{copyright}"
        "#
        ))
        .unwrap();
        assert_eq!(
            package_metadata_value(&manifest, "copyright")
                .as_str()
                .unwrap(),
            copyright
        );
    }

    #[test]
    fn package_metadata_nested() {
        let program_id = "11111111111111111111111111111111";
        let manifest = toml::from_str(&format!(
            r#"
            [package.metadata.solana]
            program-id = "{program_id}"
        "#
        ))
        .unwrap();
        assert_eq!(
            package_metadata_value(&manifest, "solana.program-id")
                .as_str()
                .unwrap(),
            program_id
        );
    }

    #[test]
    fn package_metadata_bool() {
        let manifest = toml::from_str(
            r#"
            [package.metadata]
            is-ok = true
        "#,
        )
        .unwrap();
        assert!(package_metadata_value(&manifest, "is-ok")
            .as_bool()
            .unwrap());
    }

    #[test]
    fn package_metadata_int() {
        let number = 123;
        let manifest = toml::from_str(&format!(
            r#"
            [package.metadata]
            number = {number}
        "#
        ))
        .unwrap();
        assert_eq!(
            package_metadata_value(&manifest, "number")
                .as_integer()
                .unwrap(),
            number
        );
    }

    #[test]
    fn package_metadata_float() {
        let float = 123.456;
        let manifest = toml::from_str(&format!(
            r#"
            [package.metadata]
            float = {float}
        "#
        ))
        .unwrap();
        assert_eq!(
            package_metadata_value(&manifest, "float")
                .as_float()
                .unwrap(),
            float
        );
    }

    #[test]
    fn package_metadata_array() {
        let array = ["1", "2", "3"];
        let manifest = toml::from_str(&format!(
            r#"
            [package.metadata]
            array = {array:?}
        "#
        ))
        .unwrap();
        assert_eq!(
            package_metadata_value(&manifest, "array")
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap())
                .collect::<Vec<_>>(),
            array
        );
    }

    #[test]
    fn package_metadata_datetime() {
        let datetime = "1979-05-27T07:32:00Z";
        let manifest = toml::from_str(&format!(
            r#"
            [package.metadata]
            datetime = {datetime}
        "#
        ))
        .unwrap();
        let toml_datetime = toml::value::Datetime::from_str(datetime).unwrap();
        assert_eq!(
            package_metadata_value(&manifest, "datetime")
                .as_datetime()
                .unwrap(),
            &toml_datetime
        );
    }
}
