use {
    crate::ArgConstant,
    clap::{value_parser, Arg},
};

pub const COMPUTE_UNIT_PRICE_ARG: ArgConstant<'static> = ArgConstant {
    name: "compute_unit_price",
    long: "--with-compute-unit-price",
    help: "Set compute unit price for transaction, in increments of 0.000001 lamports per compute unit.",
};

pub const COMPUTE_UNIT_LIMIT_ARG: ArgConstant<'static> = ArgConstant {
    name: "compute_unit_limit",
    long: "--with-compute-unit-limit",
    help: "Set compute unit limit for transaction.",
};

pub fn compute_unit_price_arg<'a>() -> Arg<'a> {
    Arg::new(COMPUTE_UNIT_PRICE_ARG.name)
        .long(COMPUTE_UNIT_PRICE_ARG.long)
        .takes_value(true)
        .value_name("COMPUTE-UNIT-PRICE")
        .value_parser(value_parser!(u64))
        .help(COMPUTE_UNIT_PRICE_ARG.help)
}

pub fn compute_unit_limit_arg<'a>() -> Arg<'a> {
    Arg::new(COMPUTE_UNIT_LIMIT_ARG.name)
        .long(COMPUTE_UNIT_LIMIT_ARG.long)
        .takes_value(true)
        .value_name("COMPUTE-UNIT-LIMIT")
        .value_parser(value_parser!(u32))
        .help(COMPUTE_UNIT_LIMIT_ARG.help)
}
