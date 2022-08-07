use {proc_macro::TokenStream, quote::quote};

/// Wraps a block of code in Measure::start and stop without changing how
/// the code needs to be written.
///
/// # Arguments
///
/// * `measure_name` - The name of the measure struct for the block
/// * `block` - The block of code you want to measure
///
/// # Example
///
/// ```
/// use solana_measure_block::measure_block;
/// use solana_measure::measure::Measure;
/// measure_block!(my_measure,
///     let x = 5;
/// );
///
/// // can access assigned variables outside the macro call
/// assert_eq!(x, 5);
/// // a new measure is created with the name given to the `measure_block!` macro
/// let my_time_us = my_measure.as_us();
/// ```
#[proc_macro]
pub fn measure_block(input: TokenStream) -> TokenStream {
    // Convert input int proc_macro2 format
    let input = proc_macro2::TokenStream::from(input);

    // Make sure input is in the expected format
    let mut token_trees = input.into_iter();
    let measure_name_tt = token_trees.next().expect("missing 'measure_name'");
    let comma_tt = token_trees
        .next()
        .expect("expected comma between 'measure_name' and 'block'");
    if let proc_macro2::TokenTree::Punct(punct) = comma_tt {
        assert!(punct.as_char() == ',');
    } else {
        panic!("incorrect input format: missing code block");
    }

    let measure_name = if let proc_macro2::TokenTree::Ident(measure_name) = measure_name_tt {
        measure_name
    } else {
        panic!("incorrect input: missing measure_name")
    };

    let measure_name_str = measure_name.to_string();

    let expanded = quote! {
        let mut #measure_name = Measure::start(&#measure_name_str);
        #(#token_trees)*
        #measure_name.stop();
    };
    TokenStream::from(expanded)
}
