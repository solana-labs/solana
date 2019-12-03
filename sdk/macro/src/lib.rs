//! Convenience macro to declare a static public key and functions to interact with it
//!
//! bs58_string: bs58 string representation the program's id
//!
//! # Examples
//!
//! ```
//! solana_sdk::declare_id!("My!!!11111111111111111111111111111111111111");
//! ```

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use std::convert::TryFrom;
use syn::{
    parse::{Parse, ParseStream, Result},
    parse_macro_input, LitByte, LitStr,
};

struct Id([u8; 32]);
impl Parse for Id {
    fn parse(input: ParseStream) -> Result<Self> {
        let id_literal: LitStr = input.parse()?;
        if !input.is_empty() {
            let stream: proc_macro2::TokenStream = input.parse()?;
            return Err(syn::Error::new_spanned(stream, "unexpected token"));
        }

        let id_vec = bs58::decode(id_literal.value())
            .into_vec()
            .map_err(|_| syn::Error::new_spanned(&id_literal, "failed to decode base58 id"))?;
        let id_array = <[u8; 32]>::try_from(<&[u8]>::clone(&&id_vec[..]))
            .map_err(|_| syn::Error::new_spanned(&id_literal, "id is not 32 bytes long"))?;
        Ok(Id(id_array))
    }
}

impl ToTokens for Id {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let bytes = self.0.iter().map(|b| LitByte::new(*b, Span::call_site()));
        let id_array = quote! { [#(#bytes,)*] };
        tokens.extend(quote! {
            pub static ID: ::solana_sdk::pubkey::Pubkey =
                ::solana_sdk::pubkey::Pubkey::new_from_array(#id_array);

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
