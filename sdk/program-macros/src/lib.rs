//! Macros to generate program methods and structures from a single input

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro_error::*;
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream, Result},
    parse_macro_input, parse_quote, ExprCall, Fields, Ident, ItemEnum, Lit, Meta, NestedMeta,
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

impl AccountDetails {
    fn format_doc(&self, index: usize) -> String {
        let index = if self.allows_multiple {
            "* ".to_string()
        } else {
            format!("{}. ", index)
        };
        let paren_tag = if self.is_optional {
            "(Optional) "
        } else if self.allows_multiple {
            "(Multiple) "
        } else {
            ""
        };
        let mut account_meta = String::new();
        if self.is_writable {
            account_meta.push_str("WRITABLE")
        }
        if self.is_signer {
            if !account_meta.is_empty() {
                account_meta.push_str(", ");
            }
            account_meta.push_str("SIGNER");
        }
        format!("  {}{}`[{}]` {}", index, paren_tag, account_meta, self.desc)
    }
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

fn build_instruction_enum(program_details: &ProgramDetails) -> proc_macro2::TokenStream {
    let mut instruction_enum = program_details.instruction_enum.clone();
    for (variant, variant_details) in instruction_enum
        .variants
        .iter_mut()
        .zip(program_details.variants.iter())
    {
        if !variant_details.account_details.is_empty() {
            variant.attrs.push(parse_quote!(#[doc = "<br/>"]));
            variant
                .attrs
                .push(parse_quote!(#[doc = "* Accounts expected by this instruction:"]));
            for (i, account) in variant_details.account_details.iter().enumerate() {
                let account_docs = account.format_doc(i);
                variant.attrs.push(parse_quote! {
                    #[doc = #account_docs]
                });
            }
        }
    }
    quote! {#instruction_enum}
}

#[proc_macro_error]
#[proc_macro_attribute]
pub fn instructions(attr: TokenStream, item: TokenStream) -> TokenStream {
    let _program_id = parse_macro_input!(attr as ProgramId);
    let program_details = parse_macro_input!(item as ProgramDetails);

    let instruction_enum = build_instruction_enum(&program_details);

    TokenStream::from(quote! {
        #instruction_enum
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::Span;
    use syn::{punctuated::Punctuated, token::Comma};

    #[test]
    fn test_format_doc() {
        let mut account_details = AccountDetails {
            ident: Ident::new("test", Span::call_site()),
            desc: "Description".to_string(),
            is_signer: false,
            is_writable: false,
            is_optional: false,
            allows_multiple: false,
        };
        assert_eq!(account_details.format_doc(1), "  1. `[]` Description");
        assert_eq!(account_details.format_doc(2), "  2. `[]` Description");

        account_details.is_signer = true;
        assert_eq!(account_details.format_doc(2), "  2. `[SIGNER]` Description");

        account_details.is_signer = false;
        account_details.is_writable = true;
        assert_eq!(
            account_details.format_doc(2),
            "  2. `[WRITABLE]` Description"
        );

        account_details.is_signer = true;
        account_details.is_writable = true;
        assert_eq!(
            account_details.format_doc(2),
            "  2. `[WRITABLE, SIGNER]` Description"
        );

        account_details.is_optional = true;
        assert_eq!(
            account_details.format_doc(2),
            "  2. (Optional) `[WRITABLE, SIGNER]` Description"
        );

        account_details.is_optional = false;
        account_details.allows_multiple = true;
        assert_eq!(
            account_details.format_doc(2),
            "  * (Multiple) `[WRITABLE, SIGNER]` Description"
        );
    }

    fn build_test_program_details() -> ProgramDetails {
        let instruction_enum: ItemEnum = parse_quote! {
            #[derive(Clone, Debug, PartialEq)]
            pub enum TestInstruction {
                /// Test instruction
                Test {
                    /// Field doc
                    lamports: u64
                },
                /// Multiple signers
                Multiple,
            }
        };
        let account_details0 = vec![
            AccountDetails {
                ident: Ident::new("test_account", Span::call_site()),
                desc: "Description".to_string(),
                is_signer: true,
                is_writable: true,
                is_optional: false,
                allows_multiple: false,
            },
            AccountDetails {
                ident: Ident::new("another", Span::call_site()),
                desc: "Different".to_string(),
                is_signer: false,
                is_writable: false,
                is_optional: true,
                allows_multiple: false,
            },
        ];
        let account_details1 = vec![AccountDetails {
            ident: Ident::new("signers", Span::call_site()),
            desc: "A signer".to_string(),
            is_signer: true,
            is_writable: false,
            is_optional: false,
            allows_multiple: true,
        }];
        ProgramDetails {
            instruction_enum,
            variants: vec![
                VariantDetails {
                    account_details: account_details0,
                },
                VariantDetails {
                    account_details: account_details1,
                },
            ],
        }
    }
}
