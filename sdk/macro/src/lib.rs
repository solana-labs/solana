//! Convenience macro to declare a static public key and functions to interact with it
//!
//! Input: a single literal base58 string representation of a program's id

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::{Delimiter, Span, TokenTree};
use quote::{quote, ToTokens};
use std::convert::TryFrom;
use syn::{
    bracketed,
    parse::{Parse, ParseStream, Result},
    parse_macro_input,
    punctuated::Punctuated,
    token::Bracket,
    Expr, Ident, LitByte, LitStr, Path, Token,
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
            /// The static program ID
            pub static ID: ::solana_sdk::pubkey::Pubkey = #id;

            /// Confirms that a given pubkey is equivalent to the program ID
            pub fn check_id(id: &::solana_sdk::pubkey::Pubkey) -> bool {
                id == &ID
            }

            /// Returns the program ID
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
            val @ _ => Err(syn::Error::new_spanned(
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
            // Note: `proc_macro2::Span::resolved_at` is currently gated with cfg(procmacro2_semver_exempt)
            // Once this gate is removed, we will no longer need to use 'unwrap()' to call
            // the underling `proc_macro::Span::resolved_at` method.
            let new_span: Span = t.span().unwrap().resolved_at(respan_using.unwrap()).into();
            t.set_span(new_span);
            t
        })
        .collect();
    return TokenStream::from(to_respan);
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
