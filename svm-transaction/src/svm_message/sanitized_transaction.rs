use {
    crate::{
        instruction::SVMInstruction, message_address_table_lookup::SVMMessageAddressTableLookup,
        svm_message::SVMMessage,
    },
    solana_sdk::{
        hash::Hash, message::AccountKeys, pubkey::Pubkey, transaction::SanitizedTransaction,
    },
};

impl SVMMessage for SanitizedTransaction {
    fn num_total_signatures(&self) -> u64 {
        SVMMessage::num_total_signatures(SanitizedTransaction::message(self))
    }

    fn num_write_locks(&self) -> u64 {
        SVMMessage::num_write_locks(SanitizedTransaction::message(self))
    }

    fn recent_blockhash(&self) -> &Hash {
        SVMMessage::recent_blockhash(SanitizedTransaction::message(self))
    }

    fn num_instructions(&self) -> usize {
        SVMMessage::num_instructions(SanitizedTransaction::message(self))
    }

    fn instructions_iter(&self) -> impl Iterator<Item = SVMInstruction> {
        SVMMessage::instructions_iter(SanitizedTransaction::message(self))
    }

    fn program_instructions_iter(&self) -> impl Iterator<Item = (&Pubkey, SVMInstruction)> {
        SVMMessage::program_instructions_iter(SanitizedTransaction::message(self))
    }

    fn account_keys(&self) -> AccountKeys {
        SVMMessage::account_keys(SanitizedTransaction::message(self))
    }

    fn fee_payer(&self) -> &Pubkey {
        SVMMessage::fee_payer(SanitizedTransaction::message(self))
    }

    fn is_writable(&self, index: usize) -> bool {
        SVMMessage::is_writable(SanitizedTransaction::message(self), index)
    }

    fn is_signer(&self, index: usize) -> bool {
        SVMMessage::is_signer(SanitizedTransaction::message(self), index)
    }

    fn is_invoked(&self, key_index: usize) -> bool {
        SVMMessage::is_invoked(SanitizedTransaction::message(self), key_index)
    }

    fn num_lookup_tables(&self) -> usize {
        SVMMessage::num_lookup_tables(SanitizedTransaction::message(self))
    }

    fn message_address_table_lookups(&self) -> impl Iterator<Item = SVMMessageAddressTableLookup> {
        SVMMessage::message_address_table_lookups(SanitizedTransaction::message(self))
    }
}
