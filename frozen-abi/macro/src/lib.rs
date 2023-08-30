extern crate proc_macro;

// This file littered with these essential cfgs so ensure them.
#[cfg(not(any(RUSTC_WITH_SPECIALIZATION, RUSTC_WITHOUT_SPECIALIZATION)))]
compile_error!("rustc_version is missing in build dependency and build.rs is not specified");

#[cfg(any(RUSTC_WITH_SPECIALIZATION, RUSTC_WITHOUT_SPECIALIZATION))]
use proc_macro::TokenStream;

// Define dummy macro_attribute and macro_derive for stable rustc

#[cfg(RUSTC_WITHOUT_SPECIALIZATION)]
#[proc_macro_attribute]
pub fn frozen_abi(_attrs: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[cfg(RUSTC_WITHOUT_SPECIALIZATION)]
#[proc_macro_derive(AbiExample)]
pub fn derive_abi_sample(_item: TokenStream) -> TokenStream {
    "".parse().unwrap()
}

#[cfg(RUSTC_WITHOUT_SPECIALIZATION)]
#[proc_macro_derive(AbiEnumVisitor)]
pub fn derive_abi_enum_visitor(_item: TokenStream) -> TokenStream {
    "".parse().unwrap()
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
#[cfg(RUSTC_WITH_SPECIALIZATION)]
use quote::{quote, ToTokens};
#[cfg(RUSTC_WITH_SPECIALIZATION)]
use syn::{
    parse_macro_input, Attribute, Error, Fields, Ident, Item, ItemEnum, ItemStruct, ItemType,
    LitStr, Variant,
};

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn filter_serde_attrs(attrs: &[Attribute]) -> bool {
    fn contains_skip(tokens: TokenStream2) -> bool {
        for token in tokens.into_iter() {
            match token {
                TokenTree::Group(group) => {
                    if contains_skip(group.stream()) {
                        return true;
                    }
                }
                TokenTree::Ident(ident) => {
                    if ident == "skip" {
                        return true;
                    }
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => (),
            }
        }

        false
    }

    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }

        if contains_skip(attr.to_token_stream()) {
            return true;
        }
    }

    false
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn filter_allow_attrs(attrs: &mut Vec<Attribute>) {
    attrs.retain(|attr| {
        let ss = &attr.path().segments.first().unwrap().ident.to_string();
        ss.starts_with("allow")
    });
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn derive_abi_sample_enum_type(input: ItemEnum) -> TokenStream {
    let type_name = &input.ident;

    let mut sample_variant = quote! {};
    let mut sample_variant_found = false;

    for variant in &input.variants {
        let variant_name = &variant.ident;
        let variant = &variant.fields;
        if *variant == Fields::Unit {
            sample_variant.extend(quote! {
                #type_name::#variant_name
            });
        } else if let Fields::Unnamed(variant_fields) = variant {
            let mut fields = quote! {};
            for field in &variant_fields.unnamed {
                if !(field.ident.is_none() && field.colon_token.is_none()) {
                    unimplemented!("tuple enum: {:?}", field);
                }
                let field_type = &field.ty;
                fields.extend(quote! {
                    <#field_type>::example(),
                });
            }
            sample_variant.extend(quote! {
                #type_name::#variant_name(#fields)
            });
        } else if let Fields::Named(variant_fields) = variant {
            let mut fields = quote! {};
            for field in &variant_fields.named {
                if field.ident.is_none() || field.colon_token.is_none() {
                    unimplemented!("tuple enum: {:?}", field);
                }
                let field_type = &field.ty;
                let field_name = &field.ident;
                fields.extend(quote! {
                    #field_name: <#field_type>::example(),
                });
            }
            sample_variant.extend(quote! {
                #type_name::#variant_name{#fields}
            });
        } else {
            unimplemented!("{:?}", variant);
        }

        if !sample_variant_found {
            sample_variant_found = true;
            break;
        }
    }

    if !sample_variant_found {
        unimplemented!("empty enum");
    }

    let mut attrs = input.attrs.clone();
    filter_allow_attrs(&mut attrs);
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let result = quote! {
        #[automatically_derived]
        #( #attrs )*
        impl #impl_generics ::solana_frozen_abi::abi_example::AbiExample for #type_name #ty_generics #where_clause {
            fn example() -> Self {
                ::log::info!(
                    "AbiExample for enum: {}",
                    std::any::type_name::<#type_name #ty_generics>()
                );
                #sample_variant
            }
        }
    };
    result.into()
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn derive_abi_sample_struct_type(input: ItemStruct) -> TokenStream {
    let type_name = &input.ident;
    let mut sample_fields = quote! {};
    let fields = &input.fields;

    match fields {
        Fields::Named(_) => {
            for field in fields {
                let field_name = &field.ident;
                sample_fields.extend(quote! {
                    #field_name: AbiExample::example(),
                });
            }
            sample_fields = quote! {
                { #sample_fields }
            }
        }
        Fields::Unnamed(_) => {
            for _ in fields {
                sample_fields.extend(quote! {
                    AbiExample::example(),
                });
            }
            sample_fields = quote! {
                ( #sample_fields )
            }
        }
        _ => unimplemented!("fields: {:?}", fields),
    }

    let mut attrs = input.attrs.clone();
    filter_allow_attrs(&mut attrs);
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let turbofish = ty_generics.as_turbofish();

    let result = quote! {
        #[automatically_derived]
        #( #attrs )*
        impl #impl_generics ::solana_frozen_abi::abi_example::AbiExample for #type_name #ty_generics #where_clause {
            fn example() -> Self {
                ::log::info!(
                    "AbiExample for struct: {}",
                    std::any::type_name::<#type_name #ty_generics>()
                );
                use ::solana_frozen_abi::abi_example::AbiExample;

                #type_name #turbofish #sample_fields
            }
        }
    };

    result.into()
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
#[proc_macro_derive(AbiExample)]
pub fn derive_abi_sample(item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as Item);

    match item {
        Item::Struct(input) => derive_abi_sample_struct_type(input),
        Item::Enum(input) => derive_abi_sample_enum_type(input),
        _ => Error::new_spanned(item, "AbiSample isn't applicable; only for struct and enum")
            .to_compile_error()
            .into(),
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn do_derive_abi_enum_visitor(input: ItemEnum) -> TokenStream {
    let type_name = &input.ident;
    let mut serialized_variants = quote! {};
    let mut variant_count: u64 = 0;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    for variant in &input.variants {
        // Don't digest a variant with serde(skip)
        if filter_serde_attrs(&variant.attrs) {
            continue;
        };
        let sample_variant = quote_sample_variant(type_name, &ty_generics, variant);
        variant_count = if let Some(variant_count) = variant_count.checked_add(1) {
            variant_count
        } else {
            break;
        };
        serialized_variants.extend(quote! {
            #sample_variant;
            Serialize::serialize(&sample_variant, digester.create_enum_child()?)?;
        });
    }

    let type_str = format!("{type_name}");
    (quote! {
        impl #impl_generics ::solana_frozen_abi::abi_example::AbiEnumVisitor for #type_name #ty_generics #where_clause {
            fn visit_for_abi(&self, digester: &mut ::solana_frozen_abi::abi_digester::AbiDigester) -> ::solana_frozen_abi::abi_digester::DigestResult {
                let enum_name = #type_str;
                use ::serde::ser::Serialize;
                use ::solana_frozen_abi::abi_example::AbiExample;
                digester.update_with_string(format!("enum {} (variants = {})", enum_name, #variant_count));
                #serialized_variants
                digester.create_child()
            }
        }
    }).into()
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
#[proc_macro_derive(AbiEnumVisitor)]
pub fn derive_abi_enum_visitor(item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as Item);

    match item {
        Item::Enum(input) => do_derive_abi_enum_visitor(input),
        _ => Error::new_spanned(item, "AbiEnumVisitor not applicable; only for enum")
            .to_compile_error()
            .into(),
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn quote_for_test(
    test_mod_ident: &Ident,
    type_name: &Ident,
    expected_digest: &str,
) -> TokenStream2 {
    // escape from nits.sh...
    let p = Ident::new(&("ep".to_owned() + "rintln"), Span::call_site());
    quote! {
        #[cfg(test)]
        mod #test_mod_ident {
            use super::*;
            use ::solana_frozen_abi::abi_example::{AbiExample, AbiEnumVisitor};

            #[test]
            fn test_abi_digest() {
                ::solana_logger::setup();
                let mut digester = ::solana_frozen_abi::abi_digester::AbiDigester::create();
                let example = <#type_name>::example();
                let result = <_>::visit_for_abi(&&example, &mut digester);
                let mut hash = digester.finalize();
                // pretty-print error
                if result.is_err() {
                    ::log::error!("digest error: {:#?}", result);
                }
                result.unwrap();
                let actual_digest = format!("{}", hash);
                if ::std::env::var("SOLANA_ABI_BULK_UPDATE").is_ok() {
                    if #expected_digest != actual_digest {
                        #p!("sed -i -e 's/{}/{}/g' $(git grep --files-with-matches frozen_abi)", #expected_digest, hash);
                    }
                    ::log::warn!("Not testing the abi digest under SOLANA_ABI_BULK_UPDATE!");
                } else {
                    if let Ok(dir) = ::std::env::var("SOLANA_ABI_DUMP_DIR") {
                        assert_eq!(#expected_digest, actual_digest, "Possibly ABI changed? Examine the diff in SOLANA_ABI_DUMP_DIR!: \n$ diff -u {}/*{}* {}/*{}*", dir, #expected_digest, dir, actual_digest);
                    } else {
                        assert_eq!(#expected_digest, actual_digest, "Possibly ABI changed? Confirm the diff by rerunning before and after this test failed with SOLANA_ABI_DUMP_DIR!");
                    }
                }
            }
        }
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn test_mod_name(type_name: &Ident) -> Ident {
    Ident::new(&format!("{type_name}_frozen_abi"), Span::call_site())
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn frozen_abi_type_alias(input: ItemType, expected_digest: &str) -> TokenStream {
    let type_name = &input.ident;
    let test = quote_for_test(&test_mod_name(type_name), type_name, expected_digest);
    let result = quote! {
        #input
        #test
    };
    result.into()
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn frozen_abi_struct_type(input: ItemStruct, expected_digest: &str) -> TokenStream {
    let type_name = &input.ident;
    let test = quote_for_test(&test_mod_name(type_name), type_name, expected_digest);
    let result = quote! {
        #input
        #test
    };
    result.into()
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn quote_sample_variant(
    type_name: &Ident,
    ty_generics: &syn::TypeGenerics,
    variant: &Variant,
) -> TokenStream2 {
    let variant_name = &variant.ident;
    let variant = &variant.fields;
    if *variant == Fields::Unit {
        quote! {
            let sample_variant: #type_name #ty_generics = #type_name::#variant_name;
        }
    } else if let Fields::Unnamed(variant_fields) = variant {
        let mut fields = quote! {};
        for field in &variant_fields.unnamed {
            if !(field.ident.is_none() && field.colon_token.is_none()) {
                unimplemented!();
            }
            let ty = &field.ty;
            fields.extend(quote! {
                <#ty>::example(),
            });
        }
        quote! {
            let sample_variant: #type_name #ty_generics = #type_name::#variant_name(#fields);
        }
    } else if let Fields::Named(variant_fields) = variant {
        let mut fields = quote! {};
        for field in &variant_fields.named {
            if field.ident.is_none() || field.colon_token.is_none() {
                unimplemented!();
            }
            let field_type_name = &field.ty;
            let field_name = &field.ident;
            fields.extend(quote! {
                #field_name: <#field_type_name>::example(),
            });
        }
        quote! {
            let sample_variant: #type_name #ty_generics = #type_name::#variant_name{#fields};
        }
    } else {
        unimplemented!("variant: {:?}", variant)
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
fn frozen_abi_enum_type(input: ItemEnum, expected_digest: &str) -> TokenStream {
    let type_name = &input.ident;
    let test = quote_for_test(&test_mod_name(type_name), type_name, expected_digest);
    let result = quote! {
        #input
        #test
    };
    result.into()
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
#[proc_macro_attribute]
pub fn frozen_abi(attrs: TokenStream, item: TokenStream) -> TokenStream {
    let mut expected_digest: Option<String> = None;
    let attrs_parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("digest") {
            expected_digest = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else {
            Err(meta.error("unsupported \"frozen_abi\" property"))
        }
    });
    parse_macro_input!(attrs with attrs_parser);

    let Some(expected_digest) = expected_digest else {
        return Error::new_spanned(
            TokenStream2::from(item),
            "the required \"digest\" = ... attribute is missing.",
        )
        .to_compile_error()
        .into();
    };

    let item = parse_macro_input!(item as Item);
    match item {
        Item::Struct(input) => frozen_abi_struct_type(input, &expected_digest),
        Item::Enum(input) => frozen_abi_enum_type(input, &expected_digest),
        Item::Type(input) => frozen_abi_type_alias(input, &expected_digest),
        _ => Error::new_spanned(
            item,
            "frozen_abi isn't applicable; only for struct, enum and type",
        )
        .to_compile_error()
        .into(),
    }
}
