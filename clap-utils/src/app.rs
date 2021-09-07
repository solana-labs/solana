use clap::{App, Arg, Shell, SubCommand};
use std::io::stdout;

/// Trait that adds common solana arguments and subcommands to clap App.
pub trait SolanaApp<'ab, 'v> {
    /// Add common arguments and subcommands to provided App.
    fn finalize(self) -> App<'ab, 'v>;
    /// Process the common subcommands and arguments
    fn process_common(&mut self, name: &str);
}

impl<'ab, 'v> SolanaApp<'ab, 'v> for App<'ab, 'v> {
    fn finalize(self) -> App<'ab, 'v> {
        self.subcommand(
            SubCommand::with_name("completion")
                .about("Generate completion scripts for various shells")
                .arg(
                    Arg::with_name("shell")
                        .long("shell")
                        .short("s")
                        .takes_value(true)
                        .possible_values(&["bash", "fish", "zsh", "powershell", "elvish"])
                        .default_value("bash"),
                ),
        )
    }

    fn process_common(&mut self, name: &str) {
        let matches = self.clone().get_matches();
        match matches.subcommand() {
            // Autocompletion Command
            ("completion", Some(matches)) => {
                let shell_choice = match matches.value_of("shell") {
                    Some("bash") => Shell::Bash,
                    Some("fish") => Shell::Fish,
                    Some("zsh") => Shell::Zsh,
                    Some("powershell") => Shell::PowerShell,
                    Some("elvish") => Shell::Elvish,
                    // This is safe, since we assign default_value and possible_values
                    // are restricted
                    _ => unreachable!(),
                };
                self.gen_completions_to(name, shell_choice, &mut stdout());
                std::process::exit(0);
            }
            (_, _) => {}
        }
    }
}
