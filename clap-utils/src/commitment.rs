use crate::ArgConstant;
use clap::Arg;

pub const COMMITMENT_ARG: ArgConstant<'static> = ArgConstant {
    name: "commitment",
    long: "commitment",
    help: "Return information at the selected commitment level",
};

pub fn commitment_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(COMMITMENT_ARG.name)
        .long(COMMITMENT_ARG.long)
        .takes_value(true)
        .possible_values(&["default", "max", "recent", "root"])
        .value_name("COMMITMENT_LEVEL")
        .help(COMMITMENT_ARG.help)
}
