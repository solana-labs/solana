//! A nonblocking [`RpcSender`] used for unit testing [`RpcClient`](crate::rpc_client::RpcClient).

use {
    crate::rpc_sender::*,
    async_trait::async_trait,
    serde_json::{json, Number, Value},
    solana_account_decoder::{UiAccount, UiAccountEncoding},
    solana_rpc_client_api::{
        client_error::Result,
        config::RpcBlockProductionConfig,
        request::RpcRequest,
        response::{
            Response, RpcAccountBalance, RpcBlockProduction, RpcBlockProductionRange, RpcBlockhash,
            RpcConfirmedTransactionStatusWithSignature, RpcContactInfo, RpcFees, RpcIdentity,
            RpcInflationGovernor, RpcInflationRate, RpcInflationReward, RpcKeyedAccount,
            RpcPerfSample, RpcResponseContext, RpcSimulateTransactionResult, RpcSnapshotSlotInfo,
            RpcStakeActivation, RpcSupply, RpcVersionInfo, RpcVoteAccountInfo,
            RpcVoteAccountStatus, StakeActivationState,
        },
    },
    solana_sdk::{
        account::Account,
        clock::{Slot, UnixTimestamp},
        epoch_info::EpochInfo,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        instruction::InstructionError,
        message::MessageHeader,
        pubkey::Pubkey,
        signature::Signature,
        sysvar::epoch_schedule::EpochSchedule,
        transaction::{self, Transaction, TransactionError, TransactionVersion},
    },
    solana_transaction_status::{
        option_serializer::OptionSerializer, EncodedConfirmedBlock,
        EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction,
        EncodedTransactionWithStatusMeta, Rewards, TransactionBinaryEncoding,
        TransactionConfirmationStatus, TransactionStatus, UiCompiledInstruction, UiMessage,
        UiRawMessage, UiTransaction, UiTransactionStatusMeta,
    },
    solana_version::Version,
    std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::RwLock},
};

pub const PUBKEY: &str = "7RoSF9fUmdphVCpabEoefH81WwrW7orsWonXWqTXkKV8";

pub type Mocks = HashMap<RpcRequest, Value>;
pub struct MockSender {
    mocks: RwLock<Mocks>,
    url: String,
}

/// An [`RpcSender`] used for unit testing [`RpcClient`](crate::rpc_client::RpcClient).
///
/// This is primarily for internal use.
///
/// Unless directed otherwise, it will generally return a reasonable default
/// response, at least for [`RpcRequest`] values for which responses have been
/// implemented.
///
/// The behavior can be customized in two ways:
///
/// 1) The `url` constructor argument is not actually a URL, but a simple string
///    directive that changes `MockSender`s behavior in specific scenarios.
///
///    If `url` is "fails" then any call to `send` will return `Ok(Value::Null)`.
///
///    It is customary to set the `url` to "succeeds" for mocks that should
///    return sucessfully, though this value is not actually interpreted.
///
///    Other possible values of `url` are specific to different `RpcRequest`
///    values. Read the implementation for specifics.
///
/// 2) Custom responses can be configured by providing [`Mocks`] to the
///    [`MockSender::new_with_mocks`] constructor. This type is a [`HashMap`]
///    from [`RpcRequest`] to a JSON [`Value`] response, Any entries in this map
///    override the default behavior for the given request.
impl MockSender {
    pub fn new<U: ToString>(url: U) -> Self {
        Self::new_with_mocks(url, Mocks::default())
    }

    pub fn new_with_mocks<U: ToString>(url: U, mocks: Mocks) -> Self {
        Self {
            url: url.to_string(),
            mocks: RwLock::new(mocks),
        }
    }
}

#[async_trait]
impl RpcSender for MockSender {
    fn get_transport_stats(&self) -> RpcTransportStats {
        RpcTransportStats::default()
    }

