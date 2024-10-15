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
/// use solana_package_metadata::package_metadata;
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
pub use solana_package_metadata_macro::package_metadata;
/// Re-export solana_pubkey::declare_id for easy usage within the macro
pub use solana_pubkey::declare_id;

/// Convenience macro for declaring a program id from Cargo.toml package metadata.
///
/// # Arguments
/// * `key` - A string slice of a dot-separated path to the TOML key of interest
///
/// # Example
/// Given the following `Cargo.toml`:
/// ```ignore
/// [package]
/// name = "my-solana-program"
/// version = "0.1.0"
///
/// [package.metadata.solana]
/// program-id = "MyProgram1111111111111111111111111111111111"
/// ```
///
/// A program can use the program id declared in its `Cargo.toml` as the program
/// id in code:
///
/// ```ignore
/// declare_id_with_package_metadata!("solana.program-id");
/// ```
///
/// This program id behaves exactly as if the developer had written:
///
/// ```
/// solana_pubkey::declare_id!("MyProgram1111111111111111111111111111111111");
/// ```
///
/// Meaning that it's possible to refer to the program id using `crate::id()`,
/// without needing to specify the program id in multiple places.
#[macro_export]
macro_rules! declare_id_with_package_metadata {
    ($key:literal) => {
        $crate::declare_id!($crate::package_metadata!($key));
    };
}
