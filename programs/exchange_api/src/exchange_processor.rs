//! Config processor

use crate::exchange_instruction::*;
use crate::exchange_state::*;
use crate::id;
use log::*;
use solana_metrics::counter::Counter;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use std::cmp;

pub struct ExchangeProcessor {}

impl ExchangeProcessor {
    #[allow(clippy::needless_pass_by_value)]
    fn map_to_invalid_arg(err: std::boxed::Box<bincode::ErrorKind>) -> InstructionError {
        warn!("Deserialize failed, not a valid state: {:?}", err);
        InstructionError::InvalidArgument
    }

    fn is_account_unallocated(data: &[u8]) -> Result<(), InstructionError> {
        let state: ExchangeState = bincode::deserialize(data).map_err(Self::map_to_invalid_arg)?;
        if let ExchangeState::Unallocated = state {
            Ok(())
        } else {
            error!("New account is already in use");
            Err(InstructionError::InvalidAccountData)?
        }
    }

    fn deserialize_account(data: &[u8]) -> Result<TokenAccountInfo, InstructionError> {
        let state: ExchangeState = bincode::deserialize(data).map_err(Self::map_to_invalid_arg)?;
        if let ExchangeState::Account(account) = state {
            Ok(account)
        } else {
            error!("Not a valid account");
            Err(InstructionError::InvalidAccountData)?
        }
    }

    fn deserialize_trade(data: &[u8]) -> Result<TradeOrderInfo, InstructionError> {
        let state: ExchangeState = bincode::deserialize(data).map_err(Self::map_to_invalid_arg)?;
        if let ExchangeState::Trade(info) = state {
            Ok(info)
        } else {
            error!("Not a valid trade");
            Err(InstructionError::InvalidAccountData)?
        }
    }

