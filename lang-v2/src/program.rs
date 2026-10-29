pub use pinocchio::cpi::{set_return_data, MAX_RETURN_DATA};
use {
    crate::{address_eq, require, Address, CpiHandle, CpiInstruction},
    core::{mem::MaybeUninit, slice::from_raw_parts},
    pinocchio::{
        cpi::CpiAccount,
        instruction::{InstructionAccount, InstructionView},
    },
    solana_program_error::{ProgramError, ProgramResult},
};

/// Borrowed fixed-capacity storage accepted by the public CPI helpers.
///
/// The capacity is part of the call's type, allowing callers to choose the
/// amount of inline storage required by a particular CPI without introducing
/// a global account limit. Arrays remain supported for small hand-written
/// calls, while generated code uses [`crate::CpiHandleBuffer`].
pub trait CpiHandleSource<'a, const N: usize> {
    fn as_slice(&self) -> &[CpiHandle<'a>];
}

impl<'a, const N: usize> CpiHandleSource<'a, N> for [CpiHandle<'a>; N] {
    #[inline(always)]
    fn as_slice(&self) -> &[CpiHandle<'a>] {
        self
    }
}

impl<'a, const N: usize> CpiHandleSource<'a, N> for crate::CpiHandleBuffer<'a, N> {
    #[inline(always)]
    fn as_slice(&self) -> &[CpiHandle<'a>] {
        self.as_slice()
    }
}

/// Get the return data from an invoked program.
///
/// This preserves the v1-shaped `program::get_return_data()` return value while
/// sourcing the data from Pinocchio's `AccountView`-native CPI module.
pub fn get_return_data() -> Option<(crate::Address, pinocchio::cpi::ReturnData)> {
    pinocchio::cpi::get_return_data().map(|data| (*data.program_id(), data))
}

/// Invoke a cross-program instruction using Anchor v2 CPI handles.
///
/// Unlike the legacy `AccountInfo` API, callers pass [`CpiHandle`]s obtained
/// from `cpi_handle()` / `cpi_handle_mut()`, so CPI account lifetimes remain
/// tied to Rust borrows of the caller's typed accounts.
///
/// Optional program-id sentinel metas from `Option::None` CPI slots are not
/// inferred from address alone — use
/// [`invoke_with_optional_sentinels`] / [`invoke_signed_with_optional_sentinels`]
/// (or [`crate::CpiContext`]) so required readonly program-id accounts still
/// require matching handles.
pub fn invoke<'a, H, I, const N: usize>(instruction: &I, account_handles: &H) -> ProgramResult
where
    H: CpiHandleSource<'a, N>,
    I: CpiInstruction + ?Sized,
{
    invoke_signed::<H, I, N>(instruction, account_handles, &[])
}

/// Like [`invoke`], but marks instruction-account indices that are intentional
/// optional `None` program-id sentinels (parallel to
/// [`crate::ToCpiAccountsWithCapacity::optional_account_sentinel_flags_with_capacity`]).
pub fn invoke_with_optional_sentinels<'a, H, I, F, const N: usize>(
    instruction: &I,
    account_handles: &H,
    optional_sentinel_flags: &F,
) -> ProgramResult
where
    H: CpiHandleSource<'a, N>,
    I: CpiInstruction + ?Sized,
    F: crate::OptionalSentinelFlags + ?Sized,
{
    invoke_signed_with_optional_sentinels::<H, I, F, N>(
        instruction,
        account_handles,
        &[],
        optional_sentinel_flags,
    )
}

/// Invoke a cross-program instruction with PDA signer seeds using Anchor v2
/// CPI handles.
pub fn invoke_signed<'a, 'seeds, H, I, const N: usize>(
    instruction: &I,
    account_handles: &H,
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
) -> ProgramResult
where
    H: CpiHandleSource<'a, N>,
    I: CpiInstruction + ?Sized,
{
    invoke_signed_with_optional_sentinels::<H, I, _, N>(
        instruction,
        account_handles,
        signer_seeds,
        &[],
    )
}

/// Like [`invoke_signed`], with an explicit optional-sentinel mask.
pub fn invoke_signed_with_optional_sentinels<'a, 'seeds, H, I, F, const N: usize>(
    instruction: &I,
    account_handles: &H,
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
    optional_sentinel_flags: &F,
) -> ProgramResult
where
    H: CpiHandleSource<'a, N>,
    I: CpiInstruction + ?Sized,
    F: crate::OptionalSentinelFlags + ?Sized,
{
    invoke_signed_with_capacity::<N, I, F>(
        instruction,
        account_handles.as_slice(),
        signer_seeds,
        optional_sentinel_flags,
    )
}

