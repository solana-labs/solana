use {
    serde::Serialize,
    solana_sdk::{
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        client::Client,
        instruction::{AccountMeta, Instruction},
        loader_instruction,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
    },
};

const CHUNK_SIZE: usize = 512; // Size of chunk just needs to fit into tx

pub fn load_program<T: Client>(
    bank_client: &T,
    from_keypair: &Keypair,
    loader_pubkey: &Pubkey,
    program: Vec<u8>,
) -> Pubkey {
    let program_keypair = Keypair::new();
    let program_pubkey = program_keypair.pubkey();

    let instruction = system_instruction::create_account(
        &from_keypair.pubkey(),
        &program_pubkey,
        1.max(
            bank_client
                .get_minimum_balance_for_rent_exemption(program.len())
                .unwrap(),
        ),
        program.len() as u64,
        loader_pubkey,
    );
    bank_client
        .send_and_confirm_message(
            &[from_keypair, &program_keypair],
            Message::new(&[instruction], Some(&from_keypair.pubkey())),
        )
        .unwrap();

    let chunk_size = CHUNK_SIZE;
    let mut offset = 0;
    for chunk in program.chunks(chunk_size) {
        let instruction =
            loader_instruction::write(&program_pubkey, loader_pubkey, offset, chunk.to_vec());
        let message = Message::new(&[instruction], Some(&from_keypair.pubkey()));
        bank_client
            .send_and_confirm_message(&[from_keypair, &program_keypair], message)
            .unwrap();
        offset += chunk_size as u32;
    }

    let instruction = loader_instruction::finalize(&program_pubkey, loader_pubkey);
    let message = Message::new(&[instruction], Some(&from_keypair.pubkey()));
    bank_client
        .send_and_confirm_message(&[from_keypair, &program_keypair], message)
        .unwrap();

    program_pubkey
}

pub fn load_buffer_account<T: Client>(
    bank_client: &T,
    from_keypair: &Keypair,
    buffer_keypair: &Keypair,
    buffer_authority_keypair: &Keypair,
    program: &[u8],
) {
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
}

pub fn load_upgradeable_program<T: Client>(
    bank_client: &T,
    from_keypair: &Keypair,
    buffer_keypair: &Keypair,
    executable_keypair: &Keypair,
    authority_keypair: &Keypair,
    program: Vec<u8>,
) {
    let program_pubkey = executable_keypair.pubkey();
    let authority_pubkey = authority_keypair.pubkey();

    load_buffer_account(
        bank_client,
        from_keypair,
        buffer_keypair,
        authority_keypair,
        &program,
    );

    let message = Message::new(
        &bpf_loader_upgradeable::deploy_with_max_program_len(
            &from_keypair.pubkey(),
            &program_pubkey,
            &buffer_keypair.pubkey(),
            &authority_pubkey,
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
}

pub fn upgrade_program<T: Client>(
    bank_client: &T,
    from_keypair: &Keypair,
    program_pubkey: &Pubkey,
    buffer_pubkey: &Pubkey,
    authority_keypair: &Keypair,
    spill_pubkey: &Pubkey,
) {
    let message = Message::new(
        &[bpf_loader_upgradeable::upgrade(
            program_pubkey,
            buffer_pubkey,
            &authority_keypair.pubkey(),
            spill_pubkey,
        )],
        Some(&from_keypair.pubkey()),
    );
    bank_client
        .send_and_confirm_message(&[from_keypair, authority_keypair], message)
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
