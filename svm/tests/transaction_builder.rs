use {
    solana_sdk::{
        hash::Hash,
        instruction::{AccountMeta, CompiledInstruction},
        message::{
            v0::{self, LoadedAddresses, MessageAddressTableLookup},
            AddressLoader, AddressLoaderError, Message, MessageHeader, VersionedMessage,
        },
        pubkey::Pubkey,
        reserved_account_keys::ReservedAccountKeys,
        signature::Signature,
        transaction::{SanitizedTransaction, SanitizedVersionedTransaction, VersionedTransaction},
    },
    std::collections::HashMap,
};

#[derive(Default)]
pub struct SanitizedTransactionBuilder {
    instructions: Vec<InnerInstruction>,
    num_required_signatures: u8,
    num_readonly_signed_accounts: u8,
    num_readonly_unsigned_accounts: u8,
    signed_readonly_accounts: Vec<(Pubkey, Signature)>,
    signed_mutable_accounts: Vec<(Pubkey, Signature)>,
    unsigned_readonly_accounts: Vec<Pubkey>,
    unsigned_mutable_account: Vec<Pubkey>,
}

struct InnerInstruction {
    program_id: Pubkey,
    accounts: Vec<Pubkey>,
    data: Vec<u8>,
}

#[derive(Clone)]
struct MockLoader {}

// This implementation is only necessary if one is using account table lookups.
impl AddressLoader for MockLoader {
    fn load_addresses(
        self,
        _lookups: &[MessageAddressTableLookup],
    ) -> Result<LoadedAddresses, AddressLoaderError> {
        Ok(LoadedAddresses {
            writable: vec![],
            readonly: vec![],
        })
    }
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

        let mut instruction = InnerInstruction {
            program_id,
            accounts: Vec::new(),
            data,
        };

        for item in &accounts {
            match (item.is_signer, item.is_writable) {
                (true, true) => {
                    self.signed_mutable_accounts
                        .push((item.pubkey, signatures[&item.pubkey]));
                }
                (true, false) => {
                    self.num_readonly_signed_accounts =
                        self.num_readonly_signed_accounts.saturating_add(1);
                    self.signed_readonly_accounts
                        .push((item.pubkey, signatures[&item.pubkey]));
                }
                (false, true) => {
                    self.unsigned_mutable_account.push(item.pubkey);
                }
                (false, false) => {
                    self.num_readonly_unsigned_accounts =
                        self.num_readonly_unsigned_accounts.saturating_add(1);
                    self.unsigned_readonly_accounts.push(item.pubkey);
                }
            }
            instruction.accounts.push(item.pubkey);
        }

        self.instructions.push(instruction);
    }

    pub fn build(
        &mut self,
        block_hash: Hash,
        fee_payer: (Pubkey, Signature),
        v0_message: bool,
    ) -> SanitizedTransaction {
        let mut account_keys = Vec::new();
        let header = MessageHeader {
            // The fee payer always requires a signature so +1
            num_required_signatures: self.num_required_signatures.saturating_add(1),
            num_readonly_signed_accounts: self.num_readonly_signed_accounts,
            // The program id is always a readonly unsigned account
            num_readonly_unsigned_accounts: self.num_readonly_unsigned_accounts.saturating_add(1),
        };

        let mut compiled_instructions = Vec::new();

        let mut signatures = Vec::with_capacity(
            self.signed_mutable_accounts
                .len()
                .saturating_add(self.signed_readonly_accounts.len())
                .saturating_add(self.unsigned_mutable_account.len())
                .saturating_add(self.unsigned_readonly_accounts.len())
                .saturating_add(1),
        );
        let mut positions: HashMap<Pubkey, usize> = HashMap::new();

        account_keys.push(fee_payer.0);
        signatures.push(fee_payer.1);

        let mut positions_lambda = |key: &Pubkey| {
            positions.insert(*key, account_keys.len());
            account_keys.push(*key);
        };

        self.signed_mutable_accounts
            .iter()
            .for_each(|(key, signature)| {
                positions_lambda(key);
                signatures.push(*signature);
            });
        self.signed_readonly_accounts
            .iter()
            .for_each(|(key, signature)| {
                positions_lambda(key);
                signatures.push(*signature);
            });
        self.unsigned_mutable_account
            .iter()
            .for_each(&mut positions_lambda);
        self.unsigned_readonly_accounts
            .iter()
            .for_each(&mut positions_lambda);

        let instructions = self.clean_up();

        for item in instructions {
            let accounts = item
                .accounts
                .iter()
                .map(|key| positions[key] as u8)
                .collect::<Vec<u8>>();
            let instruction = CompiledInstruction {
                program_id_index: push_and_return_index(item.program_id, &mut account_keys),
                accounts,
                data: item.data,
            };

            compiled_instructions.push(instruction);
        }

        let message = if v0_message {
            let message = v0::Message {
                header,
                account_keys,
                recent_blockhash: block_hash,
                instructions: compiled_instructions,
                address_table_lookups: vec![],
            };

            VersionedMessage::V0(message)
        } else {
            let message = Message {
                header,
                account_keys,
                recent_blockhash: block_hash,
                instructions: compiled_instructions,
            };

            VersionedMessage::Legacy(message)
        };

        let transaction = VersionedTransaction {
            signatures,
            message,
        };

        let sanitized_versioned_transaction =
            SanitizedVersionedTransaction::try_new(transaction).unwrap();

        let loader = MockLoader {};
        let sanitized_transaction = SanitizedTransaction::try_new(
            sanitized_versioned_transaction,
            Hash::new_unique(),
            false,
            loader,
            &ReservedAccountKeys::new_all_activated().active,
        );

        sanitized_transaction.unwrap()
    }

    fn clean_up(&mut self) -> Vec<InnerInstruction> {
        let mut instructions = Vec::new();

        std::mem::swap(&mut instructions, &mut self.instructions);
        self.num_required_signatures = 0;
        self.num_readonly_signed_accounts = 0;
        self.num_readonly_unsigned_accounts = 0;
        self.signed_mutable_accounts.clear();
        self.signed_readonly_accounts.clear();
        self.unsigned_mutable_account.clear();
        self.unsigned_mutable_account.clear();

        instructions
    }
}

fn push_and_return_index(value: Pubkey, vector: &mut Vec<Pubkey>) -> u8 {
    vector.push(value);
    vector.len().saturating_sub(1) as u8
}
