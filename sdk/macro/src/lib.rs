//! Convenience macro to declare a static public key and functions to interact with it
//!
//! Input: a single literal base58 string representation of a program's id

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use std::convert::TryFrom;
use syn::{
    parse::{Parse, ParseStream, Result},
    parse_macro_input, Expr, LitByte, LitStr,
};

struct Id(proc_macro2::TokenStream);
impl Parse for Id {
    fn parse(input: ParseStream) -> Result<Self> {
        let token_stream = if input.peek(syn::LitStr) {
            let id_literal: LitStr = input.parse()?;
            let id_vec = bs58::decode(id_literal.value())
                .into_vec()
                .map_err(|_| syn::Error::new_spanned(&id_literal, "failed to decode base58 id"))?;
            let id_array = <[u8; 32]>::try_from(<&[u8]>::clone(&&id_vec[..])).map_err(|_| {
                syn::Error::new_spanned(
                    &id_literal,
                    format!("id is not 32 bytes long: len={}", id_vec.len()),
                )
            })?;
            let bytes = id_array.iter().map(|b| LitByte::new(*b, Span::call_site()));
            quote! {
                ::solana_sdk::pubkey::Pubkey::new_from_array(
                    [#(#bytes,)*]
                )
            }
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
