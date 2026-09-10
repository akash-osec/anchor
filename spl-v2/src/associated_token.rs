//! Associated Token Account address derivation and CPI helpers.
//!
//! Users can validate ATA accounts via `associated_token::*` constraints:
//! ```ignore
//! #[account(
//!     associated_token::mint = mint,
//!     associated_token::authority = authority,
//!     associated_token::token_program = token_program,
//! )]
//! pub vault: Account<TokenAccount>,
//! ```

extern crate alloc;

use anchor_lang::require;
use {
    anchor_lang::{programs::Token, CpiContext, CpiHandle, CpiHandleMut, Id, ToCpiAccounts},
    solana_address::Address,
    solana_program_error::ProgramError,
};

pub use anchor_lang::programs::AssociatedToken;

pub const ID: Address = anchor_lang::address!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Derive the associated token account address for a given wallet and mint.
pub fn get_associated_token_address(wallet: &Address, mint: &Address) -> Address {
    get_associated_token_address_with_program_id(wallet, mint, &Token::id())
}

/// Derive the associated token account address for a given wallet, mint, and token program.
pub fn get_associated_token_address_with_program_id(
    wallet: &Address,
    mint: &Address,
    token_program_id: &Address,
) -> Address {
    let seeds: &[&[u8]] = &[wallet.as_ref(), token_program_id.as_ref(), mint.as_ref()];
    let (addr, _bump) = Address::find_program_address(seeds, &ID);
    addr
}

#[derive(ToCpiAccounts)]
pub struct Create<'a> {
    #[signer]
    pub payer: CpiHandleMut<'a>,
    pub associated_token: CpiHandleMut<'a>,
    pub authority: CpiHandle<'a>,
    pub mint: CpiHandle<'a>,
    pub system_program: CpiHandle<'a>,
    pub token_program: CpiHandle<'a>,
}

pub type CreateIdempotent<'a> = Create<'a>;

pub fn create<'a>(ctx: CpiContext<'a, Create<'a>>) -> Result<(), ProgramError> {
    require!(
        anchor_lang::address_eq(ctx.program, &AssociatedToken::id()),
        ProgramError::IncorrectProgramId
    );
    require!(
        anchor_lang::address_eq(
            ctx.accounts.system_program.address(),
            &anchor_lang::programs::System::id(),
        ),
        ProgramError::IncorrectProgramId
    );
    crate::token_shared::validate_token_interface_program(ctx.accounts.token_program.address())?;
    ctx.invoke(&[0])
}

pub fn create_idempotent<'a>(
    ctx: CpiContext<'a, CreateIdempotent<'a>>,
) -> Result<(), ProgramError> {
    require!(
        anchor_lang::address_eq(ctx.program, &AssociatedToken::id()),
        ProgramError::IncorrectProgramId
    );
    require!(
        anchor_lang::address_eq(
            ctx.accounts.system_program.address(),
            &anchor_lang::programs::System::id(),
        ),
        ProgramError::IncorrectProgramId
    );
    crate::token_shared::validate_token_interface_program(ctx.accounts.token_program.address())?;
    ctx.invoke(&[1])
}

#[cfg(test)]
mod tests {
    use {
        super::{create, create_idempotent, Create},
        anchor_lang::{testing::AccountBuffer, CpiContext, CpiHandle, CpiHandleMut},
        pinocchio::account::AccountView,
        pinocchio::address::Address,
        solana_program_error::ProgramError,
    };

    fn context<'a>(
        program: &'a Address,
        payer: &'a mut AccountView,
        associated_token: &'a mut AccountView,
        authority: &'a AccountView,
        mint: &'a AccountView,
        system_program: &'a AccountView,
        token_program: &'a AccountView,
    ) -> CpiContext<'a, Create<'a>> {
        CpiContext::new(
            program,
            Create {
                payer: CpiHandleMut::writable(payer),
                associated_token: CpiHandleMut::writable(associated_token),
                authority: CpiHandle::readonly(authority),
                mint: CpiHandle::readonly(mint),
                system_program: CpiHandle::readonly(system_program),
                token_program: CpiHandle::readonly(token_program),
            },
        )
    }

    fn accounts() -> (
        AccountBuffer<128>,
        AccountBuffer<128>,
        AccountBuffer<128>,
        AccountBuffer<128>,
        AccountBuffer<128>,
        AccountBuffer<128>,
    ) {
        let payer = AccountBuffer::new();
        let associated_token = AccountBuffer::new();
        let authority = AccountBuffer::new();
        let mint = AccountBuffer::new();
        let system_program = AccountBuffer::new();
        let token_program = AccountBuffer::new();
        for (account, address) in [
            (&payer, [1; 32]),
            (&associated_token, [2; 32]),
            (&authority, [3; 32]),
            (&mint, [4; 32]),
            (&system_program, [5; 32]),
            (&token_program, [6; 32]),
        ] {
            account.init(address, [0; 32], 0, false, true, false);
        }
        (
            payer,
            associated_token,
            authority,
            mint,
            system_program,
            token_program,
        )
    }

    #[test]
    fn create_rejects_noncanonical_targets_without_guardrails() {
        let (payer, associated_token, authority, mint, system_program, token_program) = accounts();
        let mut payer_view = unsafe { payer.view() };
        let mut associated_token_view = unsafe { associated_token.view() };
        let authority_view = unsafe { authority.view() };
        let mint_view = unsafe { mint.view() };
        let system_program_view = unsafe { system_program.view() };
        let token_program_view = unsafe { token_program.view() };
        let fake_program = Address::new_from_array([9; 32]);

        assert_eq!(
            create(context(
                &fake_program,
                &mut payer_view,
                &mut associated_token_view,
                &authority_view,
                &mint_view,
                &system_program_view,
                &token_program_view,
            )),
            Err(ProgramError::IncorrectProgramId)
        );
    }

    #[test]
    fn create_idempotent_rejects_noncanonical_targets_without_guardrails() {
        let (payer, associated_token, authority, mint, system_program, token_program) = accounts();
        let mut payer_view = unsafe { payer.view() };
        let mut associated_token_view = unsafe { associated_token.view() };
        let authority_view = unsafe { authority.view() };
        let mint_view = unsafe { mint.view() };
        let system_program_view = unsafe { system_program.view() };
        let token_program_view = unsafe { token_program.view() };
        let fake_program = Address::new_from_array([9; 32]);

        assert_eq!(
            create_idempotent(context(
                &fake_program,
                &mut payer_view,
                &mut associated_token_view,
                &authority_view,
                &mint_view,
                &system_program_view,
                &token_program_view,
            )),
            Err(ProgramError::IncorrectProgramId)
        );
    }
}
