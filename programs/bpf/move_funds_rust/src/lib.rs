// extern crate bincode;
// extern crate solana_program_interface;

// // //use bincode::{deserialize, serialize};
// // use bincode::deserialize;
// use solana_program_interface::account::{Account, KeyedAccount};
// // use solana_program_interface::pubkey::Pubkey;
// use std::mem;
// use std::mem::transmute;

// #[no_mangle]
// #[link_section = ".text,entrypoint"] // TODO platform independent needed
// pub extern "C" fn entrypoint(raw: *mut u8) {
//     let bpf_func_trace_printk =
//         unsafe { transmute::<u64, extern "C" fn(u64, u64, u64, u64, u64)>(6) };

//     // unsafe {
//     //     //let pa: u64 = transmute(raw[..7]);
//     //     let pa = raw as *const u64;
//     //     let pd = raw.offset(8) as *const u64;
//     //     bpf_func_trace_printk(0, 0, raw as u64, *pa, *pd);

//     //     //let infos: &mut Vec<KeyedAccount> = (pa as *mut Vec<KeyedAccount>).as_mut().unwrap();
//     //     // bpf_func_trace_printk(0, 0, infos[0].account.tokens as u64, infos[1].account.tokens as u64, 0);
//     // }

//     // let v: Vec<u64> = vec![5; 10];
//     // bpf_func_trace_printk(0, 0, v[0], v[1], v[2]);

//     // let args: &[u8] = raw as &[u8]; //.as_mut().unwrap();
//     //let args = unsafe { std::slice::from_raw_parts(raw, mem::size_of::<Account>()) };
//     // unsafe {
//     //     let x = *(raw as *mut u64);
//     //     //let args = std::slice::from_raw_parts(raw, x as usize);
//     //     let mut args: Vec<u64> = Vec::new();
//     //     args.push(*(raw.offset(1 * 8) as *mut u64));
//     //     args.push(*(raw.offset(2 * 8) as *mut u64));
//     //     args.push(*(raw.offset(3 * 8) as *mut u64));
//     //     //bpf_func_trace_printk(0, 0, args[0], args[1], args[2]);
//     // };

//     //let account: Account = deserialize(args).unwrap();

//     // , data: &[u8]
//     //let infos: &mut Vec<KeyedAccount> = deserialize(raw).unwrap();

//     //let tokens: i64 = 100;
//     //let data: Vec<u8> = serialize(&tokens).unwrap();
//     //let keys = vec![Pubkey::default(); 2];
//     //let mut accounts = vec![Account::default(), Account::default()];
//     //accounts[0].tokens = 100;

//     // let mut infos: Vec<_> = (&keys)
//     //     .into_iter()
//     //     .zip(&mut accounts)
//     //     .map(|(key, account)| KeyedAccount { key, account })
//     //     .collect();

//     //     let some_struct = SomeStruct { a: 32 };

//     // let mut v: Vec<u8> = Vec::new();
//     // let view = &some_struct as *const _ as *const u8;
//     // let slice = unsafe { std::slice::from_raw_parts(view, mem::size_of::<SomeStruct>()) };
//     // v.write(slice).expect("Unable to write");

//     // println!("{:?}", v);

//     // let tokens: i64 = deserialize(data).unwrap();
//     // if infos[0].account.tokens >= tokens {
//     //     infos[0].account.tokens -= tokens;
//     //     infos[1].account.tokens += tokens;
//     // } else {
//     //     println!(
//     //         "Insufficient funds, asked {}, only had {}",
//     //         tokens, infos[0].account.tokens
//     //     );
//     // }

//     // let x;
//     // let y;
//     // let z;
//     // unsafe {
//     //     x = { *raw.offset(0) };
//     //     y = { *raw.offset(1) };
//     //     z = { *raw.offset(2) };
//     // }
//     // bpf_func_trace_printk(0, 0, x as u64, y as u64, z as u64);

//     // // everything below here except return value are experiments

//     // // works
//     // let v: Vec<u64> = vec![10, 11, 12];
//     // bpf_func_trace_printk(0, 0, v[0], v[1], v[2]);

//     // // works
//     // let a: [u64; 3] = [10, 11, 12];
//     bpf_func_trace_printk(0, 0, 1, 2, 7);
//     // (x + y + z) as u32
// }

//! tic-tac-toe program

extern crate bincode;
extern crate solana_program_interface;

use solana_program_interface::pubkey::Pubkey;

#[no_mangle]
#[link_section = ".text,entrypoint"] // TODO platform independent needed
pub extern "C" fn entrypoint(raw: *mut u8) {
    let player1 = Pubkey::default();
    // let player2 = Pubkey::default();
    // let mut game = Game::create(&player1);
    //game.join(&player2);
    // game.next_move(player1, 0, 0);
}

