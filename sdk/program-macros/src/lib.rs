//! Macros to generate program methods and structures from a single input

extern crate proc_macro;

use inflector::Inflector;
use proc_macro::TokenStream;
use proc_macro2::Span;
use proc_macro_error::*;
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream, Result},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    token::Comma,
    ExprCall, Fields, FnArg, Ident, ItemEnum, Lit, Meta, NestedMeta, Path,
    Type,
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

    fn format_account_meta(&self) -> proc_macro2::TokenStream {
        let account_meta_path: Path = if self.is_writable {
            parse_quote!(::solana_sdk::instruction::AccountMeta::new)
        } else {
            parse_quote!(::solana_sdk::instruction::AccountMeta::new_readonly)
        };
        let ident = &self.ident;
        let is_signer = &self.is_signer;
        if self.is_optional {
            quote! {
                if let Some(#ident) = #ident {
                    account_metas.push(#account_meta_path(#ident, #is_signer));
                }
            }
        } else if self.allows_multiple {
            quote! {
                for pubkey in #ident.into_iter() {
                    account_metas.push(#account_meta_path(pubkey, #is_signer));
                }
            }
        } else {
            quote! {
                account_metas.push(#account_meta_path(#ident, #is_signer));
            }
        }
    }

    fn format_arg(&self) -> FnArg {
        let account_ident = &self.ident;
        let account_type: Type = if self.is_optional {
            parse_quote!(Option<::solana_sdk::pubkey::Pubkey>)
        } else if self.allows_multiple {
            parse_quote!(Vec<::solana_sdk::pubkey::Pubkey>)
        } else {
            parse_quote!(::solana_sdk::pubkey::Pubkey)
        };
        parse_quote!(#account_ident: #account_type)
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

fn build_helper_fns(
    program_details: &ProgramDetails,
    program_id: ProgramId,
) -> proc_macro2::TokenStream {
    let mut stream = proc_macro2::TokenStream::new();
    let ident = &program_details.instruction_enum.ident;
    for (variant, variant_details) in program_details
        .instruction_enum
        .variants
        .iter()
        .zip(program_details.variants.iter())
    {
        let fn_ident = Ident::new(
            &variant.ident.to_string().to_snake_case(),
            Span::call_site(),
        );
        let variant_ident = &variant.ident;
        let mut fields: Punctuated<Ident, Comma> = Punctuated::new();
        let mut args: Punctuated<FnArg, Comma> = Punctuated::new();

        let mut accounts_stream = proc_macro2::TokenStream::new();
        accounts_stream.extend(quote! {
            let mut account_metas: Vec<::solana_sdk::instruction::AccountMeta> = vec![];
        });
        for account in &variant_details.account_details {
            let account_meta = account.format_account_meta();
            accounts_stream.extend(quote! {
                #account_meta
            });

            args.push(account.format_arg());
        }

        for field in variant.fields.iter() {
            let ident = field.ident.as_ref().unwrap();
            fields.push(ident.clone());

            if let Type::Path(path) = &field.ty {
                let arg: FnArg = parse_quote!(#ident: #path);
                args.push(arg);
            }
        }

        stream.extend(
            quote!( pub fn #fn_ident( #args ) -> ::solana_sdk::instruction::Instruction {
                    #accounts_stream
                    ::solana_sdk::instruction::Instruction::new(
                        #program_id,
                        &#ident::#variant_ident{ #fields },
                        account_metas,
                    )
                }
            ),
        );
    }
    stream
}

#[proc_macro_error]
#[proc_macro_attribute]
pub fn instructions(attr: TokenStream, item: TokenStream) -> TokenStream {
    let program_id = parse_macro_input!(attr as ProgramId);
    let program_details = parse_macro_input!(item as ProgramDetails);

    let instruction_enum = build_instruction_enum(&program_details);
    let helper_fns = build_helper_fns(&program_details, program_id);

    TokenStream::from(quote! {
        #instruction_enum
        #helper_fns
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::Stmt;

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

    #[test]
    fn test_format_account_meta() {
        let mut account_details = AccountDetails {
            ident: Ident::new("test", Span::call_site()),
            desc: "Description".to_string(),
            is_signer: false,
            is_writable: false,
            is_optional: false,
            allows_multiple: false,
        };
        let account_meta = account_details.format_account_meta();
        let account_meta: Stmt = parse_quote!(#account_meta);
        let expected_account_meta: Stmt = parse_quote!(
            account_metas.push(::solana_sdk::instruction::AccountMeta::new_readonly(test, false));
        );
        assert_eq!(account_meta, expected_account_meta);

        account_details.is_signer = true;
        let account_meta = account_details.format_account_meta();
        let account_meta: Stmt = parse_quote!(#account_meta);
        let expected_account_meta: Stmt = parse_quote!(
            account_metas.push(::solana_sdk::instruction::AccountMeta::new_readonly(test, true));
        );
        assert_eq!(account_meta, expected_account_meta);

        account_details.is_writable = true;
        let account_meta = account_details.format_account_meta();
        let account_meta: Stmt = parse_quote!(#account_meta);
        let expected_account_meta: Stmt = parse_quote!(
            account_metas.push(::solana_sdk::instruction::AccountMeta::new(test, true));
        );
        assert_eq!(account_meta, expected_account_meta);

        account_details.is_optional = true;
        let account_meta = account_details.format_account_meta();
        let account_meta: Stmt = parse_quote!(#account_meta);
        let expected_account_meta: Stmt = parse_quote!(if let Some(test) = test {
            account_metas.push(::solana_sdk::instruction::AccountMeta::new(test, true));
        });
        assert_eq!(account_meta, expected_account_meta);

        account_details.is_optional = false;
        account_details.allows_multiple = true;
        let account_meta = account_details.format_account_meta();
        let account_meta: Stmt = parse_quote!(#account_meta);
        let expected_account_meta: Stmt = parse_quote!(for pubkey in test.into_iter() {
            account_metas.push(::solana_sdk::instruction::AccountMeta::new(pubkey, true));
        });
        assert_eq!(account_meta, expected_account_meta);
    }

    #[test]
    fn test_format_arg() {
        let mut account_details = AccountDetails {
            ident: Ident::new("test", Span::call_site()),
            desc: "Description".to_string(),
            is_signer: false,
            is_writable: false,
            is_optional: false,
            allows_multiple: false,
        };
        let expected_arg: FnArg = parse_quote!(test: ::solana_sdk::pubkey::Pubkey);
        assert_eq!(account_details.format_arg(), expected_arg);

        account_details.is_optional = true;
        let expected_arg: FnArg = parse_quote!(test: Option<::solana_sdk::pubkey::Pubkey>);
        assert_eq!(account_details.format_arg(), expected_arg);

        account_details.is_optional = false;
        account_details.allows_multiple = true;
        let expected_arg: FnArg = parse_quote!(test: Vec<::solana_sdk::pubkey::Pubkey>);
        assert_eq!(account_details.format_arg(), expected_arg);
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

    #[test]
    fn test_build_instruction_enum() {
        let program_details = build_test_program_details();
        let documented_enum = build_instruction_enum(&program_details);
        let documented_enum: ItemEnum = parse_quote!(#documented_enum);

        let expected_enum: ItemEnum = parse_quote! {
            #[derive(Clone, Debug, PartialEq)]
            pub enum TestInstruction {
                /// Test instruction
                #[doc = "<br/>"]
                #[doc = "* Accounts expected by this instruction:"]
                #[doc = "  0. `[WRITABLE, SIGNER]` Description"]
                #[doc = "  1. (Optional) `[]` Different"]
                Test {
                    /// Field doc
                    lamports: u64
                },
                /// Multiple signers
                #[doc = "<br/>"]
                #[doc = "* Accounts expected by this instruction:"]
                #[doc = "  * (Multiple) `[SIGNER]` A signer"]
                Multiple,
            }
        };
        assert_eq!(documented_enum, expected_enum);
    }

    #[test]
    fn test_build_helper_fns() {
        let program_id: ExprCall = parse_quote! { program::id() };
        let program_id = ProgramId(program_id);

        let program_details = build_test_program_details();
        let helper_fns = build_helper_fns(&program_details, program_id);
        let helper_fns: Vec<Stmt> = parse_quote!(#helper_fns);

        let expected_fns: Vec<Stmt> = parse_quote! {
            pub fn test(
                test_account: ::solana_sdk::pubkey::Pubkey,
                another: ::solana_sdk::pubkey::Pubkey,
                lamports: u64,
            ) -> ::solana_sdk::instruction::Instruction {
                let mut account_metas: Vec<::solana_sdk::instruction::AccountMeta> = vec![];
                account_metas.push(
                    ::solana_sdk::instruction::AccountMeta::new(test_account, true)
                );
                if let Some(another) = another {
                    account_metas.push(
                        ::solana_sdk::instruction::AccountMeta::new_readonly(another, false)
                    );
                }
                ::solana_sdk::instruction::Instruction::new(
                    program::id(),
                    &TestInstruction::Transfer{lamports,},
                    account_metas,
                )
            }

            pub fn multiple(
                signers: Vec<::solana_sdk::pubkey::Pubkey>
            ) -> ::solana_sdk::instruction::Instruction {
                let mut account_metas: Vec<::solana_sdk::instruction::AccountMeta> = vec![];
                for pubkey in signers.into_iter() {
                    account_metas.push(
                        ::solana_sdk::instruction::AccountMeta::new_readonly(pubkey, true)
                    );
                }

                ::solana_sdk::instruction::Instruction::new(
                    program::id(),
                    &TestInstruction::Multiple{},
                    account_metas,
                )
            }
        };
        assert_eq!(helper_fns[1], expected_fns[1]);
    }
}
