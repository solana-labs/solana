//! Just logging!
use {
    std::io::Write,
    termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor},
};

fn log_magenta(msg: &str) {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);

    stdout
        .set_color(ColorSpec::new().set_fg(Some(Color::Magenta)).set_bold(true))
        .unwrap();

    writeln!(&mut stdout, "\n[PAYTUBE]: INFO: {}\n", msg).unwrap();

    stdout.reset().unwrap();
}

pub(crate) fn setup_solana_logging() {
    #[rustfmt::skip]
    solana_logger::setup_with_default(
        "solana_rbpf::vm=debug,\
            solana_runtime::message_processor=debug,\
            solana_runtime::system_instruction_processor=trace",
    );
}

pub(crate) fn creating_paytube_channel() {
    log_magenta("Creating PayTube channel...");
}

pub(crate) fn processing_transactions(num_transactions: usize) {
    log_magenta("Processing PayTube transactions with the SVM API...");
    log_magenta(&format!("Number of transactions: {}", num_transactions));
}

pub(crate) fn settling_to_base_chain(num_transactions: usize) {
    log_magenta("Settling results from PayTube to the base chain...");
    log_magenta(&format!(
        "Number of settlement transactions: {}",
        num_transactions
    ));
}

pub(crate) fn channel_closed() {
    log_magenta("PayTube channel closed.");
}
