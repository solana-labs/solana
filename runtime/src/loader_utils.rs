use {
    crate::{bank::Bank, bank_client::BankClient},
    serde::Serialize,
    solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        client::{Client, SyncClient},
        clock::Clock,
        instruction::{AccountMeta, Instruction},
        loader_instruction,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
    },
    std::{env, fs::File, io::Read, path::PathBuf},
};

const CHUNK_SIZE: usize = 512; // Size of chunk just needs to fit into tx

pub fn load_program_from_file(name: &str) -> Vec<u8> {
    let mut pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };
    pathbuf.push("sbf/");
    pathbuf.push(name);
    pathbuf.set_extension("so");
    let mut file = File::open(&pathbuf).unwrap_or_else(|err| {
        panic!("Failed to open {}: {}", pathbuf.display(), err);
    });
    let mut program = Vec::new();
    file.read_to_end(&mut program).unwrap();
    program
}

// Creates an unverified program by bypassing the loader built-in program
pub fn create_program(bank: &Bank, loader_id: &Pubkey, name: &str) -> Pubkey {
    let program_id = Pubkey::new_unique();
    let elf = load_program_from_file(name);
    let mut program_account = AccountSharedData::new(1, elf.len(), loader_id);
    program_account
        .data_as_mut_slice()
        .get_mut(..)
        .unwrap()
        .copy_from_slice(&elf);
    program_account.set_executable(true);
    bank.store_account(&program_id, &program_account);
    program_id
}

pub fn load_and_finalize_program<T: Client>(
    bank_client: &T,
    loader_id: &Pubkey,
    program_keypair: Option<Keypair>,
    payer_keypair: &Keypair,
    name: &str,
) -> (Keypair, Instruction) {
    let program = load_program_from_file(name);
    let program_keypair = program_keypair.unwrap_or_else(|| {
        let program_keypair = Keypair::new();
        let instruction = system_instruction::create_account(
            &payer_keypair.pubkey(),
            &program_keypair.pubkey(),
            1.max(
                bank_client
                    .get_minimum_balance_for_rent_exemption(program.len())
                    .unwrap(),
            ),
            program.len() as u64,
            loader_id,
        );
        let message = Message::new(&[instruction], Some(&payer_keypair.pubkey()));
        bank_client
            .send_and_confirm_message(&[payer_keypair, &program_keypair], message)
            .unwrap();
        program_keypair
    });
    let chunk_size = CHUNK_SIZE;
    let mut offset = 0;
    for chunk in program.chunks(chunk_size) {
        let instruction =
            loader_instruction::write(&program_keypair.pubkey(), loader_id, offset, chunk.to_vec());
        let message = Message::new(&[instruction], Some(&payer_keypair.pubkey()));
        bank_client
            .send_and_confirm_message(&[payer_keypair, &program_keypair], message)
            .unwrap();
        offset += chunk_size as u32;
    }
    let instruction = loader_instruction::finalize(&program_keypair.pubkey(), loader_id);
    (program_keypair, instruction)
}

pub fn load_program<T: Client>(
    bank_client: &T,
    loader_id: &Pubkey,
    payer_keypair: &Keypair,
    name: &str,
) -> Pubkey {
    let (program_keypair, instruction) =
        load_and_finalize_program(bank_client, loader_id, None, payer_keypair, name);
    let message = Message::new(&[instruction], Some(&payer_keypair.pubkey()));
    bank_client
        .send_and_confirm_message(&[payer_keypair, &program_keypair], message)
        .unwrap();
    program_keypair.pubkey()
}

