//! Convenience macro to declare a static public key and functions to interact with it
//!
//! Input: a single literal base58 string representation of a program's id

extern crate proc_macro;

use {
    proc_macro::TokenStream,
    proc_macro2::{Delimiter, Span, TokenTree},
    quote::{quote, ToTokens},
    std::{env, fs},
    syn::{
        bracketed,
        parse::{Parse, ParseStream, Result},
        parse_macro_input,
        punctuated::Punctuated,
        token::Bracket,
        Expr, Ident, LitByte, LitStr, Path, Token,
    },
    toml::value::{Array, Value},
};

fn parse_id(
    input: ParseStream,
    pubkey_type: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let id = if input.peek(syn::LitStr) {
        let id_literal: LitStr = input.parse()?;
        parse_pubkey(&id_literal, &pubkey_type)?
    } else {
        let expr: Expr = input.parse()?;
        quote! { #expr }
    };

    if !input.is_empty() {
        let stream: proc_macro2::TokenStream = input.parse()?;
        return Err(syn::Error::new_spanned(stream, "unexpected token"));
    }
    Ok(id)
}

fn id_to_tokens(
    id: &proc_macro2::TokenStream,
    pubkey_type: proc_macro2::TokenStream,
    tokens: &mut proc_macro2::TokenStream,
) {
    tokens.extend(quote! {
        /// The const program ID.
        pub const ID: #pubkey_type = #id;

        /// Returns `true` if given pubkey is the program ID.
        // TODO make this const once `derive_const` makes it out of nightly
        // and we can `derive_const(PartialEq)` on `Pubkey`.
        pub fn check_id(id: &#pubkey_type) -> bool {
            id == &ID
        }

        /// Returns the program ID.
        pub const fn id() -> #pubkey_type {
            ID
        }

        #[cfg(test)]
        #[test]
        fn test_id() {
            assert!(check_id(&id()));
        }
    });
}

fn deprecated_id_to_tokens(
    id: &proc_macro2::TokenStream,
    pubkey_type: proc_macro2::TokenStream,
    tokens: &mut proc_macro2::TokenStream,
) {
    tokens.extend(quote! {
        /// The static program ID.
        pub static ID: #pubkey_type = #id;

        /// Returns `true` if given pubkey is the program ID.
        #[deprecated()]
        pub fn check_id(id: &#pubkey_type) -> bool {
            id == &ID
        }

        /// Returns the program ID.
        #[deprecated()]
        pub fn id() -> #pubkey_type {
            ID
        }

        #[cfg(test)]
        #[test]
        #[allow(deprecated)]
        fn test_id() {
            assert!(check_id(&id()));
        }
    });
}

struct SdkPubkey(proc_macro2::TokenStream);

impl Parse for SdkPubkey {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_id(input, quote! { ::solana_sdk::pubkey::Pubkey }).map(Self)
    }
}

impl ToTokens for SdkPubkey {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let id = &self.0;
        tokens.extend(quote! {#id})
    }
}

struct ProgramSdkPubkey(proc_macro2::TokenStream);

impl Parse for ProgramSdkPubkey {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_id(input, quote! { ::solana_program::pubkey::Pubkey }).map(Self)
    }
}

impl ToTokens for ProgramSdkPubkey {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let id = &self.0;
        tokens.extend(quote! {#id})
    }
}

struct Id(proc_macro2::TokenStream);

impl Parse for Id {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_id(input, quote! { ::solana_sdk::pubkey::Pubkey }).map(Self)
    }
}

impl ToTokens for Id {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        id_to_tokens(&self.0, quote! { ::solana_sdk::pubkey::Pubkey }, tokens)
    }
}

struct IdDeprecated(proc_macro2::TokenStream);

impl Parse for IdDeprecated {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_id(input, quote! { ::solana_sdk::pubkey::Pubkey }).map(Self)
    }
}

impl ToTokens for IdDeprecated {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        deprecated_id_to_tokens(&self.0, quote! { ::solana_sdk::pubkey::Pubkey }, tokens)
    }
}

struct ProgramSdkId(proc_macro2::TokenStream);
impl Parse for ProgramSdkId {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_id(input, quote! { ::solana_program::pubkey::Pubkey }).map(Self)
    }
}

impl ToTokens for ProgramSdkId {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        id_to_tokens(&self.0, quote! { ::solana_program::pubkey::Pubkey }, tokens)
    }
}

