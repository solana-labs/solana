use {crate::ArgConstant, clap::Arg};

pub const MEMO_ARG: ArgConstant<'static> = ArgConstant {
    name: "memo",
    long: "--with-memo",
    help: "Specify a memo string to include in the transaction.",
};

pub fn memo_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(MEMO_ARG.name)
        .long(MEMO_ARG.long)
        .takes_value(true)
        .value_name("MEMO")
        .help(MEMO_ARG.help)
}
