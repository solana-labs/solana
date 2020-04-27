use std::process::exit;

fn press_enter() {
    // On windows, where installation happens in a console that may have opened just for this
    // purpose, give the user an opportunity to see the error before the window closes.
    if cfg!(windows) && atty::is(atty::Stream::Stdin) {
        println!();
        println!("Press the Enter key to continue.");

        use std::io::BufRead;
        let stdin = std::io::stdin();
        let stdin = stdin.lock();
        let mut lines = stdin.lines();
        lines.next();
    }
}

fn main() {
    solana_install::main_init().unwrap_or_else(|err| {
        println!("Error: {}", err);
        press_enter();
        exit(1);
    });
    press_enter();
}
