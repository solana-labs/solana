use {
    lazy_static::lazy_static,
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction},
    std::{collections::HashMap, str::FromStr},
};

#[macro_export]
macro_rules! to_pubkey {
    ($id_string:expr) => {{
        Pubkey::from_str($id_string).unwrap()
    }};
}

lazy_static! {
    pub static ref COMPUTE_BUDGET_WHITELIST: HashMap<Pubkey, u64> = [
        (to_pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"), 1_300_000),
        (to_pubkey!("2WG3bYHtVuuVQFKvRSnbYYf6TzxcjU2A5tzgridDTnFd"), 1_400_000)
    ]
    .iter()
    .cloned()
    .collect();
}

pub fn is_whitelisted(tx: &SanitizedTransaction) -> Option<u64> {
    tx.message()
        .program_ids()
        .iter()
        .find_map(|program_id| COMPUTE_BUDGET_WHITELIST.get(program_id).copied())
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::{
        hash::Hash, instruction::CompiledInstruction, signature::Keypair, transaction::Transaction,
    };

    #[test]
    fn test_compute_budget_is_whitelisted() {
        let signer = Keypair::new();
        let program_id = to_pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");

        let instructions = vec![CompiledInstruction::new(1, &(), vec![])];
        let tx = SanitizedTransaction::from_transaction_for_tests(
            Transaction::new_with_compiled_instructions(
                &[&signer],
                &[],
                Hash::default(),
                vec![program_id, Pubkey::new_unique(), Pubkey::new_unique()],
                instructions,
            ),
        );
        assert_eq!(is_whitelisted(&tx), Some(1_300_000));

        let instructions = vec![CompiledInstruction::new(2, &(), vec![])];
        let tx = SanitizedTransaction::from_transaction_for_tests(
            Transaction::new_with_compiled_instructions(
                &[&signer],
                &[],
                Hash::default(),
                vec![Pubkey::new_unique(), program_id, Pubkey::new_unique()],
                instructions,
            ),
        );
        assert_eq!(is_whitelisted(&tx), Some(1_300_000));

        let instructions = vec![CompiledInstruction::new(1, &(), vec![])];
        let tx = SanitizedTransaction::from_transaction_for_tests(
            Transaction::new_with_compiled_instructions(
                &[&signer],
                &[],
                Hash::default(),
                vec![Pubkey::new_unique(), program_id, Pubkey::new_unique()],
                instructions,
            ),
        );
        assert_eq!(is_whitelisted(&tx), None);
    }

    #[test]
    fn test_compute_budget_whitelist() {
        let signer = Keypair::new();
        let instructions = vec![CompiledInstruction::new(1, &(), vec![])];

        let ids = &[
            ("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", 1_300_000),
            ("2WG3bYHtVuuVQFKvRSnbYYf6TzxcjU2A5tzgridDTnFd", 1_400_000),
        ];
        for (id, cap) in ids.iter() {
            let program_id = to_pubkey!(id);
            let tx = SanitizedTransaction::from_transaction_for_tests(
                Transaction::new_with_compiled_instructions(
                    &[&signer],
                    &[],
                    Hash::default(),
                    vec![program_id],
                    instructions.clone(),
                ),
            );
            assert_eq!(is_whitelisted(&tx), Some(*cap));
        }
    }
}