    async fn send(
        &self,
        request: RpcRequest,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if let Some(value) = self.mocks.write().unwrap().remove(&request) {
            return Ok(value);
        }
        if self.url == "fails" {
            return Ok(Value::Null);
        }

        let method = &request.build_request_json(42, params.clone())["method"];

        let val = match method.as_str().unwrap() {
            "getAccountInfo" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: Value::Null,
            })?,
            "getBalance" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: Value::Number(Number::from(50)),
            })?,
            "getRecentBlockhash" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: (
                    Value::String(PUBKEY.to_string()),
                    serde_json::to_value(FeeCalculator::default()).unwrap(),
                ),
            })?,
            "getEpochInfo" => serde_json::to_value(EpochInfo {
                epoch: 1,
                slot_index: 2,
                slots_in_epoch: 32,
                absolute_slot: 34,
                block_height: 34,
                transaction_count: Some(123),
            })?,
            "getFeeCalculatorForBlockhash" => {
                let value = if self.url == "blockhash_expired" {
                    Value::Null
                } else {
                    serde_json::to_value(Some(FeeCalculator::default())).unwrap()
                };
                serde_json::to_value(Response {
                    context: RpcResponseContext { slot: 1, api_version: None },
                    value,
                })?
            }
            "getFeeRateGovernor" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: serde_json::to_value(FeeRateGovernor::default()).unwrap(),
            })?,
            "getFees" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: serde_json::to_value(RpcFees {
                    blockhash: PUBKEY.to_string(),
                    fee_calculator: FeeCalculator::default(),
                    last_valid_slot: 42,
                    last_valid_block_height: 42,
                })
                .unwrap(),
            })?,
            "getSignatureStatuses" => {
                let status: transaction::Result<()> = if self.url == "account_in_use" {
                    Err(TransactionError::AccountInUse)
                } else if self.url == "instruction_error" {
                    Err(TransactionError::InstructionError(
                        0,
                        InstructionError::UninitializedAccount,
                    ))
                } else {
                    Ok(())
                };
                let status = if self.url == "sig_not_found" {
                    None
                } else {
                    let err = status.clone().err();
                    Some(TransactionStatus {
                        status,
                        slot: 1,
                        confirmations: None,
                        err,
                        confirmation_status: Some(TransactionConfirmationStatus::Finalized),
                    })
                };
                let statuses: Vec<Option<TransactionStatus>> = params.as_array().unwrap()[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|_| status.clone())
                    .collect();
                serde_json::to_value(Response {
                    context: RpcResponseContext { slot: 1, api_version: None },
                    value: statuses,
                })?
            }
            "getTransaction" => serde_json::to_value(EncodedConfirmedTransactionWithStatusMeta {
                slot: 2,
                transaction: EncodedTransactionWithStatusMeta {
                    version: Some(TransactionVersion::LEGACY),
                    transaction: EncodedTransaction::Json(
                        UiTransaction {
                            signatures: vec!["3AsdoALgZFuq2oUVWrDYhg2pNeaLJKPLf8hU2mQ6U8qJxeJ6hsrPVpMn9ma39DtfYCrDQSvngWRP8NnTpEhezJpE".to_string()],
                            message: UiMessage::Raw(
                                UiRawMessage {
                                    header: MessageHeader {
                                        num_required_signatures: 1,
                                        num_readonly_signed_accounts: 0,
                                        num_readonly_unsigned_accounts: 1,
                                    },
                                    account_keys: vec![
                                        "C6eBmAXKg6JhJWkajGa5YRGUfG4YKXwbxF5Ufv7PtExZ".to_string(),
                                        "2Gd5eoR5J4BV89uXbtunpbNhjmw3wa1NbRHxTHzDzZLX".to_string(),
                                        "11111111111111111111111111111111".to_string(),
                                    ],
                                    recent_blockhash: "D37n3BSG71oUWcWjbZ37jZP7UfsxG2QMKeuALJ1PYvM6".to_string(),
                                    instructions: vec![UiCompiledInstruction {
                                        program_id_index: 2,
                                        accounts: vec![0, 1],
                                        data: "3Bxs49DitAvXtoDR".to_string(),
                                        stack_height: None,
                                    }],
                                    address_table_lookups: None,
                                })
                        }),
                    meta: Some(UiTransactionStatusMeta {
                            err: None,
                            status: Ok(()),
                            fee: 0,
                            pre_balances: vec![499999999999999950, 50, 1],
                            post_balances: vec![499999999999999950, 50, 1],
                            inner_instructions: OptionSerializer::None,
                            log_messages: OptionSerializer::None,
                            pre_token_balances: OptionSerializer::None,
                            post_token_balances: OptionSerializer::None,
                            rewards: OptionSerializer::None,
                            loaded_addresses: OptionSerializer::Skip,
                            return_data: OptionSerializer::Skip,
                            compute_units_consumed: OptionSerializer::Skip,
                        }),
                },
                block_time: Some(1628633791),
            })?,
            "getTransactionCount" => json![1234],
            "getSlot" => json![0],
            "getMaxShredInsertSlot" => json![0],
            "requestAirdrop" => Value::String(Signature::new(&[8; 64]).to_string()),
            "getSnapshotSlot" => Value::Number(Number::from(0)),
            "getHighestSnapshotSlot" => json!(RpcSnapshotSlotInfo {
                full: 100,
                incremental: Some(110),
            }),
            "getBlockHeight" => Value::Number(Number::from(1234)),
            "getSlotLeaders" => json!([PUBKEY]),
            "getBlockProduction" => {
                if params.is_null() {
                    json!(Response {
                        context: RpcResponseContext { slot: 1, api_version: None },
                        value: RpcBlockProduction {
                            by_identity: HashMap::new(),
                            range: RpcBlockProductionRange {
                                first_slot: 1,
                                last_slot: 2,
                            },
                        },
                    })
                } else {
                    let config: Vec<RpcBlockProductionConfig> =
                        serde_json::from_value(params).unwrap();
                    let config = config[0].clone();
                    let mut by_identity = HashMap::new();
                    by_identity.insert(config.identity.unwrap(), (1, 123));
                    let config_range = config.range.unwrap_or_default();

                    json!(Response {
                        context: RpcResponseContext { slot: 1, api_version: None },
                        value: RpcBlockProduction {
                            by_identity,
                            range: RpcBlockProductionRange {
                                first_slot: config_range.first_slot,
                                last_slot: {
                                    if let Some(last_slot) = config_range.last_slot {
                                        last_slot
                                    } else {
                                        2
                                    }
                                },
                            },
                        },
                    })
                }
            }
            "getStakeActivation" => json!(RpcStakeActivation {
                state: StakeActivationState::Activating,
                active: 123,
                inactive: 12,
            }),
            "getStakeMinimumDelegation" => json!(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: 123_456_789,
            }),
            "getSupply" => json!(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: RpcSupply {
                    total: 100000000,
                    circulating: 50000,
                    non_circulating: 20000,
                    non_circulating_accounts: vec![PUBKEY.to_string()],
                },
            }),
            "getLargestAccounts" => {
                let rpc_account_balance = RpcAccountBalance {
                    address: PUBKEY.to_string(),
                    lamports: 10000,
                };

                json!(Response {
                    context: RpcResponseContext { slot: 1, api_version: None },
                    value: vec![rpc_account_balance],
                })
            }
            "getVoteAccounts" => {
                json!(RpcVoteAccountStatus {
                    current: vec![],
                    delinquent: vec![RpcVoteAccountInfo {
                        vote_pubkey: PUBKEY.to_string(),
                        node_pubkey: PUBKEY.to_string(),
                        activated_stake: 0,
                        commission: 0,
                        epoch_vote_account: false,
                        epoch_credits: vec![],
                        last_vote: 0,
                        root_slot: Slot::default(),
                    }],
                })
            }
            "sendTransaction" => {
                let signature = if self.url == "malicious" {
                    Signature::new(&[8; 64]).to_string()
                } else {
                    let tx_str = params.as_array().unwrap()[0].as_str().unwrap().to_string();
                    let data = base64::decode(tx_str).unwrap();
                    let tx: Transaction = bincode::deserialize(&data).unwrap();
                    tx.signatures[0].to_string()
                };
                Value::String(signature)
            }
            "simulateTransaction" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: RpcSimulateTransactionResult {
                    err: None,
                    logs: None,
                    accounts: None,
                    units_consumed: None,
                    return_data: None,
                },
            })?,
            "getMinimumBalanceForRentExemption" => json![20],
            "getVersion" => {
                let version = Version::default();
                json!(RpcVersionInfo {
                    solana_core: version.to_string(),
                    feature_set: Some(version.feature_set),
                })
            }
            "getLatestBlockhash" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: RpcBlockhash {
                    blockhash: PUBKEY.to_string(),
                    last_valid_block_height: 1234,
                },
            })?,
            "getFeeForMessage" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: json!(Some(0)),
            })?,
            "getClusterNodes" => serde_json::to_value(vec![RpcContactInfo {
                pubkey: PUBKEY.to_string(),
                gossip: Some(SocketAddr::from(([10, 239, 6, 48], 8899))),
                tpu: Some(SocketAddr::from(([10, 239, 6, 48], 8856))),
                rpc: Some(SocketAddr::from(([10, 239, 6, 48], 8899))),
                version: Some("1.0.0 c375ce1f".to_string()),
                feature_set: None,
                shred_version: None,
            }])?,
            "getBlock" => serde_json::to_value(EncodedConfirmedBlock {
                previous_blockhash: "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B".to_string(),
                blockhash: "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA".to_string(),
                parent_slot: 429,
                transactions: vec![EncodedTransactionWithStatusMeta {
                    transaction: EncodedTransaction::Binary(
                        "ju9xZWuDBX4pRxX2oZkTjxU5jB4SSTgEGhX8bQ8PURNzyzqKMPPpNvWihx8zUe\
                                 FfrbVNoAaEsNKZvGzAnTDy5bhNT9kt6KFCTBixpvrLCzg4M5UdFUQYrn1gdgjX\
                                 pLHxcaShD81xBNaFDgnA2nkkdHnKtZt4hVSfKAmw3VRZbjrZ7L2fKZBx21CwsG\
                                 hD6onjM2M3qZW5C8J6d1pj41MxKmZgPBSha3MyKkNLkAGFASK"
                            .to_string(),
                        TransactionBinaryEncoding::Base58,
                    ),
                    meta: None,
                    version: Some(TransactionVersion::LEGACY),
                }],
                rewards: Rewards::new(),
                block_time: None,
                block_height: Some(428),
            })?,
            "getBlocks" => serde_json::to_value(vec![1, 2, 3])?,
            "getBlocksWithLimit" => serde_json::to_value(vec![1, 2, 3])?,
            "getSignaturesForAddress" => {
                serde_json::to_value(vec![RpcConfirmedTransactionStatusWithSignature {
                    signature: crate::mock_sender_for_cli::SIGNATURE.to_string(),
                    slot: 123,
                    err: None,
                    memo: None,
                    block_time: None,
                    confirmation_status: Some(TransactionConfirmationStatus::Finalized),
                }])?
            }
            "getBlockTime" => serde_json::to_value(UnixTimestamp::default())?,
            "getEpochSchedule" => serde_json::to_value(EpochSchedule::default())?,
            "getRecentPerformanceSamples" => serde_json::to_value(vec![RpcPerfSample {
                slot: 347873,
                num_transactions: 125,
                num_slots: 123,
                sample_period_secs: 60,
            }])?,
            "getIdentity" => serde_json::to_value(RpcIdentity {
                identity: PUBKEY.to_string(),
            })?,
            "getInflationGovernor" => serde_json::to_value(
                RpcInflationGovernor {
                    initial: 0.08,
                    terminal: 0.015,
                    taper: 0.15,
                    foundation: 0.05,
                    foundation_term: 7.0,
                })?,
            "getInflationRate" => serde_json::to_value(
                RpcInflationRate {
                    total: 0.08,
                    validator: 0.076,
                    foundation: 0.004,
                    epoch: 0,
                })?,
            "getInflationReward" => serde_json::to_value(vec![
                Some(RpcInflationReward {
                    epoch: 2,
                    effective_slot: 224,
                    amount: 2500,
                    post_balance: 499999442500,
                    commission: None,
                })])?,
            "minimumLedgerSlot" => json![123],
            "getMaxRetransmitSlot" => json![123],
            "getMultipleAccounts" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1, api_version: None },
                value: vec![Value::Null, Value::Null]
            })?,
            "getProgramAccounts" => {
                let pubkey = Pubkey::from_str(PUBKEY).unwrap();
                let account = Account {
                    lamports: 1_000_000,
                    data: vec![],
                    owner: pubkey,
                    executable: false,
                    rent_epoch: 0,
                };
                serde_json::to_value(vec![
                    RpcKeyedAccount {
                        pubkey: PUBKEY.to_string(),
                        account: UiAccount::encode(
                            &pubkey,
                            &account,
                            UiAccountEncoding::Base64,
                            None,
                            None,
                        )
                    }
                ])?
            },
            _ => Value::Null,
        };
        Ok(val)
    }

    fn url(&self) -> String {
        format!("MockSender: {}", self.url)
    }
}
