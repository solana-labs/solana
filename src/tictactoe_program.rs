//! tic-tac-toe program

use serde_cbor;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std;
use transaction::Transaction;

#[derive(Debug, PartialEq)]
pub enum Error {
    GameInProgress,
    InvalidArguments,
    InvalidMove,
    InvalidUserdata,
    InvalidTimestamp,
    NoGame,
    NotYourTurn,
    PlayerNotFound,
    UserdataTooSmall,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "error")
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
enum BoardItem {
    F, // Free
    X,
    O,
}

impl Default for BoardItem {
    fn default() -> BoardItem {
        BoardItem::F
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum State {
    Waiting,
    XMove,
    OMove,
    XWon,
    OWon,
    Draw,
}
impl Default for State {
    fn default() -> State {
        State::Waiting
    }
}

#[repr(C)]
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Game {
    player_x: Pubkey,
    player_o: Option<Pubkey>,
    pub state: State,
    board: [BoardItem; 9],
    keep_alive: [i64; 2],
}

impl Game {
    pub fn create(player_x: &Pubkey) -> Game {
        let mut game = Game::default();
        game.player_x = *player_x;
        assert_eq!(game.state, State::Waiting);
        game
    }

    #[cfg(test)]
    pub fn new(player_x: Pubkey, player_o: Pubkey) -> Game {
        let mut game = Game::create(&player_x);
        game.join(player_o, 1).unwrap();
        game
    }

    pub fn join(self: &mut Game, player_o: Pubkey, timestamp: i64) -> Result<()> {
        if self.state == State::Waiting {
            self.player_o = Some(player_o);
            self.state = State::XMove;

            if timestamp <= self.keep_alive[1] {
                Err(Error::InvalidTimestamp)
            } else {
                self.keep_alive[1] = timestamp;
                Ok(())
            }
        } else {
            Err(Error::GameInProgress)
        }
    }

    fn same(x_or_o: BoardItem, triple: &[BoardItem]) -> bool {
        triple.iter().all(|&i| i == x_or_o)
    }

    pub fn next_move(self: &mut Game, player: Pubkey, x: usize, y: usize) -> Result<()> {
        let board_index = y * 3 + x;
        if board_index >= self.board.len() || self.board[board_index] != BoardItem::F {
            Err(Error::InvalidMove)?;
        }

        let (x_or_o, won_state) = match self.state {
            State::XMove => {
                if player != self.player_x {
                    return Err(Error::PlayerNotFound);
                }
                self.state = State::OMove;
                (BoardItem::X, State::XWon)
            }
            State::OMove => {
                if player != self.player_o.unwrap() {
                    return Err(Error::PlayerNotFound);
                }
                self.state = State::XMove;
                (BoardItem::O, State::OWon)
            }
            _ => {
                return Err(Error::NotYourTurn);
            }
        };
        self.board[board_index] = x_or_o;

        let winner =
            // Check rows
            Game::same(x_or_o, &self.board[0..3])
            || Game::same(x_or_o, &self.board[3..6])
            || Game::same(x_or_o, &self.board[6..9])
            // Check columns
            || Game::same(x_or_o, &[self.board[0], self.board[3], self.board[6]])
            || Game::same(x_or_o, &[self.board[1], self.board[4], self.board[7]])
            || Game::same(x_or_o, &[self.board[2], self.board[5], self.board[8]])
            // Check both diagonals
            || Game::same(x_or_o, &[self.board[0], self.board[4], self.board[8]])
            || Game::same(x_or_o, &[self.board[2], self.board[4], self.board[6]]);

        if winner {
            self.state = won_state;
        } else if self.board.iter().all(|&p| p != BoardItem::F) {
            self.state = State::Draw;
        }

        Ok(())
    }

    pub fn keep_alive(self: &mut Game, player: Pubkey, timestamp: i64) -> Result<()> {
        match self.state {
            State::Waiting | State::XMove | State::OMove => {
                if player == self.player_x {
                    if timestamp <= self.keep_alive[0] {
                        Err(Error::InvalidTimestamp)?;
                    }
                    self.keep_alive[0] = timestamp;
                } else if Some(player) == self.player_o {
                    if timestamp <= self.keep_alive[1] {
                        Err(Error::InvalidTimestamp)?;
                    }
                    self.keep_alive[1] = timestamp;
                } else {
                    Err(Error::PlayerNotFound)?;
                }
            }
            // Ignore keep_alive when game is no longer in progress
            State::XWon | State::OWon | State::Draw => {}
        };
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub enum Command {
    Init,           // player X initializes a new game
    Join(i64),      // player O wants to join (seconds since UNIX epoch)
    KeepAlive(i64), // player X/O keep alive (seconds since UNIX epoch)
    Move(u8, u8),   // player X/O mark board position (x, y)
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TicTacToeProgram {
    pub game: Option<Game>,
}

pub const TICTACTOE_PROGRAM_ID: [u8; 32] = [
    3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

impl TicTacToeProgram {
    pub fn deserialize(input: &[u8]) -> Result<TicTacToeProgram> {
        let len = input[0] as usize;

        if len == 0 {
            Ok(TicTacToeProgram::default())
        } else if input.len() < len + 1 {
            Err(Error::InvalidUserdata)
        } else {
            serde_cbor::from_slice(&input[1..=len]).map_err(|err| {
                error!("Unable to deserialize game: {:?}", err);
                Error::InvalidUserdata
            })
        }
    }

    fn dispatch_command(self: &mut TicTacToeProgram, cmd: &Command, player: &Pubkey) -> Result<()> {
        info!("dispatch_command: cmd={:?} player={}", cmd, player);
        info!("dispatch_command: account={:?}", self);

        if let Command::Init = cmd {
            return if self.game.is_some() {
                Err(Error::GameInProgress)
            } else {
                let game = Game::create(player);
                self.game = Some(game);
                Ok(())
            };
        }

        if let Some(ref mut game) = self.game {
            match cmd {
                Command::Join(timestamp) => game.join(*player, *timestamp),
                Command::Move(x, y) => game.next_move(*player, *x as usize, *y as usize),
                Command::KeepAlive(timestamp) => game.keep_alive(*player, *timestamp),
                Command::Init => panic!("Unreachable"),
            }
        } else {
            Err(Error::NoGame)
        }
    }

    fn serialize(self: &TicTacToeProgram, output: &mut [u8]) -> Result<()> {
        let self_serialized = serde_cbor::to_vec(self).unwrap();

        if output.len() + 1 < self_serialized.len() {
            warn!(
                "{} bytes required to serialize but only have {} bytes",
                self_serialized.len(),
                output.len() + 1
            );
            return Err(Error::UserdataTooSmall);
        }

        assert!(self_serialized.len() <= 255);
        output[0] = self_serialized.len() as u8;
        output[1..=self_serialized.len()].clone_from_slice(&self_serialized);
        Ok(())
    }

    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == TICTACTOE_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&TICTACTOE_PROGRAM_ID)
    }

    pub fn process_transaction(
        tx: &Transaction,
        pix: usize,
        accounts: &mut [&mut Account],
    ) -> Result<()> {
        // accounts[1] must always be the Tic-tac-toe game state account
        if accounts.len() < 2 || !Self::check_id(&accounts[1].program_id) {
            error!("accounts[1] is not assigned to the TICTACTOE_PROGRAM_ID");
            Err(Error::InvalidArguments)?;
        }
        if accounts[1].userdata.is_empty() {
            error!("accounts[1] userdata is empty");
            Err(Error::InvalidArguments)?;
        }

        let mut program_state = Self::deserialize(&accounts[1].userdata)?;

        let command = serde_cbor::from_slice::<Command>(tx.userdata(pix)).map_err(|err| {
            error!("{:?}", err);
            Error::InvalidUserdata
        })?;

        if let Command::Init = command {
            // Init must be signed by the game state account itself, who's private key is
            // known only to player X
            if !Self::check_id(&accounts[0].program_id) {
                error!("accounts[0] is not assigned to the TICTACTOE_PROGRAM_ID");
                return Err(Error::InvalidArguments);
            }
            // player X public key is in keys[2]
            if tx.key(pix, 2).is_none() {
                Err(Error::InvalidArguments)?;
            }
            program_state.dispatch_command(&command, tx.key(pix, 2).unwrap())?;
        } else {
            program_state.dispatch_command(&command, tx.key(pix, 0).unwrap())?;
        }
        program_state.serialize(&mut accounts[1].userdata)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    pub fn serde_no_game() {
        let account = TicTacToeProgram::default();
        assert!(account.game.is_none());

        let mut userdata = vec![0xff; 256];
        account.serialize(&mut userdata).unwrap();

        let account = TicTacToeProgram::deserialize(&userdata).unwrap();
        assert!(account.game.is_none());

        let account = TicTacToeProgram::deserialize(&[0]).unwrap();
        assert!(account.game.is_none());
    }

    #[test]
    pub fn serde_with_game() {
        let mut account = TicTacToeProgram::default();
        assert!(account.game.is_none());

        let player_x = Pubkey::new(&[1; 32]);
        account.dispatch_command(&Command::Init, &player_x).unwrap();

        let mut userdata = vec![0xff; 256];
        account.serialize(&mut userdata).unwrap();

        let account2 = TicTacToeProgram::deserialize(&userdata).unwrap();

        assert_eq!(account.game.unwrap(), account2.game.unwrap());
    }

    #[test]
    pub fn serde_no_room() {
        let account = TicTacToeProgram::default();
        let mut userdata = vec![0xff; 1];
        assert_eq!(
            account.serialize(&mut userdata),
            Err(Error::UserdataTooSmall)
        );

        let err = TicTacToeProgram::deserialize(&userdata);
        assert!(err.is_err());
        assert_eq!(err.err().unwrap(), Error::InvalidUserdata);
    }

    #[test]
    pub fn column_1_x_wins() {
        /*
            X|O|
            -+-+-
            X|O|
            -+-+-
            X| |
        */

        let player_x = Pubkey::new(&[1; 32]);
        let player_o = Pubkey::new(&[2; 32]);

        let mut g = Game::new(player_x, player_o);
        assert_eq!(g.state, State::XMove);

        g.next_move(player_x, 0, 0).unwrap();
        assert_eq!(g.state, State::OMove);
        g.next_move(player_o, 1, 0).unwrap();
        assert_eq!(g.state, State::XMove);
        g.next_move(player_x, 0, 1).unwrap();
        assert_eq!(g.state, State::OMove);
        g.next_move(player_o, 1, 1).unwrap();
        assert_eq!(g.state, State::XMove);
        g.next_move(player_x, 0, 2).unwrap();
        assert_eq!(g.state, State::XWon);
    }

    #[test]
    pub fn right_diagonal_x_wins() {
        /*
            X|O|X
            -+-+-
            O|X|O
            -+-+-
            X| |
        */

        let player_x = Pubkey::new(&[1; 32]);
        let player_o = Pubkey::new(&[2; 32]);
        let mut g = Game::new(player_x, player_o);

        g.next_move(player_x, 0, 0).unwrap();
        g.next_move(player_o, 1, 0).unwrap();
        g.next_move(player_x, 2, 0).unwrap();
        g.next_move(player_o, 0, 1).unwrap();
        g.next_move(player_x, 1, 1).unwrap();
        g.next_move(player_o, 2, 1).unwrap();
        g.next_move(player_x, 0, 2).unwrap();
        assert_eq!(g.state, State::XWon);

        assert_eq!(g.next_move(player_o, 1, 2), Err(Error::NotYourTurn));
    }

    #[test]
    pub fn bottom_row_o_wins() {
        /*
            X|X|
            -+-+-
            X| |
            -+-+-
            O|O|O
        */

        let player_x = Pubkey::new(&[1; 32]);
        let player_o = Pubkey::new(&[2; 32]);
        let mut g = Game::new(player_x, player_o);

        g.next_move(player_x, 0, 0).unwrap();
        g.next_move(player_o, 0, 2).unwrap();
        g.next_move(player_x, 1, 0).unwrap();
        g.next_move(player_o, 1, 2).unwrap();
        g.next_move(player_x, 0, 1).unwrap();
        g.next_move(player_o, 2, 2).unwrap();
        assert_eq!(g.state, State::OWon);

        assert!(g.next_move(player_x, 1, 2).is_err());
    }

    #[test]
    pub fn left_diagonal_x_wins() {
        /*
            X|O|X
            -+-+-
            O|X|O
            -+-+-
            O|X|X
        */

        let player_x = Pubkey::new(&[1; 32]);
        let player_o = Pubkey::new(&[2; 32]);
        let mut g = Game::new(player_x, player_o);

        g.next_move(player_x, 0, 0).unwrap();
        g.next_move(player_o, 1, 0).unwrap();
        g.next_move(player_x, 2, 0).unwrap();
        g.next_move(player_o, 0, 1).unwrap();
        g.next_move(player_x, 1, 1).unwrap();
        g.next_move(player_o, 2, 1).unwrap();
        g.next_move(player_x, 1, 2).unwrap();
        g.next_move(player_o, 0, 2).unwrap();
        g.next_move(player_x, 2, 2).unwrap();
        assert_eq!(g.state, State::XWon);
    }

    #[test]
    pub fn draw() {
        /*
            X|O|O
            -+-+-
            O|O|X
            -+-+-
            X|X|O
        */

        let player_x = Pubkey::new(&[1; 32]);
        let player_o = Pubkey::new(&[2; 32]);
        let mut g = Game::new(player_x, player_o);

        g.next_move(player_x, 0, 0).unwrap();
        g.next_move(player_o, 1, 1).unwrap();
        g.next_move(player_x, 0, 2).unwrap();
        g.next_move(player_o, 0, 1).unwrap();
        g.next_move(player_x, 2, 1).unwrap();
        g.next_move(player_o, 1, 0).unwrap();
        g.next_move(player_x, 1, 2).unwrap();
        g.next_move(player_o, 2, 2).unwrap();
        g.next_move(player_x, 2, 0).unwrap();

        assert_eq!(g.state, State::Draw);
    }

    #[test]
    pub fn solo() {
        /*
            X|O|
            -+-+-
             | |
            -+-+-
             | |
        */

        let player_x = Pubkey::new(&[1; 32]);

        let mut g = Game::new(player_x, player_x);
        assert_eq!(g.state, State::XMove);
        g.next_move(player_x, 0, 0).unwrap();
        assert_eq!(g.state, State::OMove);
        g.next_move(player_x, 1, 0).unwrap();
        assert_eq!(g.state, State::XMove);
    }
}
