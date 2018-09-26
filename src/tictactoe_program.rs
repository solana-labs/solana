//! tic-tac-toe program

use account::Account;
use pubkey::Pubkey;
use serde_cbor;
use std;
use transaction::Transaction;

#[derive(Debug, PartialEq)]
pub enum Error {
    GameInProgress,
    InvalidArguments,
    InvalidMove,
    InvalidUserdata,
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

type Result<T> = std::result::Result<T, Error>;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
enum GridItem {
    Free,
    X,
    O,
}

impl Default for GridItem {
    fn default() -> GridItem {
        GridItem::Free
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
enum State {
    XMove,
    OMove,
    XWon,
    OWon,
    Draw,
}
impl Default for State {
    fn default() -> State {
        State::XMove
    }
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
struct Game {
    players: [Pubkey; 2],
    state: State,
    grid: [GridItem; 9],
}

impl Game {
    pub fn new(player1: Pubkey, player2: Pubkey) -> Game {
        let mut game = Game::default();
        game.players = [player1, player2];
        game
    }

    fn same(x_or_o: GridItem, triple: &[GridItem]) -> bool {
        triple.iter().all(|&i| i == x_or_o)
    }

    pub fn next_move(self: &mut Game, player: Pubkey, x: usize, y: usize) -> Result<()> {
        let index = self
            .players
            .iter()
            .position(|&p| p == player)
            .ok_or(Error::PlayerNotFound)?;

        let grid_index = y * 3 + x;
        if grid_index >= self.grid.len() || self.grid[grid_index] != GridItem::Free {
            return Err(Error::InvalidMove);
        }

        let (x_or_o, won_state) = match self.state {
            State::XMove => {
                if index != 0 {
                    return Err(Error::NotYourTurn)?;
                }
                self.state = State::OMove;
                (GridItem::X, State::XWon)
            }
            State::OMove => {
                if index != 1 {
                    return Err(Error::NotYourTurn)?;
                }
                self.state = State::XMove;
                (GridItem::O, State::OWon)
            }
            _ => {
                return Err(Error::NotYourTurn)?;
            }
        };
        self.grid[grid_index] = x_or_o;

        let winner =
            // Check rows
            Game::same(x_or_o, &self.grid[0..3])
            || Game::same(x_or_o, &self.grid[3..6])
            || Game::same(x_or_o, &self.grid[6..9])
            // Check columns
            || Game::same(x_or_o, &[self.grid[0], self.grid[3], self.grid[6]])
            || Game::same(x_or_o, &[self.grid[1], self.grid[4], self.grid[7]])
            || Game::same(x_or_o, &[self.grid[2], self.grid[5], self.grid[8]])
            // Check both diagonals
            || Game::same(x_or_o, &[self.grid[0], self.grid[4], self.grid[8]])
            || Game::same(x_or_o, &[self.grid[2], self.grid[4], self.grid[6]]);

        if winner {
            self.state = won_state;
        } else if self.grid.iter().all(|&p| p != GridItem::Free) {
            self.state = State::Draw;
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum Command {
    Init(Pubkey, Pubkey), // player1, player2
    Move(Pubkey, u8, u8), // player, x, y
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TicTacToeProgram {
    game: Option<Game>,
}

pub const TICTACTOE_PROGRAM_ID: [u8; 32] = [
    3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

impl TicTacToeProgram {
    fn deserialize(input: &[u8]) -> Result<TicTacToeProgram> {
        let len = input[0] as usize;

        if len == 0 {
            Ok(TicTacToeProgram::default())
        } else if input.len() < len + 1 {
            Err(Error::InvalidUserdata)
        } else {
            serde_cbor::from_slice(&input[1..len + 1]).map_err(|err| {
                error!("Unable to deserialize game: {:?}", err);
                Error::InvalidUserdata
            })
        }
    }

    fn dispatch_command(self: &mut TicTacToeProgram, cmd: &Command) -> Result<()> {
        info!("dispatch_command: cmd={:?}", cmd);
        info!("dispatch_command: account={:?}", self);
        match cmd {
            Command::Init(player_1, player_2) => {
                if let Some(_) = self.game {
                    Err(Error::GameInProgress)
                } else {
                    let game = Game::new(*player_1, *player_2);
                    self.game = Some(game);
                    Ok(())
                }
            }
            Command::Move(player, x, y) => {
                if let Some(ref mut game) = self.game {
                    game.next_move(*player, *x as usize, *y as usize)
                } else {
                    Err(Error::NoGame)
                }
            }
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
        output[1..self_serialized.len() + 1].clone_from_slice(&self_serialized);
        Ok(())
    }

    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == TICTACTOE_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&TICTACTOE_PROGRAM_ID)
    }

    pub fn process_transaction(tx: &Transaction, accounts: &mut [Account]) -> Result<()> {
        // accounts[1] must always be the Tic-tac-toe game state account
        if accounts.len() < 2 || !Self::check_id(&accounts[1].program_id) {
            error!("accounts[1] is not assigned to the TICTACTOE_PROGRAM_ID");
            return Err(Error::InvalidArguments);
        }
        if accounts[1].userdata.is_empty() {
            error!("accounts[1] userdata is empty");
            return Err(Error::InvalidArguments);
        }

        let mut program_state = Self::deserialize(&accounts[1].userdata)?;

        let command = serde_cbor::from_slice::<Command>(&tx.userdata).map_err(|err| {
            error!("{:?}", err);
            Error::InvalidUserdata
        })?;

        match command {
            Command::Init(_, _) => {
                // Init() must be signed by the game state account itself, who's private key is
                // known only to one of the players.
                if !Self::check_id(&accounts[0].program_id) {
                    error!("accounts[0] is not assigned to the TICTACTOE_PROGRAM_ID");
                    return Err(Error::InvalidArguments);
                }
            }
            Command::Move(player, _, _) => {
                // Move() must be signed by the player that is wanting to make the next move.
                if player != tx.keys[0] {
                    error!("keys[0]({})/player({}) mismatch", tx.keys[0], player);
                    return Err(Error::InvalidArguments);
                }
            }
        }

        program_state.dispatch_command(&command)?;
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

        let player_1 = Pubkey::new(&[1; 32]);
        let player_2 = Pubkey::new(&[2; 32]);
        account
            .dispatch_command(&Command::Init(player_1, player_2))
            .unwrap();

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

        let player_1 = Pubkey::new(&[1; 32]);
        let player_2 = Pubkey::new(&[2; 32]);

        let mut g = Game::new(player_1, player_2);
        assert_eq!(g.state, State::XMove);

        g.next_move(player_1, 0, 0).unwrap();
        assert_eq!(g.state, State::OMove);
        g.next_move(player_2, 1, 0).unwrap();
        assert_eq!(g.state, State::XMove);
        g.next_move(player_1, 0, 1).unwrap();
        assert_eq!(g.state, State::OMove);
        g.next_move(player_2, 1, 1).unwrap();
        assert_eq!(g.state, State::XMove);
        g.next_move(player_1, 0, 2).unwrap();
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

        let player_1 = Pubkey::new(&[1; 32]);
        let player_2 = Pubkey::new(&[2; 32]);
        let mut g = Game::new(player_1, player_2);

        g.next_move(player_1, 0, 0).unwrap();
        g.next_move(player_2, 1, 0).unwrap();
        g.next_move(player_1, 2, 0).unwrap();
        g.next_move(player_2, 0, 1).unwrap();
        g.next_move(player_1, 1, 1).unwrap();
        g.next_move(player_2, 2, 1).unwrap();
        g.next_move(player_1, 0, 2).unwrap();
        assert_eq!(g.state, State::XWon);

        assert_eq!(g.next_move(player_2, 1, 2), Err(Error::NotYourTurn));
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

        let player_1 = Pubkey::new(&[1; 32]);
        let player_2 = Pubkey::new(&[2; 32]);
        let mut g = Game::new(player_1, player_2);

        g.next_move(player_1, 0, 0).unwrap();
        g.next_move(player_2, 0, 2).unwrap();
        g.next_move(player_1, 1, 0).unwrap();
        g.next_move(player_2, 1, 2).unwrap();
        g.next_move(player_1, 0, 1).unwrap();
        g.next_move(player_2, 2, 2).unwrap();
        assert_eq!(g.state, State::OWon);

        assert!(g.next_move(player_1, 1, 2).is_err());
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

        let player_1 = Pubkey::new(&[1; 32]);
        let player_2 = Pubkey::new(&[2; 32]);
        let mut g = Game::new(player_1, player_2);

        g.next_move(player_1, 0, 0).unwrap();
        g.next_move(player_2, 1, 0).unwrap();
        g.next_move(player_1, 2, 0).unwrap();
        g.next_move(player_2, 0, 1).unwrap();
        g.next_move(player_1, 1, 1).unwrap();
        g.next_move(player_2, 2, 1).unwrap();
        g.next_move(player_1, 1, 2).unwrap();
        g.next_move(player_2, 0, 2).unwrap();
        g.next_move(player_1, 2, 2).unwrap();
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

        let player_1 = Pubkey::new(&[1; 32]);
        let player_2 = Pubkey::new(&[2; 32]);
        let mut g = Game::new(player_1, player_2);

        g.next_move(player_1, 0, 0).unwrap();
        g.next_move(player_2, 1, 1).unwrap();
        g.next_move(player_1, 0, 2).unwrap();
        g.next_move(player_2, 0, 1).unwrap();
        g.next_move(player_1, 2, 1).unwrap();
        g.next_move(player_2, 1, 0).unwrap();
        g.next_move(player_1, 1, 2).unwrap();
        g.next_move(player_2, 2, 2).unwrap();
        g.next_move(player_1, 2, 0).unwrap();

        assert_eq!(g.state, State::Draw);
    }
}
