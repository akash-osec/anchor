use {
    crate::{
        context::{Bumps, Context},
        cursor::AccountCursor,
        loader::AccountLoader,
    },
    core::mem::MaybeUninit,
    pinocchio::{account::AccountView, address::Address},
    solana_program_error::ProgramError,
};

/// Stack storage for the declared account views of one instruction. Generated
/// `Accounts` implementations choose the concrete array length at compile
/// time, so dispatch does not reserve space for all 256 transaction accounts.
#[doc(hidden)]
pub trait AccountViewStorage {
    fn new() -> Self
    where
        Self: Sized;

    fn as_mut_slice(&mut self) -> &mut [MaybeUninit<AccountView>];
}

impl<const N: usize> AccountViewStorage for [MaybeUninit<AccountView>; N] {
    #[inline(always)]
    fn new() -> Self {
        [const { MaybeUninit::uninit() }; N]
    }

    #[inline(always)]
    fn as_mut_slice(&mut self) -> &mut [MaybeUninit<AccountView>] {
        self
    }
}

/// Trait that `#[derive(Accounts)]` implements on account structs.
///
/// `try_accounts` receives a pre-walked `&[AccountView]` slice (from a
/// single `walk_n_into(HEADER_SIZE)` in [`run_handler`]) rather than the raw
/// cursor. This lets `Nested<Inner>` fields pass a sub-slice to
/// `Inner::validate_accounts` without re-walking the cursor or fighting
/// borrow-checker splits.
///
/// `HEADER_SIZE` is computed recursively at compile time: 1 per direct
/// field, `+ <Inner as TryAccounts>::HEADER_SIZE` per `Nested<Inner>`.
pub trait TryAccounts: Bumps + Sized {
    const HEADER_SIZE: usize;

    /// Parsed instruction args carried alongside validated accounts.
    /// Accounts structs without `#[instruction(...)]` use `()`.
    type IxArgs<'ix>;

    /// Compile-time-sized storage for the flattened declared account views.
    /// The derive supplies an array sized to `HEADER_SIZE`, avoiding a
    /// transaction-wide 256-account stack reservation.
    type ViewStorage: AccountViewStorage;
    fn try_accounts<'ix>(
        program_id: &Address,
        views: &[AccountView],
        ix_data: &'ix [u8],
    ) -> Result<(Self, Self::Bumps, Self::IxArgs<'ix>), ProgramError>;

    /// Validation-only account construction path used by the dispatcher so it
    /// can run `update(...)` hooks after access-control. Manual callers should
    /// continue to use [`Self::try_accounts`], which preserves the historical
    /// "validate + update" behavior by default.
    #[doc(hidden)]
    #[inline(always)]
    fn validate_accounts<'ix>(
        program_id: &Address,
        views: &[AccountView],
        ix_data: &'ix [u8],
    ) -> Result<(Self, Self::Bumps, Self::IxArgs<'ix>), ProgramError> {
        Self::try_accounts(program_id, views, ix_data)
    }

    fn update_accounts(&mut self) -> Result<(), ProgramError>;

    fn exit_accounts<'ix>(&mut self, ix_data: &'ix [u8]) -> Result<(), ProgramError>;
}

/// Run a handler inside a fully-constructed [`Context`].
///
/// Walks all declared accounts in one `walk_n_into(HEADER_SIZE)` call, then
/// passes the views slice to `T::validate_accounts` for per-field loading and
/// constraint checking. The residual cursor (past the declared accounts) is
/// handed to `Context` for lazy `remaining_accounts()` access.
#[inline(always)]
pub fn run_handler<'a, T: TryAccounts, R>(
    program_id: &'a Address,
    cursor: &'a mut AccountCursor,
    ix_data: &'a [u8],
    num_accounts: usize,
    handler: impl FnOnce(&mut Context<'a, T>, T::IxArgs<'a>) -> Result<R, ProgramError>,
) -> Result<R, ProgramError> {
    if num_accounts < T::HEADER_SIZE {
        return Err(crate::ErrorCode::AccountNotEnoughKeys.into());
    }
    let (ctx_accounts, bumps, ix_args) = {
        let mut loader = AccountLoader::new(cursor);
        let mut view_storage = T::ViewStorage::new();
        let views = unsafe { loader.walk_n_into(T::HEADER_SIZE, view_storage.as_mut_slice()) };
        T::validate_accounts(program_id, views, ix_data)?
    };
    const _: () = assert!(pinocchio::MAX_TX_ACCOUNTS <= u8::MAX as usize);
    let remaining_num = num_accounts - T::HEADER_SIZE;
    let mut ctx = Context::new(program_id, ctx_accounts, bumps, cursor, remaining_num);
    let result = handler(&mut ctx, ix_args)?;
    ctx.accounts.exit_accounts(ix_data)?;
    Ok(result)
}
