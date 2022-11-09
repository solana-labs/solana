//! Convenience macro to declare a static public key and functions to interact with it
//!
//! Input: a single literal base58 string representation of a program's id

extern crate proc_macro;

use {
    proc_macro::TokenStream,
    proc_macro2::{Delimiter, Span, TokenTree},
    quote::{quote, ToTokens},
    std::convert::TryFrom,
    syn::{
        bracketed,
        parse::{Parse, ParseStream, Result},
        parse_macro_input,
        punctuated::Punctuated,
        token::Bracket,
        Expr, Ident, LitByte, LitStr, Path, Token,
    },
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
        /// The static program ID.
        pub static ID: #pubkey_type = #id;

        /// Returns `true` if given pubkey is the program ID.
        pub fn check_id(id: &#pubkey_type) -> bool {
            id == &ID
        }

        /// Returns the program ID.
        pub fn id() -> #pubkey_type {
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
            fn test_id() {
            #[allow(deprecated)]
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
                            .path
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
