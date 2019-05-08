use crate::message::Message;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeeCalculator {
    pub lamports_per_signature: u64,
}

impl FeeCalculator {
    pub fn new(lamports_per_signature: u64) -> Self {
        Self {
            lamports_per_signature,
        }
    }

    pub fn calculate_fee(&self, message: &Message) -> u64 {
        self.lamports_per_signature * u64::from(message.num_required_signatures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pubkey::Pubkey;
    use crate::system_instruction;

    #[test]
    fn test_fee_calculator_calculate_fee() {
        // Default: no fee.
        let message = Message::new(vec![]);
        assert_eq!(FeeCalculator::default().calculate_fee(&message), 0);

        // No signature, no fee.
        assert_eq!(FeeCalculator::new(1).calculate_fee(&message), 0);

        // One signature, a fee.
        let pubkey0 = Pubkey::new(&[0; 32]);
        let pubkey1 = Pubkey::new(&[1; 32]);
        let ix0 = system_instruction::transfer(&pubkey0, &pubkey1, 1);
        let message = Message::new(vec![ix0]);
        assert_eq!(FeeCalculator::new(2).calculate_fee(&message), 2);

        // Two signatures, double the fee.
        let ix0 = system_instruction::transfer(&pubkey0, &pubkey1, 1);
        let ix1 = system_instruction::transfer(&pubkey1, &pubkey0, 1);
        let message = Message::new(vec![ix0, ix1]);
        assert_eq!(FeeCalculator::new(2).calculate_fee(&message), 4);
    }
}
