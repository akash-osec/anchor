use {
    alloc::vec::Vec, pinocchio::address::Address, solana_program_error::ProgramError,
    solana_pubkey::Pubkey,
};

use anchor_lang::{programs::Token2022, require_eq, Id};

#[inline]
pub(crate) fn validate_token_2022_program(program: &Address) -> Result<(), ProgramError> {
    require_eq!(*program, Token2022::id(), ProgramError::IncorrectProgramId);
    Ok(())
}

pub(crate) fn pubkey_refs(pubkeys: &[Pubkey]) -> Vec<&Pubkey> {
    pubkeys.iter().collect()
}
