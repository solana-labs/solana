//! Macros to generate program methods and structures from a single input

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro_error::*;
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream, Result},
    parse_macro_input,
    ExprCall, Fields, Ident, ItemEnum, Lit, Meta, NestedMeta,
};

struct ProgramId(ExprCall);
impl Parse for ProgramId {
    fn parse(input: ParseStream) -> Result<Self> {
        let path = input.parse()?;
        Ok(ProgramId(path))
    }
}

impl ToTokens for ProgramId {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let id = &self.0;
        tokens.extend(quote! { #id });
    }
}

fn parse_account(account: NestedMeta) -> AccountDetails {
    if let NestedMeta::Meta(account_meta) = account.clone() {
        if let Meta::List(account_details) = account_meta {
            let ident = account_details
                .path
                .get_ident()
                .expect("account should have identifier")
                .clone();
            let mut desc = "".to_string();
            let mut is_signer = false;
            let mut is_writable = false;
            let mut is_optional = false;
            let mut allows_multiple = false;
            for nested in account_details.nested {
                if let NestedMeta::Meta(account_detail) = nested {
                    match account_detail {
                        Meta::Path(path) => {
                            if path.is_ident("SIGNER") {
                                is_signer = true;
                            } else if path.is_ident("WRITABLE") {
                                is_writable = true;
                            } else if path.is_ident("optional") {
                                if allows_multiple {
                                    abort!(path, "account cannot be optional and allow multiples");
                                }
                                is_optional = true;
                            } else if path.is_ident("multiple") {
                                if is_optional {
                                    abort!(path, "account cannot be optional and allow multiples");
                                }
                                allows_multiple = true;
                            } else {
                                abort!(path, "unrecognized account detail");
                            }
                        }
                        Meta::NameValue(name_value) => {
                            if name_value.path.is_ident("desc") {
                                if let Lit::Str(doc) = name_value.lit {
                                    desc = doc.value();
                                }
                            } else {
                                abort!(name_value, "unrecognized account detail");
                            }
                        }
                        Meta::List(list) => abort!(list, "unrecognized account detail"),
                    }
                } else {
                    abort!(nested, "unrecognized account detail format");
                }
            }
            return AccountDetails {
                ident,
                desc,
                is_signer,
                is_writable,
                is_optional,
                allows_multiple,
            };
        }
    }
    abort!(account, "unrecognized accounts format");
}

struct AccountDetails {
    ident: Ident,
    desc: String,
    is_signer: bool,
    is_writable: bool,
    is_optional: bool,
    allows_multiple: bool,
}

struct VariantDetails {
    account_details: Vec<AccountDetails>,
}

struct ProgramDetails {
    instruction_enum: ItemEnum,
    variants: Vec<VariantDetails>,
}

impl Parse for ProgramDetails {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut instruction_enum = ItemEnum::parse(input)?;
        let mut variants: Vec<VariantDetails> = vec![];
        for variant in instruction_enum.variants.iter_mut() {
            let mut account_details: Vec<AccountDetails> = vec![];
            variant.attrs.retain(|attr| {
                if attr.path.is_ident("accounts") {
                    let accounts_parse = attr.parse_meta();
                    if accounts_parse.is_err() {
                        abort!(attr, "unrecognized accounts format");
                    }
                    match accounts_parse.unwrap() {
                        Meta::List(accounts_list) => {
                            for account in accounts_list.nested {
                                account_details.push(parse_account(account));
                            }
                        }
                        Meta::Path(path) => abort!(path, "missing accounts list"),
                        Meta::NameValue(_) => abort!(attr, "unrecognized accounts format"),
                    }
                    false
                } else {
                    true
                }
            });

            if let Fields::Unnamed(fields) = &variant.fields {
                abort!(fields, "macro does not support unnamed variant fields");
            }

            variants.push(VariantDetails { account_details });
        }

        Ok(ProgramDetails {
            instruction_enum,
            variants,
        })
    }
}

#[proc_macro_error]
#[proc_macro_attribute]
pub fn instructions(attr: TokenStream, item: TokenStream) -> TokenStream {
    let _program_id = parse_macro_input!(attr as ProgramId);
    let _program_details = parse_macro_input!(item as ProgramDetails);
    TokenStream::new()
}