pub(crate) fn invoke_signed_with_capacity<
    'a,
    'seeds,
    const N: usize,
    I: CpiInstruction + ?Sized,
    F: crate::OptionalSentinelFlags + ?Sized,
>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
    optional_sentinel_flags: &F,
) -> ProgramResult {
    instruction.with_instruction_accounts(|instruction_accounts| {
        validate_instruction_accounts::<N, F>(
            instruction_accounts,
            instruction.program_id(),
            account_handles,
            optional_sentinel_flags,
            signer_seeds.is_empty(),
        )
    })?;

    // SAFETY: Validation above proves every non-sentinel instruction account
    // has a matching handle, writable metas use writable handles, and
    // AccountView borrow state permits the CPI.
    unsafe {
        invoke_signed_unchecked_with_capacity::<N, I, F>(
            instruction,
            account_handles,
            signer_seeds,
            optional_sentinel_flags,
        )
    }
}

/// Invoke a cross-program instruction without borrow validation.
///
/// # Safety
///
/// The caller must ensure no live Rust references or stale `AccountView` data
/// borrows can be invalidated by the callee. Prefer [`invoke`] unless this is
/// being used through a higher-level API that already enforces those lifetimes.
pub unsafe fn invoke_unchecked<'a, H, I, const N: usize>(
    instruction: &I,
    account_handles: &H,
) -> ProgramResult
where
    H: CpiHandleSource<'a, N>,
    I: CpiInstruction + ?Sized,
{
    unsafe { invoke_signed_unchecked::<H, I, N>(instruction, account_handles, &[]) }
}

/// Invoke a cross-program instruction with PDA signer seeds, without borrow
/// validation.
///
/// # Safety
///
/// The caller must ensure no live Rust references or stale `AccountView` data
/// borrows can be invalidated by the callee. Prefer [`invoke_signed`] unless
/// this is being used through a higher-level API that already enforces those
/// lifetimes.
pub unsafe fn invoke_signed_unchecked<'a, 'seeds, H, I, const N: usize>(
    instruction: &I,
    account_handles: &H,
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
) -> ProgramResult
where
    H: CpiHandleSource<'a, N>,
    I: CpiInstruction + ?Sized,
{
    unsafe {
        invoke_signed_unchecked_with_optional_sentinels::<H, I, _, N>(
            instruction,
            account_handles,
            signer_seeds,
            &[],
        )
    }
}

/// Like [`invoke_signed_unchecked`], with an explicit optional-sentinel mask.
///
/// # Safety
///
/// Same as [`invoke_signed_unchecked`].
pub unsafe fn invoke_signed_unchecked_with_optional_sentinels<
    'a,
    'seeds,
    H,
    I: CpiInstruction + ?Sized,
    F,
    const N: usize,
>(
    instruction: &I,
    account_handles: &H,
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
    optional_sentinel_flags: &F,
) -> ProgramResult
where
    H: CpiHandleSource<'a, N>,
    F: crate::OptionalSentinelFlags + ?Sized,
{
    unsafe {
        invoke_signed_unchecked_with_capacity::<N, I, F>(
            instruction,
            account_handles.as_slice(),
            signer_seeds,
            optional_sentinel_flags,
        )
    }
}

unsafe fn invoke_signed_unchecked_with_capacity<
    'a,
    'seeds,
    const N: usize,
    I: CpiInstruction + ?Sized,
    F: crate::OptionalSentinelFlags + ?Sized,
>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
    optional_sentinel_flags: &F,
) -> ProgramResult {
    instruction.with_instruction_accounts(|instruction_accounts| {
        let cpi_account_count = required_cpi_account_count::<N, F>(
            instruction_accounts,
            instruction.program_id(),
            account_handles,
            optional_sentinel_flags,
        )?;
        let instruction_view = InstructionView {
            program_id: instruction.program_id(),
            accounts: instruction_accounts,
            data: instruction.data(),
        };
        let signers = signers::<N>(signer_seeds)?;
        let _borrow_guards = crate::enter_cpi::<N>(account_handles)?;
        let cpi_accounts = cpi_accounts::<N>(account_handles)?;

        // SAFETY:
        // - `cpi_accounts` was fully initialized from the provided handles.
        // - This function's caller upholds the unchecked CPI aliasing contract.
        unsafe {
            pinocchio::cpi::invoke_signed_unchecked(
                &instruction_view,
                from_raw_parts(
                    cpi_accounts.as_ptr() as *const CpiAccount,
                    cpi_account_count,
                ),
                &signers,
            );
        }

        Ok(())
    })
}

