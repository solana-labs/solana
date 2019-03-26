use crate::exchange_instruction::*;
use crate::exchange_state::*;
use crate::id;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::Transaction;
use std::mem;

pub struct ExchangeTransaction {}

impl ExchangeTransaction {
    pub fn new_account_request(
        owner: &Keypair,
        new: &Pubkey,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let owner_id = &owner.pubkey();
        let space = mem::size_of::<ExchangeState>() as u64;
        let create_ix = SystemInstruction::new_program_account(owner_id, new, 1, space, &id());
        let request_ix = ExchangeInstruction::new_account_request(owner_id, new);
        let mut tx = Transaction::new_unsigned_instructions(vec![create_ix, request_ix]);
        tx.fee = fee;
        tx.sign(&[owner], recent_blockhash);
        tx
    }

    pub fn new_transfer_request(
        owner: &Keypair,
        to: &Pubkey,
        from: &Pubkey,
        token: Token,
        tokens: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let owner_id = &owner.pubkey();
        let request_ix =
            ExchangeInstruction::new_transfer_request(owner_id, to, from, token, tokens);
        let mut tx = Transaction::new_unsigned_instructions(vec![request_ix]);
        tx.fee = fee;
        tx.sign(&[owner], recent_blockhash);
        tx
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_trade_request(
        owner: &Keypair,
        trade: &Pubkey,
        direction: Direction,
        pair: TokenPair,
        tokens: u64,
        price: u64,
        src_account: &Pubkey,
        dst_account: &Pubkey,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let owner_id = &owner.pubkey();
        let space = mem::size_of::<ExchangeState>() as u64;
        let create_ix = SystemInstruction::new_program_account(owner_id, trade, 1, space, &id());
        let request_ix = ExchangeInstruction::new_trade_request(
            owner_id,
            trade,
            direction,
            pair,
            tokens,
            price,
            src_account,
            dst_account,
        );
        let mut tx = Transaction::new_unsigned_instructions(vec![create_ix, request_ix]);
        tx.fee = fee;
        tx.sign(&[owner], recent_blockhash);
        tx
    }

    pub fn new_trade_cancellation(
        owner: &Keypair,
        trade: &Pubkey,
        account: &Pubkey,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let owner_id = &owner.pubkey();
        let request_ix = ExchangeInstruction::new_trade_cancellation(owner_id, trade, account);
        let mut tx = Transaction::new_unsigned_instructions(vec![request_ix]);
        tx.fee = fee;
        tx.sign(&[owner], recent_blockhash);
        tx
    }

    pub fn new_swap_request(
        owner: &Keypair,
        swap: &Pubkey,
        to_trade: &Pubkey,
        from_trade: &Pubkey,
        to_trade_account: &Pubkey,
        from_trade_account: &Pubkey,
        profit_account: &Pubkey,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let owner_id = &owner.pubkey();
        let space = mem::size_of::<ExchangeState>() as u64;
        let create_ix = SystemInstruction::new_program_account(owner_id, swap, 1, space, &id());
        let request_ix = ExchangeInstruction::new_swap_request(
            owner_id,
            swap,
            to_trade,
            from_trade,
            to_trade_account,
            from_trade_account,
            profit_account,
        );
        let mut tx = Transaction::new_unsigned_instructions(vec![create_ix, request_ix]);
        tx.fee = fee;
        tx.sign(&[owner], recent_blockhash);
        tx
    }
}
