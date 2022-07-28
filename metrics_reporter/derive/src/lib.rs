use {
    proc_macro::TokenStream,
    quote::quote,
    syn::{parse_macro_input, Attribute, Data, DeriveInput, Field, Ident},
};

extern crate proc_macro;

#[proc_macro_derive(MetricsReporter, attributes(report_name, report_ignore, report_with,))]
pub fn metrics_reporter_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let report_name = find_metrics_reporter_name(&input.attrs)
        .expect("Struct must have a report_name attribute to derive MetricsReporter");

    let name = &input.ident;
    let metric_fields = get_metrics_fields(&input.data);

    let metric_reports = metric_fields.into_iter().map(|field| {
        let field_name = field.ident.as_ref().expect("Field must have a name");
        let field_name_str = field_name.to_string();
        let field_type = get_field_type(&field);
        let mapped_type = get_mapped_type(&field_type);

        let loader = if let Some(report_with) =
            field.attrs.iter().find(|a| a.path.is_ident("report_with"))
        {
            let expression: syn::Expr = report_with.parse_args().unwrap();
            quote! { #expression }
        } else {
            get_default_loader(field_name, &field_type, &mapped_type)
        };

        quote! {
            (#field_name_str, #loader, #mapped_type),
        }
    });

    let expanded = quote! {
        impl MetricsReporter for #name {
            fn report(&self) {
                // Publish datapoints
                datapoint_info!(
                    &#report_name,
                    #(
                        #metric_reports
                    )*
                );
            }
        }
    };

    TokenStream::from(expanded)
}

fn find_metrics_reporter_name(attrs: &[Attribute]) -> Option<String> {
    attrs
        .iter()
        .find(|a| a.path.is_ident("report_name"))
        .map(|a| {
            let literal: syn::LitStr = a
                .parse_args()
                .expect("\"report_name\" value should be a str");
            literal.value()
        })
}

fn get_metrics_fields(data: &Data) -> impl Iterator<Item = Field> + '_ {
    match data {
        syn::Data::Struct(data_struct) => match &data_struct.fields {
            syn::Fields::Named(fields) => fields
                .named
                .iter()
                .cloned()
                .filter(|field| !field.attrs.iter().any(|a| a.path.is_ident("report_ignore"))),
            _ => panic!("Can only derive MetricsReporter for named fields"),
        },
        _ => panic!("Can only derive MetricsReporter for structs"),
    }
}

fn get_field_type(field: &Field) -> Ident {
    match &field.ty {
        syn::Type::Path(type_path) => {
            let last_segment = type_path
                .path
                .segments
                .last()
                .expect("type should have segments");
            last_segment.ident.clone()
        }
        unexpected_field_type => panic!("Unexpected field type: {unexpected_field_type:?}"),
    }
}

fn get_mapped_type(field_type: &Ident) -> proc_macro2::TokenStream {
    match field_type.to_string().as_str() {
        "i64" | "i32" | "i16" | "i8" | "usize" | "u64" | "u32" | "u16" | "u8" | "AtomicI64"
        | "AtomicI32" | "AtomicI16" | "AtomicI8" | "AtomicUSize" | "AtomicU64" | "AtomicU32"
        | "AtomicU16" | "AtomicU8" => quote! {i64},
        "f64" | "f32" | "AtomicF64" | "AtomicF32" => quote! {f64},
        "bool" | "AtomicBool" => quote! {bool},
        unexpected_type => panic!("Unexpected type: {unexpected_type}"),
    }
}

fn get_default_loader(
    field_name: &Ident,
    field_type: &Ident,
    mapped_type: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match field_type.to_string().as_str() {
        "i64" | "i32" | "i16" | "i8" | "usize" | "u64" | "u32" | "u16" | "u8" | "f64" | "f32"
        | "bool" => quote! { self.#field_name as #mapped_type },
        "AtomicI64" | "AtomicI32" | "AtomicI16" | "AtomicI8" | "AtomicUSize" | "AtomicU64"
        | "AtomicU32" | "AtomicU16" | "AtomicU8" | "AtomicF32" | "AtomicF64" | "AtomicBool" => {
            quote! { self.#field_name.load(std::sync::atomic::Ordering::Relaxed) as #mapped_type }
        }
        unexpected_type => panic!("Unexpected type: {unexpected_type}"),
    }
}