pub(crate) fn validate_fixed_instruction_accounts<'a, const N: usize>(
    instruction_accounts: &[InstructionAccount<'a>; N],
    handles: &[CpiHandle<'a>; N],
    enforce_signers: bool,
) -> ProgramResult {
    for (account, handle) in instruction_accounts.iter().zip(handles) {
        require!(
            address_eq(account.address, handle.address())
                && (!account.is_writable || handle.is_writable()),
            ProgramError::InvalidArgument
        );

        if handle.requires_borrow_check() {
            if account.is_writable {
                handle.account_view().check_borrow_mut()?;
            } else {
                handle.account_view().check_borrow()?;
            }
        }

        if enforce_signers && account.is_signer {
            require!(
                handle.account_view().is_signer(),
                ProgramError::MissingRequiredSignature
            );
        }
    }

    Ok(())
}

pub(crate) fn validate_instruction_accounts<
    'a,
    const N: usize,
    F: crate::OptionalSentinelFlags + ?Sized,
>(
    instruction_accounts: &[InstructionAccount<'a>],
    program_id: &Address,
    account_handles: &[CpiHandle<'a>],
    optional_sentinel_flags: &F,
    enforce_signers: bool,
) -> ProgramResult {
    walk_instruction_account_bindings::<N, F, _>(
        instruction_accounts,
        program_id,
        account_handles,
        optional_sentinel_flags,
        |account, handle| {
            if account.is_writable {
                if handle.requires_borrow_check() {
                    handle.account_view().check_borrow_mut()?;
                }
            } else if handle.requires_borrow_check() {
                handle.account_view().check_borrow()?;
            }

            if enforce_signers && account.is_signer {
                require!(
                    handle.account_view().is_signer(),
                    ProgramError::MissingRequiredSignature
                );
            }
            Ok(())
        },
    )
    .map(|_| ())
}

fn is_optional_instruction_account_sentinel_candidate(
    program_id: &Address,
    account: &InstructionAccount<'_>,
) -> bool {
    !account.is_writable && !account.is_signer && address_eq(account.address, program_id)
}

fn is_skippable_optional_sentinel<F: crate::OptionalSentinelFlags + ?Sized>(
    program_id: &Address,
    account: &InstructionAccount<'_>,
    optional_sentinel_flags: &F,
    instruction_index: usize,
) -> bool {
    optional_sentinel_flags.get(instruction_index)
        && is_optional_instruction_account_sentinel_candidate(program_id, account)
}

fn required_cpi_account_count<'a, const N: usize, F: crate::OptionalSentinelFlags + ?Sized>(
    instruction_accounts: &[InstructionAccount<'a>],
    program_id: &Address,
    account_handles: &[CpiHandle<'a>],
    optional_sentinel_flags: &F,
) -> Result<usize, ProgramError> {
    walk_instruction_account_bindings::<N, F, _>(
        instruction_accounts,
        program_id,
        account_handles,
        optional_sentinel_flags,
        |_, _| Ok(()),
    )
}

fn walk_instruction_account_bindings<
    'a,
    const N: usize,
    F: crate::OptionalSentinelFlags + ?Sized,
    V,
>(
    instruction_accounts: &[InstructionAccount<'a>],
    program_id: &Address,
    account_handles: &[CpiHandle<'a>],
    optional_sentinel_flags: &F,
    mut on_match: V,
) -> Result<usize, ProgramError>
where
    V: FnMut(&InstructionAccount<'a>, &CpiHandle<'a>) -> ProgramResult,
{
    let required_account_count =
        minimum_required_handle_count(instruction_accounts, program_id, optional_sentinel_flags);
    require!(
        account_handles.len() >= required_account_count,
        ProgramError::NotEnoughAccountKeys
    );

    // One reachability bit per handle replaces the heap-backed matrix. Storage
    // follows the caller's capacity, independently of the number of optional
    // instruction slots. Checking suffixes avoids exponential backtracking
    // when many sentinels match the same program account.
    let mut reachable = crate::FixedVec::<bool, N>::new();
    reachable
        .try_resize(account_handles.len(), false)
        .map_err(|_| ProgramError::InvalidArgument)?;
    require!(
        account_bindings_possible(
            instruction_accounts,
            program_id,
            account_handles,
            optional_sentinel_flags,
            0,
            &mut reachable,
        ),
        ProgramError::InvalidArgument
    );

    let mut handle_index = 0;
    for (instruction_index, account) in instruction_accounts.iter().enumerate() {
        let optional = is_skippable_optional_sentinel(
            program_id,
            account,
            optional_sentinel_flags,
            instruction_index,
        );
        if optional {
            let matches = account_handles
                .get(handle_index)
                .is_some_and(|handle| instruction_account_matches_handle(account, handle));
            if !matches
                || !account_bindings_possible(
                    &instruction_accounts[instruction_index + 1..],
                    program_id,
                    &account_handles[handle_index + 1..],
                    optional_sentinel_flags,
                    instruction_index + 1,
                    &mut reachable,
                )
            {
                continue;
            }
        }

        let handle = account_handles
            .get(handle_index)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        require!(
            instruction_account_matches_handle(account, handle),
            ProgramError::InvalidArgument
        );
        on_match(account, handle)?;
        handle_index += 1;
    }

    require!(
        handle_index == account_handles.len(),
        ProgramError::InvalidArgument
    );
    Ok(handle_index)
}

/// Whether the entire handle suffix can bind to the instruction suffix.
/// Updating in reverse keeps the previous row in the same caller-sized slice.
fn account_bindings_possible(
    instruction_accounts: &[InstructionAccount<'_>],
    program_id: &Address,
    handles: &[CpiHandle<'_>],
    optional_sentinel_flags: &(impl crate::OptionalSentinelFlags + ?Sized),
    instruction_offset: usize,
    reachable: &mut [bool],
) -> bool {
    let reachable = &mut reachable[..handles.len()];
    reachable.fill(false);
    let mut empty_prefix = true;
    for (index, account) in instruction_accounts.iter().enumerate() {
        let optional = is_skippable_optional_sentinel(
            program_id,
            account,
            optional_sentinel_flags,
            instruction_offset + index,
        );
        for handle_index in (0..handles.len()).rev() {
            let previous = if handle_index == 0 {
                empty_prefix
            } else {
                reachable[handle_index - 1]
            };
            reachable[handle_index] = (optional && reachable[handle_index])
                || (previous
                    && instruction_account_matches_handle(account, &handles[handle_index]));
        }
        empty_prefix &= optional;
    }
    reachable.last().copied().unwrap_or(empty_prefix)
}

fn minimum_required_handle_count<F: crate::OptionalSentinelFlags + ?Sized>(
    instruction_accounts: &[InstructionAccount<'_>],
    program_id: &Address,
    optional_sentinel_flags: &F,
) -> usize {
    instruction_accounts
        .iter()
        .enumerate()
        .filter(|(index, account)| {
            !is_skippable_optional_sentinel(program_id, account, optional_sentinel_flags, *index)
        })
        .count()
}

fn instruction_account_matches_handle(
    account: &InstructionAccount<'_>,
    handle: &CpiHandle<'_>,
) -> bool {
    address_eq(account.address, handle.address()) && (!account.is_writable || handle.is_writable())
}

fn signers<'seeds, const N: usize>(
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
) -> Result<crate::SignerBuffer<'seeds, N>, ProgramError> {
    let mut signers = crate::SignerBuffer::new();
    for seeds in signer_seeds {
        signers
            .try_push(crate::cpi::signer_from_seeds(seeds))
            .map_err(|_| ProgramError::InvalidArgument)?;
    }
    Ok(signers)
}

pub(crate) fn cpi_accounts<'a, const N: usize>(
    account_handles: &[CpiHandle<'a>],
) -> Result<crate::CpiAccountBuffer<'a, N>, ProgramError> {
    let mut accounts = crate::CpiAccountBuffer::new();
    for handle in account_handles {
        let mut slot = MaybeUninit::uninit();
        CpiAccount::init_from_account_view(handle.account_view(), &mut slot);
        accounts
            .try_push(slot)
            .map_err(|_| ProgramError::InvalidArgument)?;
    }

    Ok(accounts)
}
