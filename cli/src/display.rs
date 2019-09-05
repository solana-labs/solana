use console::style;

// Pretty print a "name value"
pub fn println_name_value(name: &str, value: &str) {
    let styled_value = if value == "" {
        style("(not set)").italic()
    } else {
        style(value)
    };
    println!("{} {}", style(name).bold(), styled_value);
}

pub fn println_name_value_or(name: &str, value: &str, default_value: &str) {
    if value == "" {
        println!(
            "{} {} {}",
            style(name).bold(),
            style(default_value),
            style("(default)").italic()
        );
    } else {
        println!("{} {}", style(name).bold(), style(value));
    };
}
