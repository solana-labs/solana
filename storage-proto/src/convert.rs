use crate::StoredExtendedRewards;
use solana_sdk::{
    hash::Hash,
    instruction::CompiledInstruction,
    message::{Message, MessageHeader},
    pubkey::Pubkey,
    signature::Signature,
    transaction::Transaction,
};
use solana_transaction_status::{
    ConfirmedBlock, InnerInstructions, Reward, RewardType, TransactionStatusMeta,
    TransactionWithStatusMeta,
};
use std::convert::{TryFrom, TryInto};

pub mod generated {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        concat!("/proto/solana.storage.confirmed_block.rs")
    ));
}

impl From<Vec<Reward>> for generated::Rewards {
    fn from(rewards: Vec<Reward>) -> Self {
        Self {
            rewards: rewards.into_iter().map(|r| r.into()).collect(),
        }
    }
}

impl From<generated::Rewards> for Vec<Reward> {
    fn from(rewards: generated::Rewards) -> Self {
        rewards.rewards.into_iter().map(|r| r.into()).collect()
    }
}

impl From<StoredExtendedRewards> for generated::Rewards {
    fn from(rewards: StoredExtendedRewards) -> Self {
        Self {
            rewards: rewards
                .into_iter()
                .map(|r| {
                    let r: Reward = r.into();
                    r.into()
                })
                .collect(),
        }
    }
}

impl From<generated::Rewards> for StoredExtendedRewards {
    fn from(rewards: generated::Rewards) -> Self {
        rewards
            .rewards
            .into_iter()
            .map(|r| {
                let r: Reward = r.into();
                r.into()
            })
            .collect()
    }
}

impl From<Reward> for generated::Reward {
    fn from(reward: Reward) -> Self {
        Self {
            pubkey: reward.pubkey,
            lamports: reward.lamports,
            post_balance: reward.post_balance,
            reward_type: match reward.reward_type {
                None => generated::RewardType::Unspecified,
                Some(RewardType::Fee) => generated::RewardType::Fee,
                Some(RewardType::Rent) => generated::RewardType::Rent,
                Some(RewardType::Staking) => generated::RewardType::Staking,
                Some(RewardType::Voting) => generated::RewardType::Voting,
            } as i32,
        }
    }
}

impl From<generated::Reward> for Reward {
    fn from(reward: generated::Reward) -> Self {
        Self {
            pubkey: reward.pubkey,
            lamports: reward.lamports,
            post_balance: reward.post_balance,
            reward_type: match reward.reward_type {
                0 => None,
                1 => Some(RewardType::Fee),
                2 => Some(RewardType::Rent),
                3 => Some(RewardType::Staking),
                4 => Some(RewardType::Voting),
                _ => None,
            },
        }
    }
}

impl From<ConfirmedBlock> for generated::ConfirmedBlock {
    fn from(confirmed_block: ConfirmedBlock) -> Self {
        let ConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions,
            rewards,
            block_time,
        } = confirmed_block;

        Self {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions: transactions.into_iter().map(|tx| tx.into()).collect(),
            rewards: rewards.into_iter().map(|r| r.into()).collect(),
            block_time: block_time.map(|timestamp| generated::UnixTimestamp { timestamp }),
        }
    }
}

impl TryFrom<generated::ConfirmedBlock> for ConfirmedBlock {
    type Error = bincode::Error;
    fn try_from(
        confirmed_block: generated::ConfirmedBlock,
    ) -> std::result::Result<Self, Self::Error> {
        let generated::ConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions,
            rewards,
            block_time,
        } = confirmed_block;

        Ok(Self {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions: transactions
                .into_iter()
                .map(|tx| tx.try_into())
                .collect::<std::result::Result<Vec<TransactionWithStatusMeta>, Self::Error>>()?,
            rewards: rewards.into_iter().map(|r| r.into()).collect(),
            block_time: block_time.map(|generated::UnixTimestamp { timestamp }| timestamp),
        })
    }
}

impl From<TransactionWithStatusMeta> for generated::ConfirmedTransaction {
    fn from(value: TransactionWithStatusMeta) -> Self {
        let meta = if let Some(meta) = value.meta {
            Some(meta.into())
        } else {
            None
        };
        Self {
            transaction: Some(value.transaction.into()),
            meta,
        }
    }
}

impl TryFrom<generated::ConfirmedTransaction> for TransactionWithStatusMeta {
    type Error = bincode::Error;
    fn try_from(value: generated::ConfirmedTransaction) -> std::result::Result<Self, Self::Error> {
        let meta = if let Some(meta) = value.meta {
            Some(meta.try_into()?)
        } else {
            None
        };
        Ok(Self {
            transaction: value.transaction.expect("transaction is required").into(),
            meta,
        })
    }
}

impl From<Transaction> for generated::Transaction {
    fn from(value: Transaction) -> Self {
        Self {
            signatures: value
                .signatures
                .into_iter()
                .map(|signature| <Signature as AsRef<[u8]>>::as_ref(&signature).into())
                .collect(),
            message: Some(value.message.into()),
        }
    }
}

