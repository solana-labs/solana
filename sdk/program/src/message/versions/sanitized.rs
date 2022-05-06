use {
    super::VersionedMessage,
    crate::{instruction::CompiledInstruction, pubkey::Pubkey, sanitize::SanitizeError},
};

/// Wraps a sanitized `VersionedMessage` to provide a safe API
pub struct SanitizedVersionedMessage<'a> {
    message: &'a VersionedMessage,
}

impl<'a> TryFrom<&'a VersionedMessage> for SanitizedVersionedMessage<'a> {
    type Error = SanitizeError;
    fn try_from(message: &'a VersionedMessage) -> Result<Self, Self::Error> {
        message.sanitize(true /* require_static_program_ids */)?;
        Ok(Self { message })
    }
}

impl SanitizedVersionedMessage<'_> {
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
