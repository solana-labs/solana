use {
    crate::{
        checks::*,
        cli::{log_instruction_custom_error, CliConfig, ProcessResult},
        program::calculate_max_chunk_size,
    },
    log::*,
    solana_cli_output::CliProgramId,
    solana_client::{
        connection_cache::ConnectionCache,
        send_and_confirm_transactions_in_parallel::{
            send_and_confirm_transactions_in_parallel_blocking, SendAndConfirmConfig,
        },
        tpu_client::{TpuClient, TpuClientConfig},
    },
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::config::RpcSendTransactionConfig,
    solana_sdk::{
        account::Account,
        hash::Hash,
        instruction::Instruction,
        loader_v4::{
            self, LoaderV4State,
            LoaderV4Status::{self, Retracted},
        },
        message::Message,
        pubkey::Pubkey,
        signature::Signer,
        system_instruction::{self, SystemError},
        transaction::Transaction,
    },
    std::{cmp::Ordering, sync::Arc},
};

// This function can be used for the following use-cases
// * Deploy a program
//   - buffer_signer argument must contain program signer information
//     (program_address must be same as buffer_signer.pubkey())
// * Redeploy a program using original program account
//   - buffer_signer argument must be None
// * Redeploy a program using a buffer account
//   - buffer_signer argument must contain the temporary buffer account information
//     (program_address must contain program ID and must NOT be same as buffer_signer.pubkey())
#[allow(dead_code)]
fn process_deploy_program(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    program_data: &[u8],
    program_data_len: u32,
    program_address: &Pubkey,
    buffer_signer: Option<&dyn Signer>,
    authority_signer: &dyn Signer,
) -> ProcessResult {
    let blockhash = rpc_client.get_latest_blockhash()?;
    let payer_pubkey = config.signers[0].pubkey();

    let (initial_messages, balance_needed, buffer_address) =
        if let Some(buffer_signer) = buffer_signer {
            let buffer_address = buffer_signer.pubkey();
            let (create_buffer_message, required_lamports) = build_create_buffer_message(
                rpc_client.clone(),
                config,
                program_address,
                &buffer_address,
                &payer_pubkey,
                &authority_signer.pubkey(),
                program_data_len,
                &blockhash,
            )?;

            if let Some(message) = create_buffer_message {
                (vec![message], required_lamports, buffer_address)
            } else {
                (vec![], 0, buffer_address)
            }
        } else {
            build_retract_and_truncate_messages(
                rpc_client.clone(),
                config,
                program_data_len,
                program_address,
                authority_signer,
            )
            .map(|(messages, balance_needed)| (messages, balance_needed, *program_address))?
        };

    // Create and add write messages
    let create_msg = |offset: u32, bytes: Vec<u8>| {
        let instruction =
            loader_v4::write(&buffer_address, &authority_signer.pubkey(), offset, bytes);
        Message::new_with_blockhash(&[instruction], Some(&payer_pubkey), &blockhash)
    };

    let mut write_messages = vec![];
    let chunk_size = calculate_max_chunk_size(&create_msg);
    for (chunk, i) in program_data.chunks(chunk_size).zip(0..) {
        write_messages.push(create_msg((i * chunk_size) as u32, chunk.to_vec()));
    }

    let final_messages = if *program_address != buffer_address {
        build_retract_and_deploy_messages(
            rpc_client.clone(),
            config,
            program_address,
            &buffer_address,
            authority_signer,
        )?
    } else {
        // Create and add deploy message
        vec![Message::new_with_blockhash(
            &[loader_v4::deploy(
                program_address,
                &authority_signer.pubkey(),
            )],
            Some(&payer_pubkey),
            &blockhash,
        )]
    };

    check_payer(
        &rpc_client,
        config,
        balance_needed,
        &initial_messages,
        &write_messages,
        &final_messages,
    )?;

    send_messages(
        rpc_client,
        config,
        &initial_messages,
        &write_messages,
        &final_messages,
        buffer_signer,
        authority_signer,
    )?;

    let program_id = CliProgramId {
        program_id: program_address.to_string(),
    };
    Ok(config.output_format.formatted_string(&program_id))
}

