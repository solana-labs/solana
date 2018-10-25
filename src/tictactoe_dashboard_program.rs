//! tic-tac-toe dashboard program

use serde_cbor;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use tictactoe_program::{Error, Game, Result, State, TicTacToeProgram};
use transaction::Transaction;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TicTacToeDashboardProgram {
    pending: Pubkey,        // Latest pending game
    completed: Vec<Pubkey>, // Last N completed games (0 is the latest)
    total: usize,           // Total number of completed games
}

pub const TICTACTOE_DASHBOARD_PROGRAM_ID: [u8; 32] = [
    4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

impl TicTacToeDashboardProgram {
    fn deserialize(input: &[u8]) -> Result<TicTacToeDashboardProgram> {
        if input.len() < 2 {
            Err(Error::InvalidUserdata)?;
        }
        let len = input[0] as usize + (0x100 * input[1] as usize);
        if len == 0 {
            Ok(TicTacToeDashboardProgram::default())
        } else if input.len() < len + 2 {
            Err(Error::InvalidUserdata)
        } else {
            serde_cbor::from_slice(&input[2..(2 + len)]).map_err(|err| {
                error!("Unable to deserialize: {:?}", err);
                Error::InvalidUserdata
            })
        }
    }

    fn update(
        self: &mut TicTacToeDashboardProgram,
        game_pubkey: &Pubkey,
        game: &Game,
    ) -> Result<()> {
        match game.state {
            State::Waiting => {
                self.pending = *game_pubkey;
            }
            State::XMove | State::OMove => {
                // Nothing to do.  In progress games are not managed by the dashboard
            }
            State::XWon | State::OWon | State::Draw => {
                if !self.completed.iter().any(|pubkey| pubkey == game_pubkey) {
                    // TODO: Once the PoH height is exposed to programs, it could be used to ensure
                    //       that old games are not being re-added and causing |total| to increment
                    //       incorrectly.
                    self.total += 1;
                    self.completed.insert(0, *game_pubkey);

                    // Only track the last N completed games to
                    // avoid overrunning Account userdata
                    if self.completed.len() > 5 {
                        self.completed.pop();
                    }
                }
            }
        };

        Ok(())
    }

    fn serialize(self: &TicTacToeDashboardProgram, output: &mut [u8]) -> Result<()> {
        let self_serialized = serde_cbor::to_vec(self).unwrap();

        if output.len() < 2 + self_serialized.len() {
            warn!(
                "{} bytes required to serialize but only have {} bytes",
                self_serialized.len() + 2,
                output.len()
            );
            return Err(Error::UserdataTooSmall);
        }

        assert!(self_serialized.len() <= 0xFFFF);
        output[0] = (self_serialized.len() & 0xFF) as u8;
        output[1] = (self_serialized.len() >> 8) as u8;
        output[2..(2 + self_serialized.len())].clone_from_slice(&self_serialized);
        Ok(())
    }

    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == TICTACTOE_DASHBOARD_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&TICTACTOE_DASHBOARD_PROGRAM_ID)
    }

    pub fn process_transaction(
        tx: &Transaction,
        pix: usize,
        accounts: &mut [&mut Account],
    ) -> Result<()> {
        info!("process_transaction: {:?}", tx);
        // accounts[0] doesn't matter, anybody can cause a dashboard update
        // accounts[1] must be a Dashboard account
        // accounts[2] must be a Game account
        if accounts.len() != 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArguments)?;
        }
        if !Self::check_id(&accounts[1].program_id) {
            error!("accounts[1] is not a TICTACTOE_DASHBOARD_PROGRAM_ID");
            Err(Error::InvalidArguments)?;
        }
        if accounts[1].userdata.is_empty() {
            error!("accounts[1] userdata is empty");
            Err(Error::InvalidArguments)?;
        }

        let mut dashboard = Self::deserialize(&accounts[1].userdata)?;

        if !TicTacToeProgram::check_id(&accounts[2].program_id) {
            error!("accounts[2] is not a TICTACTOE_PROGRAM_ID");
            Err(Error::InvalidArguments)?;
        }
        let ttt = TicTacToeProgram::deserialize(&accounts[2].userdata)?;

        match ttt.game {
            None => Err(Error::NoGame),
            Some(game) => dashboard.update(tx.key(pix, 2).unwrap(), &game),
        }?;

        dashboard.serialize(&mut accounts[1].userdata)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    pub fn serde() {
        let mut dashboard1 = TicTacToeDashboardProgram::default();
        dashboard1.total = 1234567890;
        dashboard1.pending = Pubkey::new(&[100; 32]);
        for i in 1..5 {
            dashboard1.completed.push(Pubkey::new(&[100 + i; 32]));
        }

        let mut userdata = vec![0xff; 512];
        dashboard1.serialize(&mut userdata).unwrap();

        let dashboard2 = TicTacToeDashboardProgram::deserialize(&userdata).unwrap();
        assert_eq!(dashboard1, dashboard2);
    }

    #[test]
    pub fn serde_userdata_too_small() {
        let dashboard = TicTacToeDashboardProgram::default();
        let mut userdata = vec![0xff; 1];
        assert_eq!(
            dashboard.serialize(&mut userdata),
            Err(Error::UserdataTooSmall)
        );

        let err = TicTacToeDashboardProgram::deserialize(&userdata);
        assert!(err.is_err());
        assert_eq!(err.err().unwrap(), Error::InvalidUserdata);
    }

    // TODO: add tests for business logic
}
