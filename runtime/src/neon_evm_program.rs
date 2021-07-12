solana_sdk::declare_id!("eeLSJgWzzxrqKv1UxtRVVH8FX3qCQWUs9QuAjJpETGU");

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_id() {
        println!("{:?}", id());
    }
}
