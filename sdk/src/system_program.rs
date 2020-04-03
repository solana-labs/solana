use crate::{native_loader, native_program_info, pubkey::Pubkey};

crate::declare_id!("11111111111111111111111111111111");

pub fn solana_system_program() -> (native_loader::Info, Pubkey) {
    (
        native_program_info!("solana_system_program".to_string()),
        id(),
    )
}