#[allow(dead_code)]
fn process_undeploy_program(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    program_address: &Pubkey,
    authority_signer: &dyn Signer,
) -> ProcessResult {
    let blockhash = rpc_client.get_latest_blockhash()?;
    let payer_pubkey = config.signers[0].pubkey();

    let Some(program_account) = rpc_client
        .get_account_with_commitment(program_address, config.commitment)?
        .value
    else {
        return Err("Program account does not exist".into());
    };

    let retract_instruction = build_retract_instruction(
        &program_account,
        program_address,
        &authority_signer.pubkey(),
    )?;

    let mut initial_messages = if let Some(instruction) = retract_instruction {
        vec![Message::new_with_blockhash(
            &[instruction],
            Some(&payer_pubkey),
            &blockhash,
        )]
    } else {
        vec![]
    };

    let truncate_instruction = loader_v4::truncate(
        program_address,
        &authority_signer.pubkey(),
        0,
        &payer_pubkey,
    );

    initial_messages.push(Message::new_with_blockhash(
        &[truncate_instruction],
        Some(&payer_pubkey),
        &blockhash,
    ));

    check_payer(&rpc_client, config, 0, &initial_messages, &[], &[])?;

    send_messages(
        rpc_client,
        config,
        &initial_messages,
        &[],
        &[],
        None,
        authority_signer,
    )?;

    let program_id = CliProgramId {
        program_id: program_address.to_string(),
    };
    Ok(config.output_format.formatted_string(&program_id))
}

#[allow(dead_code)]
fn process_finalize_program(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    program_address: &Pubkey,
    authority_signer: &dyn Signer,
) -> ProcessResult {
    let blockhash = rpc_client.get_latest_blockhash()?;
    let payer_pubkey = config.signers[0].pubkey();

    let message = [Message::new_with_blockhash(
        &[loader_v4::transfer_authority(
            program_address,
            &authority_signer.pubkey(),
            None,
        )],
        Some(&payer_pubkey),
        &blockhash,
    )];
    check_payer(&rpc_client, config, 0, &message, &[], &[])?;

    send_messages(
        rpc_client,
        config,
        &message,
        &[],
        &[],
        None,
        authority_signer,
    )?;

    let program_id = CliProgramId {
        program_id: program_address.to_string(),
    };
    Ok(config.output_format.formatted_string(&program_id))
}

#[allow(dead_code)]
fn check_payer(
    rpc_client: &RpcClient,
    config: &CliConfig,
    balance_needed: u64,
    initial_messages: &[Message],
    write_messages: &[Message],
    other_messages: &[Message],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut fee = 0;
    for message in initial_messages {
        fee += rpc_client.get_fee_for_message(message)?;
    }
    for message in other_messages {
        fee += rpc_client.get_fee_for_message(message)?;
    }
    if !write_messages.is_empty() {
        // Assume all write messages cost the same
        if let Some(message) = write_messages.get(0) {
            fee += rpc_client.get_fee_for_message(message)? * (write_messages.len() as u64);
        }
    }
    check_account_for_spend_and_fee_with_commitment(
        rpc_client,
        &config.signers[0].pubkey(),
        balance_needed,
        fee,
        config.commitment,
    )?;
    Ok(())
}

