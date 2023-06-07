use {
    super::VersionedMessage,
    crate::{instruction::CompiledInstruction, pubkey::Pubkey, sanitize::SanitizeError},
};

/// Wraps a sanitized `VersionedMessage` to provide a safe API
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SanitizedVersionedMessage {
    pub message: VersionedMessage,
}

impl TryFrom<VersionedMessage> for SanitizedVersionedMessage {
    type Error = SanitizeError;
    fn try_from(message: VersionedMessage) -> Result<Self, Self::Error> {
        Self::try_new(message)
    }
}

impl SanitizedVersionedMessage {
    pub fn try_new(message: VersionedMessage) -> Result<Self, SanitizeError> {
        message.sanitize()?;
        Ok(Self { message })
    }

    /// Program instructions that will be executed in sequence and committed in
    /// one atomic transaction if all succeed.
    pub fn instructions(&self) -> &[CompiledInstruction] {
        self.message.instructions()
    }

    /// Program instructions iterator which includes each instruction's program
    /// id.
    pub fn program_instructions_iter(
        &self,
    ) -> impl Iterator<Item = (&Pubkey, &CompiledInstruction)> {
        self.message.instructions().iter().map(move |ix| {
            (
                self.message
                    .static_account_keys()
                    .get(usize::from(ix.program_id_index))
                    .expect("program id index is sanitized"),
                ix,
            )
        })
    }
}
