//! Convenience macro to declare a static public key and functions to interact with it
//!
//! Input: a single literal base58 string representation of a program's id

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use std::convert::TryFrom;
use syn::{
    bracketed,
    parse::{Parse, ParseStream, Result},
    parse_macro_input,
    punctuated::Punctuated,
    token::Bracket,
    Expr, Ident, LitByte, LitStr, Token,
};

struct Id(proc_macro2::TokenStream);
impl Parse for Id {
    fn parse(input: ParseStream) -> Result<Self> {
        let token_stream = if input.peek(syn::LitStr) {
            let id_literal: LitStr = input.parse()?;
            parse_pubkey(&id_literal)?
        } else {
            let expr: Expr = input.parse()?;
            quote! { #expr }
        };

        if !input.is_empty() {
            let stream: proc_macro2::TokenStream = input.parse()?;
            return Err(syn::Error::new_spanned(stream, "unexpected token"));
        }

        Ok(Id(token_stream))
    }
}

impl ToTokens for Id {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let id = &self.0;
        tokens.extend(quote! {
            pub static ID: ::solana_sdk::pubkey::Pubkey = #id;

            pub fn check_id(id: &::solana_sdk::pubkey::Pubkey) -> bool {
                id == &ID
            }

            pub fn id() -> ::solana_sdk::pubkey::Pubkey {
                ID
            }

            #[cfg(test)]
            #[test]
            fn test_id() {
                assert!(check_id(&id()));
            }
        });
    }
}

#[proc_macro]
pub fn declare_id(input: TokenStream) -> TokenStream {
    let id = parse_macro_input!(input as Id);
    TokenStream::from(quote! {#id})
}

fn parse_pubkey(id_literal: &LitStr) -> Result<proc_macro2::TokenStream> {
    let id_vec = bs58::decode(id_literal.value())
        .into_vec()
        .map_err(|_| syn::Error::new_spanned(&id_literal, "failed to decode base58 string"))?;
    let id_array = <[u8; 32]>::try_from(<&[u8]>::clone(&&id_vec[..])).map_err(|_| {
        syn::Error::new_spanned(
            &id_literal,
            format!("pubkey array is not 32 bytes long: len={}", id_vec.len()),
        )
    })?;
    let bytes = id_array.iter().map(|b| LitByte::new(*b, Span::call_site()));
    Ok(quote! {
        ::solana_sdk::pubkey::Pubkey::new_from_array(
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
        let method = input.parse()?;
        let _comma: Token![,] = input.parse()?;
        let (num, pubkeys) = if input.peek(syn::LitStr) {
            let id_literal: LitStr = input.parse()?;
            (1, parse_pubkey(&id_literal)?)
        } else if input.peek(Bracket) {
            let pubkey_strings;
            bracketed!(pubkey_strings in input);
            let punctuated: Punctuated<LitStr, Token![,]> =
                Punctuated::parse_terminated(&pubkey_strings)?;
            let mut pubkeys: Punctuated<proc_macro2::TokenStream, Token![,]> = Punctuated::new();
            for string in punctuated.iter() {
                pubkeys.push(parse_pubkey(string)?);
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
        if *num == 1 {
            tokens.extend(quote! {
                pub fn #method() -> ::solana_sdk::pubkey::Pubkey {
                    #pubkeys
                }
            });
        } else {
            tokens.extend(quote! {
                pub fn #method() -> ::std::vec::Vec<::solana_sdk::pubkey::Pubkey> {
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