struct ProgramSdkIdDeprecated(proc_macro2::TokenStream);
impl Parse for ProgramSdkIdDeprecated {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_id(input, quote! { ::solana_program::pubkey::Pubkey }).map(Self)
    }
}

impl ToTokens for ProgramSdkIdDeprecated {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        deprecated_id_to_tokens(&self.0, quote! { ::solana_program::pubkey::Pubkey }, tokens)
    }
}

#[allow(dead_code)] // `respan` may be compiled out
struct RespanInput {
    to_respan: Path,
    respan_using: Span,
}

impl Parse for RespanInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let to_respan: Path = input.parse()?;
        let _comma: Token![,] = input.parse()?;
        let respan_tree: TokenTree = input.parse()?;
        match respan_tree {
            TokenTree::Group(g) if g.delimiter() == Delimiter::None => {
                let ident: Ident = syn::parse2(g.stream())?;
                Ok(RespanInput {
                    to_respan,
                    respan_using: ident.span(),
                })
            }
            TokenTree::Ident(i) => Ok(RespanInput {
                to_respan,
                respan_using: i.span(),
            }),
            val => Err(syn::Error::new_spanned(
                val,
                "expected None-delimited group",
            )),
        }
    }
}

/// A proc-macro which respans the tokens in its first argument (a `Path`)
/// to be resolved at the tokens of its second argument.
/// For internal use only.
///
/// There must be exactly one comma in the input,
/// which is used to separate the two arguments.
/// The second argument should be exactly one token.
///
/// For example, `respan!($crate::foo, with_span)`
/// produces the tokens `$crate::foo`, but resolved
/// at the span of `with_span`.
///
/// The input to this function should be very short -
/// its only purpose is to override the span of a token
/// sequence containing `$crate`. For all other purposes,
/// a more general proc-macro should be used.
#[rustversion::since(1.46.0)] // `Span::resolved_at` is stable in 1.46.0 and above
#[proc_macro]
pub fn respan(input: TokenStream) -> TokenStream {
    // Obtain the `Path` we are going to respan, and the ident
    // whose span we will be using.
    let RespanInput {
        to_respan,
        respan_using,
    } = parse_macro_input!(input as RespanInput);
    // Respan all of the tokens in the `Path`
    let to_respan: proc_macro2::TokenStream = to_respan
        .into_token_stream()
        .into_iter()
        .map(|mut t| {
            // Combine the location of the token with the resolution behavior of `respan_using`
            let new_span: Span = t.span().resolved_at(respan_using);
            t.set_span(new_span);
            t
        })
        .collect();
    TokenStream::from(to_respan)
}

