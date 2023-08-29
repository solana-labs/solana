use {
    super::*,
    spl_token_2022::{
        extension::interest_bearing_mint::{
            instruction::{InitializeInstructionData, InterestBearingMintInstruction},
            BasisPoints,
        },
        instruction::{decode_instruction_data, decode_instruction_type},
    },
};

pub(in crate::parse_token) fn parse_interest_bearing_mint_instruction(
    instruction_data: &[u8],
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match decode_instruction_type(instruction_data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?
    {
        InterestBearingMintInstruction::Initialize => {
            check_num_token_accounts(account_indexes, 1)?;
            let InitializeInstructionData {
                rate_authority,
                rate,
            } = *decode_instruction_data(instruction_data).map_err(|_| {
                ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
            })?;
            let rate_authority: Option<Pubkey> = rate_authority.into();
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeInterestBearingConfig".to_string(),
                info: json!({
                    "mint": account_keys[account_indexes[0] as usize].to_string(),
                    "rateAuthority": rate_authority.map(|pubkey| pubkey.to_string()),
                    "rate": i16::from(rate),
                }),
            })
        }
        InterestBearingMintInstruction::UpdateRate => {
            check_num_token_accounts(account_indexes, 2)?;
            let new_rate: BasisPoints =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "newRate": i16::from(new_rate),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                1,
                account_keys,
                account_indexes,
                "rateAuthority",
                "multisigRateAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "updateInterestBearingConfigRate".to_string(),
                info: value,
            })
        }
    }
}
