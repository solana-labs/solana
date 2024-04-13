use {
    solana_sdk::{
        hash::Hash,
        instruction::{AccountMeta, CompiledInstruction},
        message::{Message, MessageHeader},
        pubkey::Pubkey,
        reserved_account_keys::ReservedAccountKeys,
        signature::Signature,
        transaction::{SanitizedTransaction, Transaction},
    },
    std::collections::HashMap,
};

#[derive(Default)]
pub struct SanitizedTransactionBuilder {
    instructions: Vec<InnerInstruction>,
    num_required_signatures: u8,
    num_readonly_signed_accounts: u8,
    num_readonly_unsigned_accounts: u8,
}

struct InnerInstruction {
    program_id: Pubkey,
    accounts: Vec<Pubkey>,
    signatures: HashMap<Pubkey, Signature>,
    data: Vec<u8>,
}

impl SanitizedTransactionBuilder {
    pub fn create_instruction(
        &mut self,
        program_id: Pubkey,
        // The fee payer and the program id shall not appear in the accounts vector
        accounts: Vec<AccountMeta>,
        signatures: HashMap<Pubkey, Signature>,
        data: Vec<u8>,
    ) {
        self.num_required_signatures = self
            .num_required_signatures
            .saturating_add(signatures.len() as u8);

        let instruction = InnerInstruction {
            program_id,
            accounts: accounts
                .iter()
                .map(|meta| {
                    if !meta.is_writable {
                        if meta.is_signer {
                            self.num_readonly_signed_accounts =
                                self.num_readonly_signed_accounts.saturating_add(1);
                        } else {
                            self.num_readonly_unsigned_accounts =
                                self.num_readonly_unsigned_accounts.saturating_add(1);
                        }
                    }
                    meta.pubkey
                })
                .collect(),
            signatures,
            data,
        };
        self.instructions.push(instruction);
    }

    pub fn build(
        &mut self,
        block_hash: Hash,
        fee_payer: (Pubkey, Signature),
    ) -> SanitizedTransaction {
        let mut message = Message {
            account_keys: vec![],
            header: MessageHeader {
                // The fee payer always requires a signature so +1
                num_required_signatures: self.num_required_signatures.saturating_add(1),
                num_readonly_signed_accounts: self.num_readonly_signed_accounts,
                num_readonly_unsigned_accounts: self.num_readonly_unsigned_accounts,
            },
            instructions: vec![],
            recent_blockhash: block_hash,
        };

        let mut signatures = Vec::new();
        let mut positions: HashMap<Pubkey, usize> = HashMap::new();

        message.account_keys.push(fee_payer.0);
        signatures.push(fee_payer.1);

        for item in &self.instructions {
            for (key, value) in &item.signatures {
                signatures.push(*value);
                positions.insert(*key, message.account_keys.len());
                message.account_keys.push(*key);
            }
        }

        let mut instructions: Vec<InnerInstruction> = Vec::new();

        // Clean up
        std::mem::swap(&mut instructions, &mut self.instructions);
        self.num_required_signatures = 0;
        self.num_readonly_signed_accounts = 0;
        self.num_readonly_unsigned_accounts = 0;

        for item in instructions {
            let accounts = item
                .accounts
                .iter()
                .map(|key| {
                    if let Some(idx) = positions.get(key) {
                        *idx as u8
                    } else {
                        push_and_return_index(*key, &mut message.account_keys)
                    }
                })
                .collect::<Vec<u8>>();
            let instruction = CompiledInstruction {
                program_id_index: push_and_return_index(item.program_id, &mut message.account_keys),
                accounts,
                data: item.data,
            };

            message.instructions.push(instruction);
        }

        let transaction = Transaction {
            signatures,
            message,
        };

        SanitizedTransaction::try_from_legacy_transaction(
            transaction,
            &ReservedAccountKeys::new_all_activated().active,
        )
        .unwrap()
    }
}

fn push_and_return_index(value: Pubkey, vector: &mut Vec<Pubkey>) -> u8 {
    vector.push(value);
    vector.len().saturating_sub(1) as u8
}
