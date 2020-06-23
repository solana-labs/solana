extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use proc_macro_error::*;
use quote::{quote, ToTokens};
use std::convert::TryFrom;
use syn::{
    bracketed,
    parse::{Parse, ParseStream, Result},
    parse_macro_input,
    punctuated::Punctuated,
    token::{Bracket, Comma},
    Attribute, Data, DataEnum, DeriveInput, Expr, Field, Fields, Ident, Lit, LitByte, LitStr, Meta,
    MetaList, MetaNameValue, NestedMeta, Token, Variant,
};

struct Id(proc_macro2::TokenStream);
impl Parse for Id {
    fn parse(input: ParseStream) -> Result<Self> {
        let token_stream = if input.peek(LitStr) {
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

/// Convenience macro to declare a static public key and functions to interact with it
///
/// Input: a single literal base58 string representation of a program's id
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

/// Convenience macro to build a program Instruction enum
///
/// Input: a verbose program-instruction enum. Output: a second enum with account fields
/// converted to documentation and variants trimmed to the fields needed for transaction
/// instructions, as well as `From` implementation between them.
///
/// The Verbose enum can be tagged with the `instruction_derive` attribute to pass desired
/// derivations to the resulting program Instruction enum.
///
/// Acccount fields should be tagged with the `account` attribute, including optional list items
/// `signer` and/or `writable`. Account fields must also have an `index` name-value attribute, and
/// may carry an optional `desc` name-value description attribute.
///
/// Example verbose enum:
///
/// ```
/// #[repr(C)]
/// #[derive(ProgramInstruction)]
/// pub enum TestInstructionVerbose {
/// #[doc = "Transfer lamports"]
///     Transfer {
///         #[account(signer, writable)]
///         #[desc = "Funding account"]
///         #[index = 0]
///         funding_account: u8,
///
///         #[account(writable)]
///         #[desc = "Recipient account"]
///         #[index = 1]
///         recipient_account: u8,
///
///         #[doc = "The `u64` parameter specifies the transfer amount"]
///         lamports: u64,
///     },
/// }
/// ```
///
/// Example new enum:
///
/// ```
/// #[repr(C)]
/// #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
/// pub enum TestInstruction {
///     /// Transfer lamports
///     ///
///     /// The `u64` parameter specifies the transfer amount
///     ///
///     /// ## Account references
///     /// 0. `[signer, writable]` Funding account
///     /// 1. `[writable]` Recipient account
///     Transfer(u64),
/// }
/// ```

#[proc_macro_error]
#[proc_macro_derive(
    ProgramInstruction,
    attributes(account, desc, index, instruction_derive)
)]
pub fn program_instruction(input: TokenStream) -> TokenStream {
    const IDENT_SUFFIX: &str = "Verbose";

    let input = parse_macro_input!(input as DeriveInput);
    let original_ident = input.ident.clone();
    let mut tokens = proc_macro2::TokenStream::new();
    if let Data::Enum(mut instruction_set) = input.data {
        // Strip ident suffix from enum ident to use as ident for new enum
        let mut ident_str = input.ident.to_string();
        if ident_str.ends_with(IDENT_SUFFIX) {
            ident_str = ident_str.replace(IDENT_SUFFIX, "");
        } else {
            abort!(
                input.ident.span(),
                "ProgramInstruction enum names must end with Verbose"
            );
        }
        let ident = Ident::new(&ident_str, Span::call_site());

        let (enum_stream, from_stream) = handle_enum_variants(&mut instruction_set, ident.clone());

        // Pass on specified derive attributes; preserve other attributes, like `#[repr(C)]`
        for attr in input.attrs {
            if attr.path.is_ident("instruction_derive") {
                if let Meta::List(MetaList { ref nested, .. }) = attr.parse_meta().unwrap() {
                    tokens.extend(quote! {
                        #[derive(#nested)]
                    });
                }
            } else {
                tokens.extend(quote! {
                    #attr
                });
            }
        }

        // Build new enum
        tokens.extend(quote! {
            pub enum #ident {
                #enum_stream
            }

            impl #original_ident {
                pub fn from_instruction(instruction: #ident, accounts: Vec<u8>) -> Self {
                    match instruction {
                        #from_stream
                    }
                }
            }
        });
    } else {
        abort!(
            input.ident.span(),
            "only enums are supported by ProgramInstruction"
        );
    }
    TokenStream::from(quote! {#tokens})
}

/// Parse verbose enum variants into variants for new enum and exhaustive documentation
fn handle_enum_variants(
    enum_data: &mut DataEnum,
    enum_ident: Ident,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let mut enum_stream = proc_macro2::TokenStream::new();
    let mut from_stream = proc_macro2::TokenStream::new();
    for variant in enum_data.variants.iter_mut() {
        let mut new_attrs: Vec<Attribute> = vec![];
        let mut variant_docs: Vec<String> = vec![];

        let variant_ident = variant.ident.clone();

        // Collect variant doc attributes for formatting
        // Preserve other variant-level attributes
        for attr in variant.attrs.iter() {
            match attr.parse_meta().unwrap() {
                Meta::NameValue(MetaNameValue {
                    ref path, ref lit, ..
                }) if path.is_ident("doc") => {
                    if let Lit::Str(lit) = lit {
                        variant_docs.push(lit.value());
                    }
                }
                _ => new_attrs.push(attr.clone()),
            }
        }
        variant.attrs = new_attrs;

        let (new_fields, mut account_references) = handle_variant_fields(variant);

        // Convert variant fields to Unit or Unnamed, if account field removal results in 0 or 1
        // remaining fields respectively
        if new_fields.is_empty() {
            variant.fields = Fields::Unit;
        } else if let Fields::Named(fields_named) = &mut variant.fields {
            fields_named.named = new_fields;
        }

        // Append line-break tags to make pretty multiline documentation in markdown
        if !variant_docs.is_empty() {
            for doc in variant_docs {
                let variant_doc = format!("{}<br/>", doc);
                enum_stream.extend(quote! {
                    #[doc = #variant_doc]
                });
            }
        }

        let mut from_inner_stream = proc_macro2::TokenStream::new();
        if !account_references.is_empty() {
            // Add header to account-reference documentation
            enum_stream.extend(quote! {
                #[doc = "## Account references"]
            });
            account_references.sort_by(|a, b| a.index.cmp(&b.index));
            for (i, reference) in account_references.iter().enumerate() {
                if reference.index as usize != i {
                    abort!(
                        variant.ident.span(),
                        "ensure account indexes do not skip values, variant {}",
                        variant.ident
                    );
                }
                //  Build pretty account documentation
                let mut account_meta = String::new();
                if reference.is_writable {
                    account_meta.push_str("writable")
                }
                if reference.is_signer {
                    if !account_meta.is_empty() {
                        account_meta.push_str(", ");
                    }
                    account_meta.push_str("signer");
                }
                let account_docs = format!(
                    "{}. `[{}]` {}",
                    reference.index, account_meta, reference.desc
                );

                enum_stream.extend(quote! {
                    #[doc = #account_docs]
                });

                // Populate `from` impl for accounts
                let account_ident = &reference.ident;
                let account_index = reference.index;
                from_inner_stream.extend(quote! {
                    #account_ident: accounts[#account_index],
                });
            }
        }
        // Convert documentation and variant to TokenStream
        enum_stream.extend(quote! {
            #variant,
        });

        // Build From stream
        let mut field_stream = proc_macro2::TokenStream::new();
        for field in variant.fields.iter() {
            let field_ident = field.ident.as_ref().unwrap();
            field_stream.extend(quote! {
                #field_ident,
            });
        }
        from_stream.extend(quote! {
            #enum_ident::#variant_ident { #field_stream } => Self::#variant_ident {
                #from_inner_stream
                #field_stream
            },
        });
    }
    (enum_stream, from_stream)
}