#[allow(dead_code)]
fn send_messages(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    initial_messages: &[Message],
    write_messages: &[Message],
    final_messages: &[Message],
    program_signer: Option<&dyn Signer>,
    authority_signer: &dyn Signer,
) -> Result<(), Box<dyn std::error::Error>> {
    let payer_signer = config.signers[0];

    for message in initial_messages {
        if message.header.num_required_signatures == 3 {
            // The initial message that creates the account and truncates it to the required size requires
            // 3 signatures (payer, program, and authority).
            if let Some(initial_signer) = program_signer {
                let blockhash = rpc_client.get_latest_blockhash()?;

                let mut initial_transaction = Transaction::new_unsigned(message.clone());
                initial_transaction
                    .try_sign(&[payer_signer, initial_signer, authority_signer], blockhash)?;
                let result =
                    rpc_client.send_and_confirm_transaction_with_spinner(&initial_transaction);
                log_instruction_custom_error::<SystemError>(result, config)
                    .map_err(|err| format!("Account allocation failed: {err}"))?;
            } else {
                return Err("Buffer account not created yet, must provide a key pair".into());
            }
        } else if message.header.num_required_signatures == 2 {
            // All other messages should require 2 signatures (payer, and authority)
            let blockhash = rpc_client.get_latest_blockhash()?;

            let mut initial_transaction = Transaction::new_unsigned(message.clone());
            initial_transaction.try_sign(&[payer_signer, authority_signer], blockhash)?;
            let result = rpc_client.send_and_confirm_transaction_with_spinner(&initial_transaction);
            log_instruction_custom_error::<SystemError>(result, config)
                .map_err(|err| format!("Failed to send initial message: {err}"))?;
        } else {
            return Err("Initial message requires incorrect number of signatures".into());
        }
    }

    if !write_messages.is_empty() {
        trace!("Writing program data");
        let connection_cache = if config.use_quic {
            ConnectionCache::new_quic("connection_cache_cli_program_v4_quic", 1)
        } else {
            ConnectionCache::with_udp("connection_cache_cli_program_v4_udp", 1)
        };
        let transaction_errors = match connection_cache {
            ConnectionCache::Udp(cache) => TpuClient::new_with_connection_cache(
                rpc_client.clone(),
                &config.websocket_url,
                TpuClientConfig::default(),
                cache,
            )?
            .send_and_confirm_messages_with_spinner(
                write_messages,
                &[payer_signer, authority_signer],
            ),
            ConnectionCache::Quic(cache) => {
                let tpu_client_fut =
                    solana_client::nonblocking::tpu_client::TpuClient::new_with_connection_cache(
                        rpc_client.get_inner_client().clone(),
                        config.websocket_url.as_str(),
                        solana_client::tpu_client::TpuClientConfig::default(),
                        cache,
                    );
                let tpu_client = rpc_client
                    .runtime()
                    .block_on(tpu_client_fut)
                    .expect("Should return a valid tpu client");

                send_and_confirm_transactions_in_parallel_blocking(
                    rpc_client.clone(),
                    Some(tpu_client),
                    write_messages,
                    &[payer_signer, authority_signer],
                    SendAndConfirmConfig {
                        resign_txs_count: Some(5),
                        with_spinner: true,
                    },
                )
            }
        }
        .map_err(|err| format!("Data writes to account failed: {err}"))?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        if !transaction_errors.is_empty() {
            for transaction_error in &transaction_errors {
                error!("{:?}", transaction_error);
            }
            return Err(format!("{} write transactions failed", transaction_errors.len()).into());
        }
    }

    for message in final_messages {
        let blockhash = rpc_client.get_latest_blockhash()?;
        let mut final_tx = Transaction::new_unsigned(message.clone());
        final_tx.try_sign(&[payer_signer, authority_signer], blockhash)?;
        rpc_client
            .send_and_confirm_transaction_with_spinner_and_config(
                &final_tx,
                config.commitment,
                RpcSendTransactionConfig {
                    skip_preflight: true,
                    preflight_commitment: Some(config.commitment.commitment),
                    ..RpcSendTransactionConfig::default()
                },
            )
            .map_err(|e| format!("Deploying program failed: {e}"))?;
    }

    Ok(())
}

