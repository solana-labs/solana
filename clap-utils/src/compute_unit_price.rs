use {crate::ArgConstant, clap::Arg};

pub const COMPUTE_UNIT_PRICE_ARG: ArgConstant<'static> = ArgConstant {
    name: "compute_unit_price",
    long: "--with-compute-unit-price",
    help: "Set the price in micro-lamports of each transaction compute unit",
};

pub fn compute_unit_price_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(COMPUTE_UNIT_PRICE_ARG.name)
        .long(COMPUTE_UNIT_PRICE_ARG.long)
        .takes_value(true)
        .value_name("MICRO-LAMPORTS")
        .help(COMPUTE_UNIT_PRICE_ARG.help)
}
