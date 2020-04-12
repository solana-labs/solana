pub enum OutputFormat {
    Display,
    Json,
}

impl OutputFormat {
    pub fn formatted_print<T>(&self, item: &T)
    where
        T: Serialize + fmt::Display,
    {
        match self {
            OutputFormat::Display => {
                println!("{}", item);
            }
            OutputFormat::Json => {
                println!("{}", serde_json::to_value(item).unwrap());
            }
        }
    }
}