#[allow(dead_code)]
fn build_create_buffer_message(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    program_address: &Pubkey,
    buffer_address: &Pubkey,
    payer_address: &Pubkey,
    authority: &Pubkey,
    program_data_length: u32,
    blockhash: &Hash,
) -> Result<(Option<Message>, u64), Box<dyn std::error::Error>> {
    let expected_account_data_len =
        LoaderV4State::program_data_offset().saturating_add(program_data_length as usize);
    let lamports_required =
        rpc_client.get_minimum_balance_for_rent_exemption(expected_account_data_len)?;

    if let Some(account) = rpc_client
        .get_account_with_commitment(buffer_address, config.commitment)?
        .value
    {
        if !loader_v4::check_id(&account.owner) {
            return Err("Buffer account passed is already in use by another program".into());
        }

        if account.lamports < lamports_required || account.data.len() != expected_account_data_len {
            if program_address == buffer_address {
                return Err("Buffer account passed could be for a different deploy? It has different size/lamports".into());
            }

            let (truncate_instructions, balance_needed) = build_truncate_instructions(
                rpc_client.clone(),
                payer_address,
                &account,
                buffer_address,
                authority,
                program_data_length,
            )?;
            if !truncate_instructions.is_empty() {
                Ok((
                    Some(Message::new_with_blockhash(
                        &truncate_instructions,
                        Some(payer_address),
                        blockhash,
                    )),
                    balance_needed,
                ))
            } else {
                Ok((None, 0))
            }
        } else {
            Ok((None, 0))
        }
    } else {
        Ok((
            Some(Message::new_with_blockhash(
                &loader_v4::create_buffer(
                    payer_address,
                    buffer_address,
                    lamports_required,
                    authority,
                    program_data_length,
                    payer_address,
                ),
                Some(payer_address),
                blockhash,
            )),
            lamports_required,
        ))
    }
}

fn build_retract_and_truncate_messages(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    program_data_len: u32,
    program_address: &Pubkey,
    authority_signer: &dyn Signer,
) -> Result<(Vec<Message>, u64), Box<dyn std::error::Error>> {
    let payer_pubkey = config.signers[0].pubkey();
    let blockhash = rpc_client.get_latest_blockhash()?;
    let Some(program_account) = rpc_client
        .get_account_with_commitment(program_address, config.commitment)?
        .value
    else {
        return Err("Program account does not exist".into());
    };

    let retract_instruction = build_retract_instruction(
        &program_account,
        program_address,
        &authority_signer.pubkey(),
    )?;

    let mut messages = if let Some(instruction) = retract_instruction {
        vec![Message::new_with_blockhash(
            &[instruction],
            Some(&payer_pubkey),
            &blockhash,
        )]
    } else {
        vec![]
    };

    let (truncate_instructions, balance_needed) = build_truncate_instructions(
        rpc_client.clone(),
        &payer_pubkey,
        &program_account,
        program_address,
        &authority_signer.pubkey(),
        program_data_len,
    )?;

    if !truncate_instructions.is_empty() {
        messages.push(Message::new_with_blockhash(
            &truncate_instructions,
            Some(&payer_pubkey),
            &blockhash,
        ));
    }

    Ok((messages, balance_needed))
}

fn build_retract_and_deploy_messages(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    program_address: &Pubkey,
    buffer_address: &Pubkey,
    authority_signer: &dyn Signer,
) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let blockhash = rpc_client.get_latest_blockhash()?;
    let payer_pubkey = config.signers[0].pubkey();

    let Some(program_account) = rpc_client
        .get_account_with_commitment(program_address, config.commitment)?
        .value
    else {
        return Err("Program account does not exist".into());
    };

    let retract_instruction = build_retract_instruction(
        &program_account,
        program_address,
        &authority_signer.pubkey(),
    )?;

    let mut messages = if let Some(instruction) = retract_instruction {
        vec![Message::new_with_blockhash(
            &[instruction],
            Some(&payer_pubkey),
            &blockhash,
        )]
    } else {
        vec![]
    };

    // Create and add deploy message
    messages.push(Message::new_with_blockhash(
        &[loader_v4::deploy_from_source(
            program_address,
            &authority_signer.pubkey(),
            buffer_address,
        )],
        Some(&payer_pubkey),
        &blockhash,
    ));
    Ok(messages)
}

#[allow(dead_code)]
fn build_retract_instruction(
    account: &Account,
    buffer_address: &Pubkey,
    authority: &Pubkey,
) -> Result<Option<Instruction>, Box<dyn std::error::Error>> {
    if !loader_v4::check_id(&account.owner) {
        return Err("Buffer account passed is already in use by another program".into());
    }

    if let Ok(LoaderV4State {
        slot: _,
        authority_address,
        status,
    }) = solana_loader_v4_program::get_state(&account.data)
    {
        if authority != authority_address {
            return Err(
                "Program authority does not match with the provided authority address".into(),
            );
        }

        match status {
            Retracted => Ok(None),
            LoaderV4Status::Deployed => Ok(Some(loader_v4::retract(buffer_address, authority))),
            LoaderV4Status::Finalized => Err("Program is immutable".into()),
        }
    } else {
        Err("Program account's state could not be deserialized".into())
    }
}