// #[derive(Debug, PartialEq)]
// pub enum Error {
//     // GameInProgress,
//     // InvalidArguments,
//     // InvalidMove,
//     // InvalidUserdata,
//     // NoGame,
//     // NotYourTurn,
//     // PlayerNotFound,
//     // UserdataTooSmall,
// }
// // impl std::fmt::Display for Error {
// //     fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
// //         write!(f, "error")
// //     }
// // }
// // impl std::error::Error for Error {}

// type Result<T> = std::result::Result<T, Error>;

// #[derive(Copy, Clone, Debug, PartialEq)]
// enum GridItem {
//     Free,
//     X,
//     O,
// }

// impl Default for GridItem {
//     fn default() -> GridItem {
//         GridItem::Free
//     }
// }

// #[derive(Copy, Clone, Debug, PartialEq)]
// enum State {
//     WaitingForO,
//     ORequestPending,
//     XMove,
//     OMove,
//     XWon,
//     OWon,
//     Draw,
// }
// impl Default for State {
//     fn default() -> State {
//         State::WaitingForO
//     }
// }

// #[derive(Debug, Default, PartialEq)]
// struct Game {
//     // player_x: Pubkey,
//     // player_o: Option<Pubkey>,
//     // state: State,
//     // grid: [GridItem; 9],
// }

// impl Game {
//     // pub fn create(player_x: &Pubkey) -> Game {
//     //     let mut game = Game::default();
//     //     // game.player_x = *player_x;
//     //     // assert_eq!(game.state, State::WaitingForO);
//     //     game
//     // }

//     // #[cfg(test)]
//     // pub fn new(player_x: Pubkey, player_o: Pubkey) -> Game {
//     //     let mut game = Game::create(&player_x);
//     //     game.join(&player_o).unwrap();
//     //     game.accept().unwrap();
//     //     game
//     // }

//     // pub fn join(self: &mut Game, player_o: &Pubkey) -> Result<()> {
//     //     if self.state == State::WaitingForO {
//     //         self.player_o = Some(*player_o);
//     //         self.state = State::ORequestPending;
//     //         Ok(())
//     //     } else {
//     //         Err(Error::NotYourTurn)
//     //     }
//     // }

//     // pub fn accept(self: &mut Game) -> Result<()> {
//     //     if self.state == State::ORequestPending {
//     //         assert!(self.player_o.is_some());
//     //         self.state = State::XMove;
//     //         Ok(())
//     //     } else {
//     //         Err(Error::NotYourTurn)
//     //     }
//     // }

//     // pub fn reject(self: &mut Game) -> Result<()> {
//     //     if self.state == State::ORequestPending {
//     //         assert!(self.player_o.is_some());
//     //         self.player_o = None;
//     //         self.state = State::WaitingForO;
//     //         Ok(())
//     //     } else {
//     //         Err(Error::NotYourTurn)
//     //     }
//     // }

//     // fn same(x_or_o: GridItem, triple: &[GridItem]) -> bool {
//     //     triple.iter().all(|&i| i == x_or_o)
//     // }

//     // pub fn next_move(self: &mut Game, player: Pubkey, x: usize, y: usize) -> Result<()> {
//     //     let grid_index = y * 3 + x;
//     //     if grid_index >= self.grid.len() || self.grid[grid_index] != GridItem::Free {
//     //         return Err(Error::InvalidMove);
//     //     }

//     //     let (x_or_o, won_state) = match self.state {
//     //         State::XMove => {
//     //             if player != self.player_x {
//     //                 return Err(Error::PlayerNotFound)?;
//     //             }
//     //             self.state = State::OMove;
//     //             (GridItem::X, State::XWon)
//     //         }
//     //         State::OMove => {
//     //             if player != self.player_o.unwrap() {
//     //                 return Err(Error::PlayerNotFound)?;
//     //             }
//     //             self.state = State::XMove;
//     //             (GridItem::O, State::OWon)
//     //         }
//     //         _ => {
//     //             return Err(Error::NotYourTurn)?;
//     //         }
//     //     };
//     //     self.grid[grid_index] = x_or_o;

//     //     let winner =
//     //         // Check rows
//     //         Game::same(x_or_o, &self.grid[0..3])
//     //         || Game::same(x_or_o, &self.grid[3..6])
//     //         || Game::same(x_or_o, &self.grid[6..9])
//     //         // Check columns
//     //         || Game::same(x_or_o, &[self.grid[0], self.grid[3], self.grid[6]])
//     //         || Game::same(x_or_o, &[self.grid[1], self.grid[4], self.grid[7]])
//     //         || Game::same(x_or_o, &[self.grid[2], self.grid[5], self.grid[8]])
//     //         // Check both diagonals
//     //         || Game::same(x_or_o, &[self.grid[0], self.grid[4], self.grid[8]])
//     //         || Game::same(x_or_o, &[self.grid[2], self.grid[4], self.grid[6]]);

//     //     if winner {
//     //         self.state = won_state;
//     //     } else if self.grid.iter().all(|&p| p != GridItem::Free) {
//     //         self.state = State::Draw;
//     //     }

//     //     Ok(())
//     // }
// }
