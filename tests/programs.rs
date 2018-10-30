extern crate bincode;
extern crate elf;
extern crate solana;
extern crate solana_sdk;

use bincode::serialize;
use solana::bank::Bank;
#[cfg(feature = "bpf_c")]
use solana::bpf_loader;
use solana::loader_transaction::LoaderTransaction;
use solana::logger;
use solana::mint::Mint;
use solana::native_loader;
use solana::signature::{Keypair, KeypairUtil};
use solana::system_transaction::SystemTransaction;
#[cfg(feature = "bpf_c")]
use solana::tictactoe_program::Command;
use solana::transaction::Transaction;
use solana_sdk::pubkey::Pubkey;
#[cfg(feature = "bpf_c")]
use std::env;
#[cfg(feature = "bpf_c")]
use std::path::PathBuf;

/// BPF program file extension
#[cfg(feature = "bpf_c")]
const PLATFORM_FILE_EXTENSION_BPF: &str = "o";
/// BPF program ELF section name where the program code is located
pub const PLATFORM_SECTION_RS: &str = ".text,entrypoint";
pub const PLATFORM_SECTION_C: &str = ".text.entrypoint";
/// Create a BPF program file name
#[cfg(feature = "bpf_c")]
fn create_bpf_path(name: &str) -> PathBuf {
    let mut pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };
    pathbuf.push("bpf/");
    pathbuf.push(name);
    pathbuf.set_extension(PLATFORM_FILE_EXTENSION_BPF);
    pathbuf
}

fn check_tx_results(bank: &Bank, tx: &Transaction, result: Vec<solana::bank::Result<()>>) {
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], Ok(()));
    assert_eq!(bank.get_signature(&tx.last_id, &tx.signature), Some(Ok(())));
}

struct Loader {
    mint: Mint,
    bank: Bank,
    loader: Pubkey,
}

impl Loader {
    pub fn new_dynamic(loader_name: &str) -> Self {
        let mint = Mint::new(50);
        let bank = Bank::new(&mint);
        let loader = Keypair::new();

        // allocate, populate, finalize, and spawn loader

        let tx = Transaction::system_create(
            &mint.keypair(),
            loader.pubkey(),
            mint.last_id(),
            1,
            56, // TODO
            native_loader::id(),
            0,
        );
        check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

        let name = String::from(loader_name);
        let tx = Transaction::write(
            &loader,
            native_loader::id(),
            0,
            name.as_bytes().to_vec(),
            mint.last_id(),
            0,
        );
        check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

        let tx = Transaction::finalize(&loader, native_loader::id(), mint.last_id(), 0);
        check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

        let tx = Transaction::system_spawn(&loader, mint.last_id(), 0);
        check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

        Loader {
            mint,
            bank,
            loader: loader.pubkey(),
        }
    }

    pub fn new_native() -> Self {
        let mint = Mint::new(50);
        let bank = Bank::new(&mint);
        let loader = native_loader::id();

        Loader { mint, bank, loader }
    }

    #[cfg(feature = "bpf_c")]
    pub fn new_bpf() -> Self {
        let mint = Mint::new(50);
        let bank = Bank::new(&mint);
        let loader = bpf_loader::id();

        Loader { mint, bank, loader }
    }
}

struct Program {
    program: Keypair,
}

impl Program {
    pub fn new(loader: &Loader, userdata: Vec<u8>) -> Self {
        let program = Keypair::new();

        // allocate, populate, finalize and spawn program

        let tx = Transaction::system_create(
            &loader.mint.keypair(),
            program.pubkey(),
            loader.mint.last_id(),
            1,
            userdata.len() as u64,
            loader.loader,
            0,
        );
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );

        let chunk_size = 256; // Size of chunk just needs to fit into tx
        let mut offset = 0;
        for chunk in userdata.chunks(chunk_size) {
            let tx = Transaction::write(
                &program,
                loader.loader,
                offset,
                chunk.to_vec(),
                loader.mint.last_id(),
                0,
            );
            check_tx_results(
                &loader.bank,
                &tx,
                loader.bank.process_transactions(&vec![tx.clone()]),
            );
            offset += chunk_size as u32;
        }

        let tx = Transaction::finalize(&program, loader.loader, loader.mint.last_id(), 0);
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );

        let tx = Transaction::system_spawn(&program, loader.mint.last_id(), 0);
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );

        Program { program }
    }
}

