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

// let from_id = from_keypair.pubkey();
//         let voter_id = voter_keypair.pubkey();
//         let space = VoteState::max_size() as u64;
//         let create_ix =
//             SystemInstruction::new_program_account(&from_id, &voter_id, lamports, space, &id());
//         let init_ix = VoteInstruction::new_initialize_account(&voter_id);
//         let delegate_ix = VoteInstruction::new_delegate_stake(&voter_id, &delegate_id);
//         let mut tx = Transaction::new(vec![create_ix, init_ix, delegate_ix]);
//         tx.fee = fee;
//         tx.sign(&[from_keypair, voter_keypair], recent_blockhash);
//         tx

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
        let mut tx = Transaction::new(vec![create_ix, request_ix]);
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
        let mut tx = Transaction::new(vec![request_ix]);
        tx.fee = fee;
        tx.sign(&[owner], recent_blockhash);
        tx
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_trade_request(
        owner: &Keypair,
        trade: &Pubkey,
        direction: Direction,
        primary_token: Token,
        secondary_token: Token,
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
            primary_token,
            secondary_token,
            tokens,
            price,
            src_account,
            dst_account,
        );
        let mut tx = Transaction::new(vec![create_ix, request_ix]);
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
        let mut tx = Transaction::new(vec![request_ix]);
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
        let mut tx = Transaction::new(vec![create_ix, request_ix]);
        tx.fee = fee;
        tx.sign(&[owner], recent_blockhash);
        tx
    }
}