#[allow(dead_code)]
fn build_truncate_instructions(
    rpc_client: Arc<RpcClient>,
    payer: &Pubkey,
    account: &Account,
    buffer_address: &Pubkey,
    authority: &Pubkey,
    program_data_length: u32,
) -> Result<(Vec<Instruction>, u64), Box<dyn std::error::Error>> {
    if !loader_v4::check_id(&account.owner) {
        return Err("Buffer account passed is already in use by another program".into());
    }

    let truncate_instruction = if account.data.is_empty() {
        loader_v4::truncate_uninitialized(buffer_address, authority, program_data_length, payer)
    } else {
        if let Ok(LoaderV4State {
            slot: _,
            authority_address,
            status,
        }) = solana_loader_v4_program::get_state(&account.data)
        {
            if authority != authority_address {
                return Err(
                    "Program authority does not match with the provided authority address".into(),
                );
            }

            if matches!(status, LoaderV4Status::Finalized) {
                return Err("Program is immutable and it cannot be truncated".into());
            }
        } else {
            return Err("Program account's state could not be deserialized".into());
        }

        loader_v4::truncate(buffer_address, authority, program_data_length, payer)
    };

    let expected_account_data_len =
        LoaderV4State::program_data_offset().saturating_add(program_data_length as usize);

    let lamports_required =
        rpc_client.get_minimum_balance_for_rent_exemption(expected_account_data_len)?;

    match account.data.len().cmp(&expected_account_data_len) {
        Ordering::Less => {
            if account.lamports < lamports_required {
                let extra_lamports_required = lamports_required.saturating_sub(account.lamports);
                Ok((
                    vec![
                        system_instruction::transfer(
                            payer,
                            buffer_address,
                            extra_lamports_required,
                        ),
                        truncate_instruction,
                    ],
                    extra_lamports_required,
                ))
            } else {
                Ok((vec![truncate_instruction], 0))
            }
        }
        Ordering::Equal => {
            if account.lamports < lamports_required {
                return Err("Program account has less lamports than required for its size".into());
            }
            Ok((vec![], 0))
        }
        Ordering::Greater => {
            if account.lamports < lamports_required {
                return Err("Program account has less lamports than required for its size".into());
            }
            Ok((vec![truncate_instruction], 0))
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        serde_json::json,
        solana_rpc_client_api::{
            request::RpcRequest,
            response::{Response, RpcResponseContext},
        },
        solana_sdk::signature::keypair_from_seed,
        std::collections::HashMap,
    };

    fn program_authority() -> solana_sdk::signature::Keypair {
        keypair_from_seed(&[3u8; 32]).unwrap()
    }

    fn rpc_client_no_existing_program() -> RpcClient {
        RpcClient::new_mock("succeeds".to_string())
    }

    fn rpc_client_with_program_data(data: &str, loader_is_owner: bool) -> RpcClient {
        let owner = if loader_is_owner {
            "LoaderV411111111111111111111111111111111111"
        } else {
            "Vote111111111111111111111111111111111111111"
        };
        let account_info_response = json!(Response {
            context: RpcResponseContext {
                slot: 1,
                api_version: None
            },
            value: json!({
                "data": [data, "base64"],
                "lamports": 42,
                "owner": owner,
                "executable": true,
                "rentEpoch": 1,
            }),
        });
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, account_info_response);
        RpcClient::new_mock_with_mocks("".to_string(), mocks)
    }

    fn rpc_client_wrong_account_owner() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAADtSSjGKNHCxurpAziQWZVhKVknOlxj+TY2wUYUrIc30QAAAAAAAAAA",
            false,
        )
    }

    fn rpc_client_wrong_authority() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            true,
        )
    }

    fn rpc_client_with_program_retracted() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAADtSSjGKNHCxurpAziQWZVhKVknOlxj+TY2wUYUrIc30QAAAAAAAAAA",
            true,
        )
    }

    fn rpc_client_with_program_deployed() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAADtSSjGKNHCxurpAziQWZVhKVknOlxj+TY2wUYUrIc30QEAAAAAAAAA",
            true,
        )
    }

    fn rpc_client_with_program_finalized() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAADtSSjGKNHCxurpAziQWZVhKVknOlxj+TY2wUYUrIc30QIAAAAAAAAA",
            true,
        )
    }

    #[test]
    fn test_deploy() {
        let mut config = CliConfig::default();
        let data = [5u8; 2048];

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let authority_signer = program_authority();

        config.signers.push(&payer);

        assert!(process_deploy_program(
            Arc::new(rpc_client_no_existing_program()),
            &config,
            &data,
            data.len() as u32,
            &program_signer.pubkey(),
            Some(&program_signer),
            &authority_signer,
        )
        .is_ok());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_account_owner()),
            &config,
            &data,
            data.len() as u32,
            &program_signer.pubkey(),
            Some(&program_signer),
            &authority_signer,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &data,
            data.len() as u32,
            &program_signer.pubkey(),
            Some(&program_signer),
            &authority_signer,
        )
        .is_err());
    }

    #[test]
    fn test_redeploy() {
        let mut config = CliConfig::default();
        let data = [5u8; 2048];

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_address = Pubkey::new_unique();
        let authority_signer = program_authority();

        config.signers.push(&payer);

        // Redeploying a non-existent program should fail
        assert!(process_deploy_program(
            Arc::new(rpc_client_no_existing_program()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            None,
            &authority_signer,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_with_program_retracted()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            None,
            &authority_signer,
        )
        .is_ok());

        assert!(process_deploy_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            None,
            &authority_signer,
        )
        .is_ok());

        assert!(process_deploy_program(
            Arc::new(rpc_client_with_program_finalized()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            None,
            &authority_signer,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_account_owner()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            None,
            &authority_signer,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_authority()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            None,
            &authority_signer,
        )
        .is_err());
    }

    #[test]
    fn test_redeploy_from_source() {
        let mut config = CliConfig::default();
        let data = [5u8; 2048];

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let buffer_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let program_address = Pubkey::new_unique();
        let authority_signer = program_authority();

        config.signers.push(&payer);

        // Redeploying a non-existent program should fail
        assert!(process_deploy_program(
            Arc::new(rpc_client_no_existing_program()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            Some(&buffer_signer),
            &authority_signer,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_account_owner()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            Some(&buffer_signer),
            &authority_signer,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_authority()),
            &config,
            &data,
            data.len() as u32,
            &program_address,
            Some(&buffer_signer),
            &authority_signer,
        )
        .is_err());
    }

    #[test]
    fn test_undeploy() {
        let mut config = CliConfig::default();

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let authority_signer = program_authority();

        config.signers.push(&payer);

        assert!(process_undeploy_program(
            Arc::new(rpc_client_no_existing_program()),
            &config,
            &program_signer.pubkey(),
            &authority_signer,
        )
        .is_err());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_with_program_retracted()),
            &config,
            &program_signer.pubkey(),
            &authority_signer,
        )
        .is_ok());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &program_signer.pubkey(),
            &authority_signer,
        )
        .is_ok());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_with_program_finalized()),
            &config,
            &program_signer.pubkey(),
            &authority_signer,
        )
        .is_err());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_wrong_account_owner()),
            &config,
            &program_signer.pubkey(),
            &authority_signer,
        )
        .is_err());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_wrong_authority()),
            &config,
            &program_signer.pubkey(),
            &authority_signer,
        )
        .is_err());
    }

    #[test]
    fn test_finalize() {
        let mut config = CliConfig::default();

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let authority_signer = program_authority();

        config.signers.push(&payer);

        assert!(process_finalize_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &program_signer.pubkey(),
            &authority_signer,
        )
        .is_ok());
    }
}