#[test]
fn test_program_native_noop() {
    logger::setup();

    let loader = Loader::new_native();
    let name = String::from("noop");
    let userdata = name.as_bytes().to_vec();
    let program = Program::new(&loader, userdata);

    // Call user program
    let tx = Transaction::new(
        &loader.mint.keypair(),
        &[],
        program.program.pubkey(),
        vec![1u8],
        loader.mint.last_id(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );
}

#[test]
fn test_program_lua_move_funds() {
    logger::setup();

    let loader = Loader::new_dynamic("solana_lua_loader");
    let userdata = r#"
            print("Lua Script!")
            local tokens, _ = string.unpack("I", data)
            accounts[1].tokens = accounts[1].tokens - tokens
            accounts[2].tokens = accounts[2].tokens + tokens
        "#.as_bytes()
    .to_vec();
    let program = Program::new(&loader, userdata);
    let from = Keypair::new();
    let to = Keypair::new().pubkey();

    // Call user program with two accounts

    let tx = Transaction::system_create(
        &loader.mint.keypair(),
        from.pubkey(),
        loader.mint.last_id(),
        10,
        0,
        program.program.pubkey(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );

    let tx = Transaction::system_create(
        &loader.mint.keypair(),
        to,
        loader.mint.last_id(),
        1,
        0,
        program.program.pubkey(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );

    let data = serialize(&10).unwrap();
    let tx = Transaction::new(
        &from,
        &[to],
        program.program.pubkey(),
        data,
        loader.mint.last_id(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );
    assert_eq!(loader.bank.get_balance(&from.pubkey()), 0);
    assert_eq!(loader.bank.get_balance(&to), 11);
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_builtin_bpf_noop() {
    logger::setup();

    let loader = Loader::new_bpf();
    let program = Program::new(
        &loader,
        elf::File::open_path(&create_bpf_path("noop"))
            .unwrap()
            .get_section(PLATFORM_SECTION_C)
            .unwrap()
            .data
            .clone(),
    );

    // Call user program
    let tx = Transaction::new(
        &loader.mint.keypair(),
        &[],
        program.program.pubkey(),
        vec![1u8],
        loader.mint.last_id(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_bpf_noop_c() {
    logger::setup();

    let loader = Loader::new_dynamic("solana_bpf_loader");
    let program = Program::new(
        &loader,
        elf::File::open_path(&create_bpf_path("noop"))
            .unwrap()
            .get_section(PLATFORM_SECTION_C)
            .unwrap()
            .data
            .clone(),
    );

    // Call user program
    let tx = Transaction::new(
        &loader.mint.keypair(),
        &[],
        program.program.pubkey(),
        vec![1u8],
        loader.mint.last_id(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );
}

#[cfg(feature = "bpf_c")]
struct TicTacToe {
    game: Keypair,
}

#[cfg(feature = "bpf_c")]
impl TicTacToe {
    pub fn new(loader: &Loader, program: &Program) -> Self {
        let game = Keypair::new();

        // Create game account
        let tx = Transaction::system_create(
            &loader.mint.keypair(),
            game.pubkey(),
            loader.mint.last_id(),
            1,
            0x78, // corresponds to the C structure size
            program.program.pubkey(),
            0,
        );
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );

        TicTacToe { game }
    }

    pub fn id(&self) -> Pubkey {
        self.game.pubkey().clone()
    }

    pub fn init(&self, loader: &Loader, program: &Program, player: &Pubkey) {
        let userdata = serialize(&Command::Init).unwrap();
        let tx = Transaction::new(
            &self.game,
            &[self.game.pubkey(), *player],
            program.program.pubkey(),
            userdata,
            loader.mint.last_id(),
            0,
        );
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );
    }

    pub fn command(&self, loader: &Loader, program: &Program, command: Command, player: &Pubkey) {
        let userdata = serialize(&command).unwrap();
        let tx = Transaction::new(
            &loader.mint.keypair(),
            &[self.game.pubkey(), *player],
            program.program.pubkey(),
            userdata,
            loader.mint.last_id(),
            0,
        );
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );
    }

    pub fn get_player_x(&self, loader: &Loader) -> Vec<u8> {
        loader
            .bank
            .get_account(&self.game.pubkey())
            .unwrap()
            .userdata[0..32]
            .to_vec()
    }

    pub fn get_player_y(&self, loader: &Loader) -> Vec<u8> {
        loader
            .bank
            .get_account(&self.game.pubkey())
            .unwrap()
            .userdata[32..64]
            .to_vec()
    }

    pub fn game(&self, loader: &Loader) -> Vec<u8> {
        loader
            .bank
            .get_account(&self.game.pubkey())
            .unwrap()
            .userdata[64..68]
            .to_vec()
    }
}

#[cfg(feature = "bpf_c")]
struct Dashboard {
    dashboard: Keypair,
}

#[cfg(feature = "bpf_c")]
impl Dashboard {
    pub fn new(loader: &Loader, program: &Program) -> Self {
        let dashboard = Keypair::new();

        // Create game account
        let tx = Transaction::system_create(
            &loader.mint.keypair(),
            dashboard.pubkey(),
            loader.mint.last_id(),
            1,
            0xD0, // corresponds to the C structure size
            program.program.pubkey(),
            0,
        );
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );

        Dashboard { dashboard }
    }

    pub fn update(&self, loader: &Loader, program: &Program, game: &Pubkey) {
        let tx = Transaction::new(
            &self.dashboard,
            &[self.dashboard.pubkey(), *game],
            program.program.pubkey(),
            vec![],
            loader.mint.last_id(),
            0,
        );
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );
    }

    pub fn get_game(&self, loader: &Loader, since_last: usize) -> Vec<u8> {
        let userdata = loader
            .bank
            .get_account(&self.dashboard.pubkey())
            .unwrap()
            .userdata;

        // TODO serialize
        let last_game = userdata[192] as usize;
        let this_game = (last_game + since_last * 4) % 5;
        let start = 32 + this_game * 32;
        let end = start + 32;

        loader
            .bank
            .get_account(&self.dashboard.pubkey())
            .unwrap()
            .userdata[start..end]
            .to_vec()
    }

    pub fn get_pending(&self, loader: &Loader) -> Vec<u8> {
        loader
            .bank
            .get_account(&self.dashboard.pubkey())
            .unwrap()
            .userdata[0..32]
            .to_vec()
    }
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_bpf_tictactoe_c() {
    logger::setup();

    let loader = Loader::new_dynamic("solana_bpf_loader");
    let program = Program::new(
        &loader,
        elf::File::open_path(&create_bpf_path("tictactoe"))
            .unwrap()
            .get_section(PLATFORM_SECTION_C)
            .unwrap()
            .data
            .clone(),
    );
    let player_x = Pubkey::new(&[0xA; 32]);
    let player_y = Pubkey::new(&[0xB; 32]);

    let ttt = TicTacToe::new(&loader, &program);
    ttt.init(&loader, &program, &player_x);
    ttt.command(&loader, &program, Command::Join(0xAABBCCDD), &player_y);
    ttt.command(&loader, &program, Command::Move(1, 1), &player_x);
    ttt.command(&loader, &program, Command::Move(0, 0), &player_y);
    ttt.command(&loader, &program, Command::Move(2, 0), &player_x);
    ttt.command(&loader, &program, Command::Move(0, 2), &player_y);
    ttt.command(&loader, &program, Command::Move(2, 2), &player_x);
    ttt.command(&loader, &program, Command::Move(0, 1), &player_y);

    assert_eq!(player_x.as_ref(), &ttt.get_player_x(&loader)[..]); // validate x's key
    assert_eq!(player_y.as_ref(), &ttt.get_player_y(&loader)[..]); // validate o's key
    assert_eq!([4, 0, 0, 0], ttt.game(&loader)[..]); // validate that o won
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_bpf_tictactoe_dashboard_c() {
    logger::setup();

    let loader = Loader::new_dynamic("solana_bpf_loader");
    let ttt_program = Program::new(
        &loader,
        elf::File::open_path(&create_bpf_path("tictactoe"))
            .unwrap()
            .get_section(PLATFORM_SECTION_C)
            .unwrap()
            .data
            .clone(),
    );
    let player_x = Pubkey::new(&[0xA; 32]);
    let player_y = Pubkey::new(&[0xB; 32]);

    let ttt1 = TicTacToe::new(&loader, &ttt_program);
    ttt1.init(&loader, &ttt_program, &player_x);
    ttt1.command(&loader, &ttt_program, Command::Join(0xAABBCCDD), &player_y);
    ttt1.command(&loader, &ttt_program, Command::Move(1, 1), &player_x);
    ttt1.command(&loader, &ttt_program, Command::Move(0, 0), &player_y);
    ttt1.command(&loader, &ttt_program, Command::Move(2, 0), &player_x);
    ttt1.command(&loader, &ttt_program, Command::Move(0, 2), &player_y);
    ttt1.command(&loader, &ttt_program, Command::Move(2, 2), &player_x);
    ttt1.command(&loader, &ttt_program, Command::Move(0, 1), &player_y);

    let ttt2 = TicTacToe::new(&loader, &ttt_program);
    ttt2.init(&loader, &ttt_program, &player_x);
    ttt2.command(&loader, &ttt_program, Command::Join(0xAABBCCDD), &player_y);
    ttt2.command(&loader, &ttt_program, Command::Move(1, 1), &player_x);
    ttt2.command(&loader, &ttt_program, Command::Move(0, 0), &player_y);
    ttt2.command(&loader, &ttt_program, Command::Move(2, 0), &player_x);
    ttt2.command(&loader, &ttt_program, Command::Move(0, 2), &player_y);
    ttt2.command(&loader, &ttt_program, Command::Move(2, 2), &player_x);
    ttt2.command(&loader, &ttt_program, Command::Move(0, 1), &player_y);

    let ttt3 = TicTacToe::new(&loader, &ttt_program);
    ttt3.init(&loader, &ttt_program, &player_x);

    let dashboard_program = Program::new(
        &loader,
        elf::File::open_path(&create_bpf_path("tictactoe_dashboard"))
            .unwrap()
            .get_section(PLATFORM_SECTION_C)
            .unwrap()
            .data
            .clone(),
    );
    let dashboard = Dashboard::new(&loader, &dashboard_program);

    dashboard.update(&loader, &dashboard_program, &ttt1.id());
    dashboard.update(&loader, &dashboard_program, &ttt2.id());
    dashboard.update(&loader, &dashboard_program, &ttt3.id());

    assert_eq!(ttt1.id().as_ref(), &dashboard.get_game(&loader, 1)[..]);
    assert_eq!(ttt2.id().as_ref(), &dashboard.get_game(&loader, 0)[..]);
    assert_eq!(ttt3.id().as_ref(), &dashboard.get_pending(&loader)[..]);
}
