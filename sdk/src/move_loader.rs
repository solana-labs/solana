use crate::{native_loader, native_program_info, pubkey::Pubkey};

crate::declare_id!("MoveLdr111111111111111111111111111111111111");

pub fn solana_move_loader_program() -> (native_loader::Info, Pubkey) {
    (
        native_program_info!("solana_move_loader_program".to_string()),
        id(),
    )
}
