//! Convenience macro to declare a static public key and functions to interact with it
//!
//! Input: a single literal base58 string representation of a program's id

extern crate proc_macro;

use {
    proc_macro::TokenStream,
    proc_macro2::Span,
    quote::{quote, ToTokens},
    syn::{
        bracketed,
        parse::{Parse, ParseStream, Result},
        parse_macro_input,
        punctuated::Punctuated,
        token::Bracket,
        Expr, Ident, LitByte, LitStr, Token,
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

#[deprecated(since = "2.1.0", note = "Use `solana_pubkey::pubkey` instead")]
#[proc_macro]
pub fn pubkey(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as SdkPubkey);
    TokenStream::from(quote! {#id})
}

#[deprecated(since = "2.1.0", note = "Use `solana_pubkey::pubkey!` instead")]
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

#[deprecated(since = "2.1.0", note = "Use `solana_pubkey::declare_id` instead")]
#[proc_macro]
pub fn program_declare_id(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as ProgramSdkId);
    TokenStream::from(quote! {#id})
}

#[deprecated(
    since = "2.1.0",
    note = "Use `solana_pubkey::declare_deprecated_id` instead"
)]
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
                        core::ptr::addr_of_mut!((*ptr).#name).write(self.#name);
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
                        let mut value = core::mem::MaybeUninit::<Self>::uninit();
                        unsafe {
                            core::ptr::write_bytes(&mut value, 0, 1);
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