pub fn load_upgradeable_buffer<T: Client>(
    bank_client: &T,
    from_keypair: &Keypair,
    buffer_keypair: &Keypair,
    buffer_authority_keypair: &Keypair,
    name: &str,
) -> Vec<u8> {
    let program = load_program_from_file(name);
    let buffer_pubkey = buffer_keypair.pubkey();
    let buffer_authority_pubkey = buffer_authority_keypair.pubkey();

    bank_client
        .send_and_confirm_message(
            &[from_keypair, buffer_keypair],
            Message::new(
                &bpf_loader_upgradeable::create_buffer(
                    &from_keypair.pubkey(),
                    &buffer_pubkey,
                    &buffer_authority_pubkey,
                    1.max(
                        bank_client
                            .get_minimum_balance_for_rent_exemption(program.len())
                            .unwrap(),
                    ),
                    program.len(),
                )
                .unwrap(),
                Some(&from_keypair.pubkey()),
            ),
        )
        .unwrap();

    let chunk_size = CHUNK_SIZE;
    let mut offset = 0;
    for chunk in program.chunks(chunk_size) {
        let message = Message::new(
            &[bpf_loader_upgradeable::write(
                &buffer_pubkey,
                &buffer_authority_pubkey,
                offset,
                chunk.to_vec(),
            )],
            Some(&from_keypair.pubkey()),
        );
        bank_client
            .send_and_confirm_message(&[from_keypair, buffer_authority_keypair], message)
            .unwrap();
        offset += chunk_size as u32;
    }

    program
}

pub fn load_upgradeable_program(
    bank_client: &BankClient,
    from_keypair: &Keypair,
    buffer_keypair: &Keypair,
    executable_keypair: &Keypair,
    authority_keypair: &Keypair,
    name: &str,
) {
    let program = load_upgradeable_buffer(
        bank_client,
        from_keypair,
        buffer_keypair,
        authority_keypair,
        name,
    );

    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &from_keypair.pubkey(),
            &executable_keypair.pubkey(),
            &buffer_keypair.pubkey(),
            &authority_keypair.pubkey(),
            1.max(
                bank_client
                    .get_minimum_balance_for_rent_exemption(
                        UpgradeableLoaderState::size_of_program(),
                    )
                    .unwrap(),
            ),
            program.len() * 2,
        )
        .unwrap(),
        Some(&from_keypair.pubkey()),
    );
    bank_client
        .send_and_confirm_message(
            &[from_keypair, executable_keypair, authority_keypair],
            message,
        )
        .unwrap();
    bank_client.set_sysvar_for_tests(&Clock {
        slot: 1,
        ..Clock::default()
    });
}

pub fn upgrade_program<T: Client>(
    bank_client: &T,
    payer_keypair: &Keypair,
    buffer_keypair: &Keypair,
    executable_pubkey: &Pubkey,
    authority_keypair: &Keypair,
    name: &str,
) {
    load_upgradeable_buffer(
        bank_client,
        payer_keypair,
        buffer_keypair,
        authority_keypair,
        name,
    );
    let message = Message::new(
        &[bpf_loader_upgradeable::upgrade(
            executable_pubkey,
            &buffer_keypair.pubkey(),
            &authority_keypair.pubkey(),
            &payer_keypair.pubkey(),
        )],
        Some(&payer_keypair.pubkey()),
    );
    bank_client
        .send_and_confirm_message(&[payer_keypair, authority_keypair], message)
        .unwrap();
}

pub fn set_upgrade_authority<T: Client>(
    bank_client: &T,
    from_keypair: &Keypair,
    program_pubkey: &Pubkey,
    current_authority_keypair: &Keypair,
    new_authority_pubkey: Option<&Pubkey>,
) {
    let message = Message::new(
        &[bpf_loader_upgradeable::set_upgrade_authority(
            program_pubkey,
            &current_authority_keypair.pubkey(),
            new_authority_pubkey,
        )],
        Some(&from_keypair.pubkey()),
    );
    bank_client
        .send_and_confirm_message(&[from_keypair, current_authority_keypair], message)
        .unwrap();
}

// Return an Instruction that invokes `program_id` with `data` and required
// a signature from `from_pubkey`.
pub fn create_invoke_instruction<T: Serialize>(
    from_pubkey: Pubkey,
    program_id: Pubkey,
    data: &T,
) -> Instruction {
    let account_metas = vec![AccountMeta::new(from_pubkey, true)];
    Instruction::new_with_bincode(program_id, data, account_metas)
}