#[proc_macro]
pub fn pubkey(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as SdkPubkey);
    TokenStream::from(quote! {#id})
}

#[proc_macro]
pub fn program_pubkey(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as ProgramSdkPubkey);
    TokenStream::from(quote! {#id})
}

#[proc_macro]
pub fn declare_id(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as Id);
    TokenStream::from(quote! {#id})
}

#[proc_macro]
pub fn declare_deprecated_id(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as IdDeprecated);
    TokenStream::from(quote! {#id})
}

#[proc_macro]
pub fn program_declare_id(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as ProgramSdkId);
    TokenStream::from(quote! {#id})
}

#[proc_macro]
pub fn program_declare_deprecated_id(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as ProgramSdkIdDeprecated);
    TokenStream::from(quote! {#id})
}

fn parse_pubkey(
    id_literal: &LitStr,
    pubkey_type: &proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let id_vec = bs58::decode(id_literal.value())
        .into_vec()
        .map_err(|_| syn::Error::new_spanned(id_literal, "failed to decode base58 string"))?;
    let id_array = <[u8; 32]>::try_from(<&[u8]>::clone(&&id_vec[..])).map_err(|_| {
        syn::Error::new_spanned(
            id_literal,
            format!("pubkey array is not 32 bytes long: len={}", id_vec.len()),
        )
    })?;
    let bytes = id_array.iter().map(|b| LitByte::new(*b, Span::call_site()));
    Ok(quote! {
        #pubkey_type::new_from_array(
            [#(#bytes,)*]
        )
    })
}

struct Pubkeys {
    method: Ident,
    num: usize,
    pubkeys: proc_macro2::TokenStream,
}
impl Parse for Pubkeys {
    fn parse(input: ParseStream) -> Result<Self> {
        let pubkey_type = quote! {
            ::solana_sdk::pubkey::Pubkey
        };

        let method = input.parse()?;
        let _comma: Token![,] = input.parse()?;
        let (num, pubkeys) = if input.peek(syn::LitStr) {
            let id_literal: LitStr = input.parse()?;
            (1, parse_pubkey(&id_literal, &pubkey_type)?)
        } else if input.peek(Bracket) {
            let pubkey_strings;
            bracketed!(pubkey_strings in input);
            let punctuated: Punctuated<LitStr, Token![,]> =
                Punctuated::parse_terminated(&pubkey_strings)?;
            let mut pubkeys: Punctuated<proc_macro2::TokenStream, Token![,]> = Punctuated::new();
            for string in punctuated.iter() {
                pubkeys.push(parse_pubkey(string, &pubkey_type)?);
            }
            (pubkeys.len(), quote! {#pubkeys})
        } else {
            let stream: proc_macro2::TokenStream = input.parse()?;
            return Err(syn::Error::new_spanned(stream, "unexpected token"));
        };

        Ok(Pubkeys {
            method,
            num,
            pubkeys,
        })
    }
}

impl ToTokens for Pubkeys {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let Pubkeys {
            method,
            num,
            pubkeys,
        } = self;

        let pubkey_type = quote! {
            ::solana_sdk::pubkey::Pubkey
        };
        if *num == 1 {
            tokens.extend(quote! {
                pub fn #method() -> #pubkey_type {
                    #pubkeys
                }
            });
        } else {
            tokens.extend(quote! {
                pub fn #method() -> ::std::vec::Vec<#pubkey_type> {
                    vec![#pubkeys]
                }
            });
        }
    }
}

#[proc_macro]
pub fn pubkeys(input: TokenStream) -> TokenStream {
    let pubkeys = parse_macro_input!(input as Pubkeys);
    TokenStream::from(quote! {#pubkeys})
}

// The normal `wasm_bindgen` macro generates a .bss section which causes the resulting
// SBF program to fail to load, so for now this stub should be used when building for SBF
#[proc_macro_attribute]
pub fn wasm_bindgen_stub(_attr: TokenStream, item: TokenStream) -> TokenStream {
    match parse_macro_input!(item as syn::Item) {
        syn::Item::Struct(mut item_struct) => {
            if let syn::Fields::Named(fields) = &mut item_struct.fields {
                // Strip out any `#[wasm_bindgen]` added to struct fields. This is custom
                // syntax supplied by the normal `wasm_bindgen` macro.
                for field in fields.named.iter_mut() {
                    field.attrs.retain(|attr| {
                        !attr
                            .path()
                            .segments
                            .iter()
                            .any(|segment| segment.ident == "wasm_bindgen")
                    });
                }
            }
            quote! { #item_struct }
        }
        item => {
            quote!(#item)
        }
    }
    .into()
}

// Sets padding in structures to zero explicitly.
// Otherwise padding could be inconsistent across the network and lead to divergence / consensus failures.
#[proc_macro_derive(CloneZeroed)]
pub fn derive_clone_zeroed(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    match parse_macro_input!(input as syn::Item) {
        syn::Item::Struct(item_struct) => {
            let clone_statements = match item_struct.fields {
                syn::Fields::Named(ref fields) => fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        std::ptr::addr_of_mut!((*ptr).#name).write(self.#name);
                    }
                }),
                _ => unimplemented!(),
            };
            let name = &item_struct.ident;
            quote! {
                impl Clone for #name {
                    // Clippy lint `incorrect_clone_impl_on_copy_type` requires that clone
                    // implementations on `Copy` types are simply wrappers of `Copy`.
                    // This is not the case here, and intentionally so because we want to
                    // guarantee zeroed padding.
                    fn clone(&self) -> Self {
                        let mut value = std::mem::MaybeUninit::<Self>::uninit();
                        unsafe {
                            std::ptr::write_bytes(&mut value, 0, 1);
                            let ptr = value.as_mut_ptr();
                            #(#clone_statements)*
                            value.assume_init()
                        }
                    }
                }
            }
        }
        _ => unimplemented!(),
    }
    .into()
}

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
