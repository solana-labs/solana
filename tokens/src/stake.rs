use {
    crate::{args::StakeArgs, commands::Error},
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::stake::state::StakeStateV2,
};

pub fn update_stake_args(client: &RpcClient, args: &mut Option<StakeArgs>) -> Result<(), Error> {
    if let Some(stake_args) = args {
        if let Some(sender_args) = &mut stake_args.sender_stake_args {
            let rent = client.get_minimum_balance_for_rent_exemption(StakeStateV2::size_of())?;
            sender_args.rent_exempt_reserve = Some(rent);
        }
    }
    Ok(())
}
