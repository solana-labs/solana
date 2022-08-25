use solana_sdk::{compute_budget::ComputeBudgetInstruction, instruction::Instruction};

pub trait WithComputeUnitPrice {
    fn with_compute_unit_price(self, compute_unit_price: Option<&u64>) -> Self;
}

impl WithComputeUnitPrice for Vec<Instruction> {
    fn with_compute_unit_price(mut self, compute_unit_price: Option<&u64>) -> Self {
        if let Some(compute_unit_price) = compute_unit_price {
            self.push(ComputeBudgetInstruction::set_compute_unit_price(
                *compute_unit_price,
            ));
        }
        self
    }
}
