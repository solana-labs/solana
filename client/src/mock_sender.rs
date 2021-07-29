//! An [`RpcSender`] used for unit testing [`RpcClient`](crate::rpc_client::RpcClient).

use {
    crate::{
        client_error::Result,
        rpc_request::RpcRequest,
        rpc_response::{
            Response, RpcBlockProduction, RpcBlockProductionRange, RpcResponseContext,
            RpcSimulateTransactionResult, RpcVersionInfo,
        },
        rpc_sender::RpcSender,
    },
    serde_json::{json, Number, Value},
    solana_sdk::{
        epoch_info::EpochInfo,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        instruction::InstructionError,
        signature::Signature,
        transaction::{self, Transaction, TransactionError},
    },
    solana_transaction_status::{TransactionConfirmationStatus, TransactionStatus},
    solana_version::Version,
    std::{collections::HashMap, sync::RwLock},
};

pub const PUBKEY: &str = "7RoSF9fUmdphVCpabEoefH81WwrW7orsWonXWqTXkKV8";
pub const SIGNATURE: &str =
    "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8";

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
    pub fn new(url: String) -> Self {
        Self::new_with_mocks(url, Mocks::default())
    }

    pub fn new_with_mocks(url: String, mocks: Mocks) -> Self {
        Self {
            url,
            mocks: RwLock::new(mocks),
        }
    }
}

impl RpcSender for MockSender {
    fn send(&self, request: RpcRequest, params: serde_json::Value) -> Result<serde_json::Value> {
        if let Some(value) = self.mocks.write().unwrap().remove(&request) {
            return Ok(value);
        }
        if self.url == "fails" {
            return Ok(Value::Null);
        }

        let method = &request.build_request_json(42, params.clone())["method"];

        let val = match method.as_str().unwrap() {
            "getAccountInfo" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1 },
                value: Value::Null,
            })?,
            "getBalance" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1 },
                value: Value::Number(Number::from(50)),
            })?,
            "getRecentBlockhash" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1 },
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
                    context: RpcResponseContext { slot: 1 },
                    value,
                })?
            }
            "getFeeRateGovernor" => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1 },
                value: serde_json::to_value(FeeRateGovernor::default()).unwrap(),
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
                    context: RpcResponseContext { slot: 1 },
                    value: statuses,
                })?
            }
            "getTransactionCount" => json![1234],
            "getSlot" => json![0],
            "getMaxShredInsertSlot" => json![0],
            "requestAirdrop" => Value::String(Signature::new(&[8; 64]).to_string()),
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
                context: RpcResponseContext { slot: 1 },
                value: RpcSimulateTransactionResult {
                    err: None,
                    logs: None,
                    accounts: None,
                    units_consumed: None,
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
            "getBlockProduction" => {
                let map = vec![(PUBKEY.to_string(), (1, 1))].into_iter().collect();
                json!(Response {
                    context: RpcResponseContext { slot: 1 },
                    value: RpcBlockProduction {
                        by_identity: map,
                        range: RpcBlockProductionRange {
                            first_slot: 0,
                            last_slot: 0,
                        },
                    },
                })
            }
            _ => Value::Null,
        };
        Ok(val)
    }
}