impl From<generated::Transaction> for Transaction {
    fn from(value: generated::Transaction) -> Self {
        Self {
            signatures: value
                .signatures
                .into_iter()
                .map(|x| Signature::new(&x))
                .collect(),
            message: value.message.expect("message is required").into(),
        }
    }
}

impl From<Message> for generated::Message {
    fn from(value: Message) -> Self {
        Self {
            header: Some(value.header.into()),
            account_keys: value
                .account_keys
                .into_iter()
                .map(|key| <Pubkey as AsRef<[u8]>>::as_ref(&key).into())
                .collect(),
            recent_blockhash: value.recent_blockhash.to_bytes().into(),
            instructions: value.instructions.into_iter().map(|ix| ix.into()).collect(),
        }
    }
}

impl From<generated::Message> for Message {
    fn from(value: generated::Message) -> Self {
        Self {
            header: value.header.expect("header is required").into(),
            account_keys: value
                .account_keys
                .into_iter()
                .map(|key| Pubkey::new(&key))
                .collect(),
            recent_blockhash: Hash::new(&value.recent_blockhash),
            instructions: value.instructions.into_iter().map(|ix| ix.into()).collect(),
        }
    }
}

impl From<MessageHeader> for generated::MessageHeader {
    fn from(value: MessageHeader) -> Self {
        Self {
            num_required_signatures: value.num_required_signatures as u32,
            num_readonly_signed_accounts: value.num_readonly_signed_accounts as u32,
            num_readonly_unsigned_accounts: value.num_readonly_unsigned_accounts as u32,
        }
    }
}

impl From<generated::MessageHeader> for MessageHeader {
    fn from(value: generated::MessageHeader) -> Self {
        Self {
            num_required_signatures: value.num_required_signatures as u8,
            num_readonly_signed_accounts: value.num_readonly_signed_accounts as u8,
            num_readonly_unsigned_accounts: value.num_readonly_unsigned_accounts as u8,
        }
    }
}

impl From<TransactionStatusMeta> for generated::TransactionStatusMeta {
    fn from(value: TransactionStatusMeta) -> Self {
        let TransactionStatusMeta {
            status,
            fee,
            pre_balances,
            post_balances,
            inner_instructions,
            log_messages,
        } = value;
        let err = match status {
            Ok(()) => None,
            Err(err) => Some(generated::TransactionError {
                err: bincode::serialize(&err).expect("transaction error to serialize to bytes"),
            }),
        };
        let inner_instructions = inner_instructions
            .unwrap_or_default()
            .into_iter()
            .map(|ii| ii.into())
            .collect();
        let log_messages = log_messages.unwrap_or_default();
        Self {
            err,
            fee,
            pre_balances,
            post_balances,
            inner_instructions,
            log_messages,
        }
    }
}

impl TryFrom<generated::TransactionStatusMeta> for TransactionStatusMeta {
    type Error = bincode::Error;

    fn try_from(value: generated::TransactionStatusMeta) -> std::result::Result<Self, Self::Error> {
        let generated::TransactionStatusMeta {
            err,
            fee,
            pre_balances,
            post_balances,
            inner_instructions,
            log_messages,
        } = value;
        let status = match &err {
            None => Ok(()),
            Some(tx_error) => Err(bincode::deserialize(&tx_error.err)?),
        };
        let inner_instructions = Some(
            inner_instructions
                .into_iter()
                .map(|inner| inner.into())
                .collect(),
        );
        let log_messages = Some(log_messages);
        Ok(Self {
            status,
            fee,
            pre_balances,
            post_balances,
            inner_instructions,
            log_messages,
        })
    }
}

impl From<InnerInstructions> for generated::InnerInstructions {
    fn from(value: InnerInstructions) -> Self {
        Self {
            index: value.index as u32,
            instructions: value.instructions.into_iter().map(|i| i.into()).collect(),
        }
    }
}

impl From<generated::InnerInstructions> for InnerInstructions {
    fn from(value: generated::InnerInstructions) -> Self {
        Self {
            index: value.index as u8,
            instructions: value.instructions.into_iter().map(|i| i.into()).collect(),
        }
    }
}

impl From<CompiledInstruction> for generated::CompiledInstruction {
    fn from(value: CompiledInstruction) -> Self {
        Self {
            program_id_index: value.program_id_index as u32,
            accounts: value.accounts,
            data: value.data,
        }
    }
}

impl From<generated::CompiledInstruction> for CompiledInstruction {
    fn from(value: generated::CompiledInstruction) -> Self {
        Self {
            program_id_index: value.program_id_index as u8,
            accounts: value.accounts,
            data: value.data,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_reward_type_encode() {
        let mut reward = Reward {
            pubkey: "invalid".to_string(),
            lamports: 123,
            post_balance: 321,
            reward_type: None,
        };
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());

        reward.reward_type = Some(RewardType::Fee);
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());

        reward.reward_type = Some(RewardType::Rent);
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());

        reward.reward_type = Some(RewardType::Voting);
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());

        reward.reward_type = Some(RewardType::Staking);
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());
    }
}
