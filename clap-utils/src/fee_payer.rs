use {
    crate::{input_validators, ArgConstant},
    clap::Arg,
};

pub const FEE_PAYER_ARG: ArgConstant<'static> = ArgConstant {
    name: "fee_payer",
    long: "fee-payer",
    help: "Specify the fee-payer account. This may be a keypair file, the ASK keyword \n\
           or the pubkey of an offline signer, provided an appropriate --signer argument \n\
           is also passed. Defaults to the client keypair.",
};

pub fn fee_payer_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(FEE_PAYER_ARG.name)
        .long(FEE_PAYER_ARG.long)
        .takes_value(true)
        .value_name("KEYPAIR")
        .validator(input_validators::is_valid_signer)
        .help(FEE_PAYER_ARG.help)
}