    fn serialize(state: &ExchangeState, data: &mut [u8]) -> Result<(), InstructionError> {
        let writer = std::io::BufWriter::new(data);
        match bincode::serialize_into(writer, state) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Serialize failed: {:?}", e);
                Err(InstructionError::GenericError)?
            }
        }
    }

    fn trade_to_token_account(trade: &TradeOrderInfo) -> TokenAccountInfo {
        // Turn trade order into token account

        let token = match trade.direction {
            Direction::To => trade.pair.secondary(),
            Direction::From => trade.pair.primary(),
        };

        let mut account = TokenAccountInfo::default().owner(&trade.owner);
        account.tokens[token] = trade.tokens_settled;
        account
    }

    fn calculate_swap(
        scaler: u64,
        to_trade: &mut TradeOrderInfo,
        from_trade: &mut TradeOrderInfo,
        profit_account: &mut TokenAccountInfo,
    ) -> Result<(), InstructionError> {
        if to_trade.tokens == 0 || from_trade.tokens == 0 {
            error!("Inactive Trade, balance is zero");
            Err(InstructionError::InvalidArgument)?
        }
        if to_trade.price == 0 || from_trade.price == 0 {
            error!("Inactive Trade, price is zero");
            Err(InstructionError::InvalidArgument)?
        }

        // Calc swap

        trace!("tt {} ft {}", to_trade.tokens, from_trade.tokens);
        trace!("tp {} fp {}", to_trade.price, from_trade.price);

        let max_to_secondary = to_trade.tokens * to_trade.price / scaler;
        let max_to_primary = from_trade.tokens * scaler / from_trade.price;

        trace!("mtp {} mts {}", max_to_primary, max_to_secondary);

        let max_primary = cmp::min(max_to_primary, to_trade.tokens);
        let max_secondary = cmp::min(max_to_secondary, from_trade.tokens);

        trace!("mp {} ms {}", max_primary, max_secondary);

        let primary_tokens = if max_secondary < max_primary {
            max_secondary * scaler / from_trade.price
        } else {
            max_primary
        };
        let secondary_tokens = if max_secondary < max_primary {
            max_secondary
        } else {
            max_primary * to_trade.price / scaler
        };

        if primary_tokens == 0 || secondary_tokens == 0 {
            error!("Trade quantities to low to be fulfilled");
            Err(InstructionError::InvalidArgument)?
        }

        trace!("pt {} st {}", primary_tokens, secondary_tokens);

        let primary_cost = cmp::max(primary_tokens, secondary_tokens * scaler / to_trade.price);
        let secondary_cost = cmp::max(secondary_tokens, primary_tokens * from_trade.price / scaler);

        trace!("pc {} sc {}", primary_cost, secondary_cost);

        let primary_profit = primary_cost - primary_tokens;
        let secondary_profit = secondary_cost - secondary_tokens;

        trace!("pp {} sp {}", primary_profit, secondary_profit);

        let primary_token = to_trade.pair.primary();
        let secondary_token = from_trade.pair.secondary();

        // Update tokens

        if to_trade.tokens < primary_cost {
            error!("Not enough tokens in to account");
            Err(InstructionError::InvalidArgument)?
        }
        if from_trade.tokens < secondary_cost {
            error!("Not enough tokens in from account");
            Err(InstructionError::InvalidArgument)?
        }
        to_trade.tokens -= primary_cost;
        to_trade.tokens_settled += secondary_tokens;
        from_trade.tokens -= secondary_cost;
        from_trade.tokens_settled += primary_tokens;

        profit_account.tokens[primary_token] += primary_profit;
        profit_account.tokens[secondary_token] += secondary_profit;

        Ok(())
    }

    fn do_account_request(keyed_accounts: &mut [KeyedAccount]) -> Result<(), InstructionError> {
        const OWNER_INDEX: usize = 0;
        const NEW_ACCOUNT_INDEX: usize = 1;

        if keyed_accounts.len() < 2 {
            error!("Not enough accounts");
            Err(InstructionError::InvalidArgument)?
        }

        Self::is_account_unallocated(&keyed_accounts[NEW_ACCOUNT_INDEX].account.data)?;
        Self::serialize(
            &ExchangeState::Account(
                TokenAccountInfo::default()
                    .owner(&keyed_accounts[OWNER_INDEX].unsigned_key())
                    .tokens(100_000, 100_000, 100_000, 100_000),
            ),
            &mut keyed_accounts[NEW_ACCOUNT_INDEX].account.data,
        )
    }

    fn do_transfer_request(
        keyed_accounts: &mut [KeyedAccount],
        token: Token,
        tokens: u64,
    ) -> Result<(), InstructionError> {
        const OWNER_INDEX: usize = 0;
        const TO_ACCOUNT_INDEX: usize = 1;
        const FROM_ACCOUNT_INDEX: usize = 2;

        if keyed_accounts.len() < 3 {
            error!("Not enough accounts");
            Err(InstructionError::InvalidArgument)?
        }

        let mut to_account =
            Self::deserialize_account(&keyed_accounts[TO_ACCOUNT_INDEX].account.data)?;

        if &id() == keyed_accounts[FROM_ACCOUNT_INDEX].unsigned_key() {
            to_account.tokens[token] += tokens;
        } else {
            let state: ExchangeState =
                bincode::deserialize(&keyed_accounts[FROM_ACCOUNT_INDEX].account.data)
                    .map_err(Self::map_to_invalid_arg)?;
            match state {
                ExchangeState::Account(mut from_account) => {
                    if &from_account.owner != keyed_accounts[OWNER_INDEX].unsigned_key() {
                        error!("Signer does not own from account");
                        Err(InstructionError::GenericError)?
                    }

                    if from_account.tokens[token] < tokens {
                        error!("From account balance too low");
                        Err(InstructionError::GenericError)?
                    }

                    from_account.tokens[token] -= tokens;
                    to_account.tokens[token] += tokens;

                    Self::serialize(
                        &ExchangeState::Account(from_account),
                        &mut keyed_accounts[FROM_ACCOUNT_INDEX].account.data,
                    )?;
                }
                ExchangeState::Trade(mut from_trade) => {
                    if &from_trade.owner != keyed_accounts[OWNER_INDEX].unsigned_key() {
                        error!("Signer does not own from account");
                        Err(InstructionError::GenericError)?
                    }

                    let from_token = match from_trade.direction {
                        Direction::To => from_trade.pair.secondary(),
                        Direction::From => from_trade.pair.primary(),
                    };
                    if token != from_token {
                        error!("Trade to transfer from does not hold correct token");
                        Err(InstructionError::GenericError)?
                    }

                    if from_trade.tokens_settled < tokens {
                        error!("From trade balance too low");
                        Err(InstructionError::GenericError)?
                    }

                    from_trade.tokens_settled -= tokens;
                    to_account.tokens[token] += tokens;

                    Self::serialize(
                        &ExchangeState::Trade(from_trade),
                        &mut keyed_accounts[FROM_ACCOUNT_INDEX].account.data,
                    )?;
                }
                _ => {
                    error!("Not a valid from account for transfer");
                    Err(InstructionError::InvalidArgument)?
                }
            }
        }

        Self::serialize(
            &ExchangeState::Account(to_account),
            &mut keyed_accounts[TO_ACCOUNT_INDEX].account.data,
        )
    }

    fn do_trade_request(
        keyed_accounts: &mut [KeyedAccount],
        info: &TradeRequestInfo,
    ) -> Result<(), InstructionError> {
        const OWNER_INDEX: usize = 0;
        const TRADE_INDEX: usize = 1;
        const ACCOUNT_INDEX: usize = 2;

        if keyed_accounts.len() < 3 {
            error!("Not enough accounts");
            Err(InstructionError::InvalidArgument)?
        }

        Self::is_account_unallocated(&keyed_accounts[TRADE_INDEX].account.data)?;

        let mut account = Self::deserialize_account(&keyed_accounts[ACCOUNT_INDEX].account.data)?;

        if &account.owner != keyed_accounts[OWNER_INDEX].unsigned_key() {
            error!("Signer does not own account");
            Err(InstructionError::GenericError)?
        }
        let from_token = match info.direction {
            Direction::To => info.pair.primary(),
            Direction::From => info.pair.secondary(),
        };
        if account.tokens[from_token] < info.tokens {
            error!("From token balance is too low");
            Err(InstructionError::GenericError)?
        }

        if let Err(e) = check_trade(info.direction, info.tokens, info.price) {
            bincode::serialize(&e).unwrap();
        }

        // Trade holds the tokens in escrow
        account.tokens[from_token] -= info.tokens;

        inc_new_counter_info!("exchange_processor-trades", 1);

        Self::serialize(
            &ExchangeState::Trade(TradeOrderInfo {
                owner: *keyed_accounts[OWNER_INDEX].unsigned_key(),
                direction: info.direction,
                pair: info.pair,
                tokens: info.tokens,
                price: info.price,
                tokens_settled: 0,
            }),
            &mut keyed_accounts[TRADE_INDEX].account.data,
        )?;
        Self::serialize(
            &ExchangeState::Account(account),
            &mut keyed_accounts[ACCOUNT_INDEX].account.data,
        )
    }

    fn do_trade_cancellation(keyed_accounts: &mut [KeyedAccount]) -> Result<(), InstructionError> {
        const OWNER_INDEX: usize = 0;
        const TRADE_INDEX: usize = 1;

        if keyed_accounts.len() < 2 {
            error!("Not enough accounts");
            Err(InstructionError::InvalidArgument)?
        }

        let trade = Self::deserialize_trade(&keyed_accounts[TRADE_INDEX].account.data)?;

        if &trade.owner != keyed_accounts[OWNER_INDEX].unsigned_key() {
            error!("Signer does not own trade");
            Err(InstructionError::GenericError)?
        }

        let token = match trade.direction {
            Direction::To => trade.pair.primary(),
            Direction::From => trade.pair.secondary(),
        };

        let mut account = TokenAccountInfo::default().owner(&trade.owner);
        account.tokens[token] = trade.tokens;
        account.tokens[token] += trade.tokens_settled;

        // Turn trade order into a token account
        Self::serialize(
            &ExchangeState::Account(account),
            &mut keyed_accounts[TRADE_INDEX].account.data,
        )
    }

    fn do_swap_request(keyed_accounts: &mut [KeyedAccount]) -> Result<(), InstructionError> {
        const TO_TRADE_INDEX: usize = 1;
        const FROM_TRADE_INDEX: usize = 2;
        const PROFIT_ACCOUNT_INDEX: usize = 3;

        if keyed_accounts.len() < 4 {
            error!("Not enough accounts");
            Err(InstructionError::InvalidArgument)?
        }

        let mut to_trade = Self::deserialize_trade(&keyed_accounts[TO_TRADE_INDEX].account.data)?;
        let mut from_trade =
            Self::deserialize_trade(&keyed_accounts[FROM_TRADE_INDEX].account.data)?;
        let mut profit_account =
            Self::deserialize_account(&keyed_accounts[PROFIT_ACCOUNT_INDEX].account.data)?;

        if to_trade.direction != Direction::To {
            error!("To trade is not a To");
            Err(InstructionError::InvalidArgument)?
        }
        if from_trade.direction != Direction::From {
            error!("From trade is not a From");
            Err(InstructionError::InvalidArgument)?
        }
        if to_trade.pair != from_trade.pair {
            error!("Mismatched token pairs");
            Err(InstructionError::InvalidArgument)?
        }
        if to_trade.direction == from_trade.direction {
            error!("Matching trade directions");
            Err(InstructionError::InvalidArgument)?
        }

        if let Err(e) =
            Self::calculate_swap(SCALER, &mut to_trade, &mut from_trade, &mut profit_account)
        {
            error!(
                "Swap calculation failed from {} for {} to {} for {}",
                from_trade.tokens, from_trade.price, to_trade.tokens, to_trade.price,
            );
            Err(e)?
        }

        inc_new_counter_info!("exchange_processor-swap", 1);

        if to_trade.tokens == 0 {
            // Turn into token account
            Self::serialize(
                &ExchangeState::Account(Self::trade_to_token_account(&from_trade)),
                &mut keyed_accounts[TO_TRADE_INDEX].account.data,
            )?;
        } else {
            Self::serialize(
                &ExchangeState::Trade(to_trade),
                &mut keyed_accounts[TO_TRADE_INDEX].account.data,
            )?;
        }

        if from_trade.tokens == 0 {
            // Turn into token account
            Self::serialize(
                &ExchangeState::Account(Self::trade_to_token_account(&from_trade)),
                &mut keyed_accounts[FROM_TRADE_INDEX].account.data,
            )?;
        } else {
            Self::serialize(
                &ExchangeState::Trade(from_trade),
                &mut keyed_accounts[FROM_TRADE_INDEX].account.data,
            )?;
        }

        Self::serialize(
            &ExchangeState::Account(profit_account),
            &mut keyed_accounts[PROFIT_ACCOUNT_INDEX].account.data,
        )
    }
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    let command = bincode::deserialize::<ExchangeInstruction>(data).map_err(|err| {
        info!("Invalid transaction data: {:?} {:?}", data, err);
        InstructionError::InvalidInstructionData
    })?;

    trace!("{:?}", command);

    match command {
        ExchangeInstruction::AccountRequest => {
            ExchangeProcessor::do_account_request(keyed_accounts)
        }
        ExchangeInstruction::TransferRequest(token, tokens) => {
            ExchangeProcessor::do_transfer_request(keyed_accounts, token, tokens)
        }
        ExchangeInstruction::TradeRequest(info) => {
            ExchangeProcessor::do_trade_request(keyed_accounts, &info)
        }
        ExchangeInstruction::TradeCancellation => {
            ExchangeProcessor::do_trade_cancellation(keyed_accounts)
        }
        ExchangeInstruction::SwapRequest => ExchangeProcessor::do_swap_request(keyed_accounts),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::exchange_instruction;
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction;
    use std::mem;

    fn try_calc(
        scaler: u64,
        primary_tokens: u64,
        primary_price: u64,
        secondary_tokens: u64,
        secondary_price: u64,
        primary_tokens_expect: u64,
        secondary_tokens_expect: u64,
        primary_tokens_settled_expect: u64,
        secondary_tokens_settled_expect: u64,
        profit_account_tokens: Tokens,
    ) -> Result<(), InstructionError> {
        trace!(
            "Swap {} for {} to {} for {}",
            primary_tokens,
            primary_price,
            secondary_tokens,
            secondary_price,
        );
        let mut to_trade = TradeOrderInfo::default();
        let mut from_trade = TradeOrderInfo::default().direction(Direction::From);
        let mut profit_account = TokenAccountInfo::default();

        to_trade.tokens = primary_tokens;
        to_trade.price = primary_price;
        from_trade.tokens = secondary_tokens;
        from_trade.price = secondary_price;
        ExchangeProcessor::calculate_swap(
            scaler,
            &mut to_trade,
            &mut from_trade,
            &mut profit_account,
        )?;

        trace!(
            "{:?} {:?} {:?} {:?}\n{:?}\n{:?}\n{:?}\n{:?}",
            to_trade.tokens,
            primary_tokens_expect,
            from_trade.tokens,
            secondary_tokens_expect,
            primary_tokens_settled_expect,
            secondary_tokens_settled_expect,
            profit_account.tokens,
            profit_account_tokens
        );

        assert_eq!(to_trade.tokens, primary_tokens_expect);
        assert_eq!(from_trade.tokens, secondary_tokens_expect);
        assert_eq!(to_trade.tokens_settled, primary_tokens_settled_expect);
        assert_eq!(from_trade.tokens_settled, secondary_tokens_settled_expect);
        assert_eq!(profit_account.tokens, profit_account_tokens);
        Ok(())
    }

    #[test]
    #[rustfmt::skip]
    fn test_calculate_swap() {
        solana_logger::setup();

        try_calc(1,     50,     2,   50,    1,  0, 0, 50,   50, Tokens::new(   0, 0, 0, 0)).unwrap_err();
        try_calc(1,     50,     1,    0,    1,  0, 0, 50,   50, Tokens::new(   0, 0, 0, 0)).unwrap_err();
        try_calc(1,      0,     1,   50,    1,  0, 0, 50,   50, Tokens::new(   0, 0, 0, 0)).unwrap_err();
        try_calc(1,     50,     1,   50,    0,  0, 0, 50,   50, Tokens::new(   0, 0, 0, 0)).unwrap_err();
        try_calc(1,     50,     0,   50,    1,  0, 0, 50,   50, Tokens::new(   0, 0, 0, 0)).unwrap_err();
        try_calc(1,       1,    2,    2,    3,  1, 2,  0,    0, Tokens::new(   0, 0, 0, 0)).unwrap_err();

        try_calc(1,     50,     1,   50,    1,  0, 0, 50,   50, Tokens::new(   0, 0, 0, 0)).unwrap();
        try_calc(1,       1,    2,    3,    3,  0, 0,  2,    1, Tokens::new(   0, 1, 0, 0)).unwrap();
        try_calc(1,       2,    2,    3,    3,  1, 0,  2,    1, Tokens::new(   0, 1, 0, 0)).unwrap();
        try_calc(1,       3,    2,    3,    3,  2, 0,  2,    1, Tokens::new(   0, 1, 0, 0)).unwrap();
        try_calc(1,       3,    2,    6,    3,  1, 0,  4,    2, Tokens::new(   0, 2, 0, 0)).unwrap();
        try_calc(1000,    1, 2000,    3, 3000,  0, 0,  2,    1, Tokens::new(   0, 1, 0, 0)).unwrap();
        try_calc(1,       3,    2,    7,    3,  1, 1,  4,    2, Tokens::new(   0, 2, 0, 0)).unwrap();
        try_calc(1000, 3000,  333, 1000,  500,  0, 1,999, 1998, Tokens::new(1002, 0, 0, 0)).unwrap();
        try_calc(1000,   50,  100,   50,  101,  0,45,  5,   49, Tokens::new(   1, 0, 0, 0)).unwrap();
    }

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_block, mint_keypair) = GenesisBlock::new(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        (bank, mint_keypair)
    }

    fn create_client(bank: Bank, mint_keypair: Keypair) -> (BankClient, Keypair) {
        let owner = Keypair::new();
        let bank_client = BankClient::new(bank);
        bank_client
            .transfer(42, &mint_keypair, &owner.pubkey())
            .unwrap();

        (bank_client, owner)
    }

    fn create_account(client: &BankClient, owner: &Keypair) -> Pubkey {
        let new = Pubkey::new_rand();
        let instruction = system_instruction::create_account(
            &owner.pubkey(),
            &new,
            1,
            mem::size_of::<ExchangeState>() as u64,
            &id(),
        );
        client
            .send_instruction(&owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));
        new
    }

    fn create_token_account(client: &BankClient, owner: &Keypair) -> Pubkey {
        let new = Pubkey::new_rand();
        let instruction = system_instruction::create_account(
            &owner.pubkey(),
            &new,
            1,
            mem::size_of::<ExchangeState>() as u64,
            &id(),
        );
        client
            .send_instruction(owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));
        let instruction = exchange_instruction::account_request(&owner.pubkey(), &new);
        client
            .send_instruction(owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));
        new
    }

    fn transfer(client: &BankClient, owner: &Keypair, to: &Pubkey, token: Token, tokens: u64) {
        let instruction =
            exchange_instruction::transfer_request(&owner.pubkey(), to, &id(), token, tokens);
        client
            .send_instruction(owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));
    }

    fn trade(
        client: &BankClient,
        owner: &Keypair,
        direction: Direction,
        pair: TokenPair,
        from_token: Token,
        src_tokens: u64,
        trade_tokens: u64,
        price: u64,
    ) -> (Pubkey, Pubkey) {
        let trade = create_account(&client, &owner);
        let src = create_token_account(&client, &owner);
        transfer(&client, &owner, &src, from_token, src_tokens);

        let instruction = exchange_instruction::trade_request(
            &owner.pubkey(),
            &trade,
            direction,
            pair,
            trade_tokens,
            price,
            &src,
        );
        client
            .send_instruction(owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));
        (trade, src)
    }

    #[test]
    fn test_exchange_new_account() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (client, owner) = create_client(bank, mint_keypair);

        let new = create_token_account(&client, &owner);
        let new_account_data = client.get_account_data(&new).unwrap().unwrap();

        // Check results

        assert_eq!(
            TokenAccountInfo::default()
                .owner(&owner.pubkey())
                .tokens(100_000, 100_000, 100_000, 100_000),
            ExchangeProcessor::deserialize_account(&new_account_data).unwrap()
        );
    }

    #[test]
    fn test_exchange_new_account_not_unallocated() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (client, owner) = create_client(bank, mint_keypair);

        let new = create_token_account(&client, &owner);
        let instruction = exchange_instruction::account_request(&owner.pubkey(), &new);
        client
            .send_instruction(&owner, instruction)
            .expect_err(&format!("{}:{}", line!(), file!()));
    }

    #[test]
    fn test_exchange_new_transfer_request() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (client, owner) = create_client(bank, mint_keypair);

        let new = create_token_account(&client, &owner);

        let instruction =
            exchange_instruction::transfer_request(&owner.pubkey(), &new, &id(), Token::A, 42);
        client
            .send_instruction(&owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));

        let new_account_data = client.get_account_data(&new).unwrap().unwrap();

        // Check results

        assert_eq!(
            TokenAccountInfo::default()
                .owner(&owner.pubkey())
                .tokens(100_042, 100_000, 100_000, 100_000),
            ExchangeProcessor::deserialize_account(&new_account_data).unwrap()
        );
    }

    #[test]
    fn test_exchange_new_trade_request() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (client, owner) = create_client(bank, mint_keypair);

        let (trade, src) = trade(
            &client,
            &owner,
            Direction::To,
            TokenPair::AB,
            Token::A,
            42,
            2,
            1000,
        );

        let trade_account_data = client.get_account_data(&trade).unwrap().unwrap();
        let src_account_data = client.get_account_data(&src).unwrap().unwrap();

        // check results

        assert_eq!(
            TradeOrderInfo {
                owner: owner.pubkey(),
                direction: Direction::To,
                pair: TokenPair::AB,
                tokens: 2,
                price: 1000,
                tokens_settled: 0
            },
            ExchangeProcessor::deserialize_trade(&trade_account_data).unwrap()
        );
        assert_eq!(
            TokenAccountInfo::default()
                .owner(&owner.pubkey())
                .tokens(100_040, 100_000, 100_000, 100_000),
            ExchangeProcessor::deserialize_account(&src_account_data).unwrap()
        );
    }

    #[test]
    fn test_exchange_new_swap_request() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (client, owner) = create_client(bank, mint_keypair);

        let profit = create_token_account(&client, &owner);
        let (to_trade, _) = trade(
            &client,
            &owner,
            Direction::To,
            TokenPair::AB,
            Token::A,
            2,
            2,
            2000,
        );
        let (from_trade, _) = trade(
            &client,
            &owner,
            Direction::From,
            TokenPair::AB,
            Token::B,
            3,
            3,
            3000,
        );

        let instruction =
            exchange_instruction::swap_request(&owner.pubkey(), &to_trade, &from_trade, &profit);
        client
            .send_instruction(&owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));

        let to_trade_account_data = client.get_account_data(&to_trade).unwrap().unwrap();
        let from_trade_account_data = client.get_account_data(&from_trade).unwrap().unwrap();
        let profit_account_data = client.get_account_data(&profit).unwrap().unwrap();

        // check results

        assert_eq!(
            TradeOrderInfo {
                owner: owner.pubkey(),
                direction: Direction::To,
                pair: TokenPair::AB,
                tokens: 1,
                price: 2000,
                tokens_settled: 2,
            },
            ExchangeProcessor::deserialize_trade(&to_trade_account_data).unwrap()
        );

        assert_eq!(
            TokenAccountInfo::default()
                .owner(&owner.pubkey())
                .tokens(1, 0, 0, 0),
            ExchangeProcessor::deserialize_account(&from_trade_account_data).unwrap()
        );

        assert_eq!(
            TokenAccountInfo::default()
                .owner(&owner.pubkey())
                .tokens(100_000, 100_001, 100_000, 100_000),
            ExchangeProcessor::deserialize_account(&profit_account_data).unwrap()
        );
    }

    #[test]
    fn test_exchange_trade_to_token_account() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (client, owner) = create_client(bank, mint_keypair);

        let profit = create_token_account(&client, &owner);
        let (to_trade, _) = trade(
            &client,
            &owner,
            Direction::To,
            TokenPair::AB,
            Token::A,
            3,
            3,
            2000,
        );
        let (from_trade, _) = trade(
            &client,
            &owner,
            Direction::From,
            TokenPair::AB,
            Token::B,
            3,
            3,
            3000,
        );

        let instruction =
            exchange_instruction::swap_request(&owner.pubkey(), &to_trade, &from_trade, &profit);
        client
            .send_instruction(&owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));

        let new = create_token_account(&client, &owner);

        let instruction =
            exchange_instruction::transfer_request(&owner.pubkey(), &new, &to_trade, Token::B, 1);
        client
            .send_instruction(&owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));

        let instruction =
            exchange_instruction::transfer_request(&owner.pubkey(), &new, &from_trade, Token::A, 1);
        client
            .send_instruction(&owner, instruction)
            .expect(&format!("{}:{}", line!(), file!()));

        let new_account_data = client.get_account_data(&new).unwrap().unwrap();

        // Check results

        assert_eq!(
            TokenAccountInfo::default()
                .owner(&owner.pubkey())
                .tokens(100_001, 100_001, 100_000, 100_000),
            ExchangeProcessor::deserialize_account(&new_account_data).unwrap()
        );
    }
}