/// Collect account information from a variant for variant docs, and remove tagged account fields
/// from variant
fn handle_variant_fields(
    variant: &mut Variant,
) -> (Punctuated<Field, Comma>, Vec<AccountReference>) {
    let mut new_fields: Punctuated<Field, Comma> = Punctuated::new();
    let mut account_references: Vec<AccountReference> = Vec::new();
    for field in variant.fields.iter_mut() {
        // Preserve fields not tagged as account
        if field
            .attrs
            .iter()
            .find(|&attr| match attr.parse_meta().unwrap() {
                Meta::Path(path) => path.is_ident("account"),
                Meta::List(list) => list.path.is_ident("account"),
                Meta::NameValue(_) => false,
            })
            .is_none()
        {
            new_fields.push(field.clone());
        } else {
            // Collect account-related documentation for formatting
            let mut account_index = String::new();
            let mut account_desc = String::new();
            let mut is_signer = false;
            let mut is_writable = false;
            for attr in field.attrs.iter() {
                match attr.parse_meta().unwrap() {
                    Meta::Path(_) => (),
                    Meta::List(list) => {
                        if list.path.is_ident("account") {
                            for meta in list.nested {
                                if let NestedMeta::Meta(Meta::Path(meta)) = meta {
                                    if meta.is_ident("signer") {
                                        is_signer = true;
                                    } else if meta.is_ident("writable") {
                                        is_writable = true;
                                    } else {
                                        let meta_ident = meta.get_ident().unwrap();
                                        abort!(
                                            meta_ident.span(),
                                            "invalid account label provided {:?}; accepted values: signer, writable",
                                            meta_ident.to_string()
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Meta::NameValue(attribute) => {
                        let MetaNameValue {
                            ref path, ref lit, ..
                        } = attribute;
                        if path.is_ident("desc") {
                            account_desc = if let Lit::Str(lit) = lit {
                                lit.value()
                            } else {
                                "".to_string()
                            };
                        }
                        if path.is_ident("index") {
                            account_index = if let Lit::Int(lit) = lit {
                                lit.base10_digits().to_string()
                            } else {
                                "".to_string()
                            };
                        }
                    }
                }
            }
            // Populate account reference
            if account_index.is_empty() {
                let field_ident = field.ident.as_ref().unwrap();
                abort!(
                    field_ident.span(),
                    "no account index, field {}",
                    field_ident
                );
            }
            account_references.push(AccountReference {
                index: account_index.parse::<usize>().unwrap(),
                is_signer,
                is_writable,
                desc: account_desc,
                ident: field.ident.as_ref().unwrap().clone(),
            });
        }
        field.attrs = vec![]; // This macro currently does not support external attributes on account fields
    }
    (new_fields, account_references)
}

struct AccountReference {
    index: usize,
    is_signer: bool,
    is_writable: bool,
    desc: String,
    ident: Ident,
}
