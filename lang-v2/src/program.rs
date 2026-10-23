use {
    crate::{address_eq, require, Address, CpiHandle, CpiInstruction},
    alloc::vec::Vec,
    core::{mem::MaybeUninit, slice::from_raw_parts},
    pinocchio::{
        cpi::{CpiAccount, Seed, Signer},
        instruction::{InstructionAccount, InstructionView},
    },
    solana_program_error::{ProgramError, ProgramResult},
};

pub use pinocchio::cpi::{set_return_data, MAX_RETURN_DATA};

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
pub fn invoke<'a, I: CpiInstruction + ?Sized>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
) -> ProgramResult {
    invoke_signed(instruction, account_handles, &[])
}

/// Like [`invoke`], but marks instruction-account indices that are intentional
/// optional `None` program-id sentinels (parallel to
/// [`crate::ToCpiAccountsWithCapacity::optional_account_sentinel_flags_with_capacity`]).
pub fn invoke_with_optional_sentinels<'a, I: CpiInstruction + ?Sized>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
    optional_sentinel_flags: &[bool],
) -> ProgramResult {
    invoke_signed_with_optional_sentinels(instruction, account_handles, &[], optional_sentinel_flags)
}

/// Invoke a cross-program instruction with PDA signer seeds using Anchor v2
/// CPI handles.
pub fn invoke_signed<'a, 'seeds, I: CpiInstruction + ?Sized>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
) -> ProgramResult {
    invoke_signed_with_optional_sentinels(instruction, account_handles, signer_seeds, &[])
}

/// Like [`invoke_signed`], with an explicit optional-sentinel mask.
pub fn invoke_signed_with_optional_sentinels<'a, 'seeds, I: CpiInstruction + ?Sized>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
    optional_sentinel_flags: &[bool],
) -> ProgramResult {
    instruction.with_instruction_accounts(|instruction_accounts| {
        validate_instruction_accounts(
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
        invoke_signed_unchecked_with_optional_sentinels(
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
pub unsafe fn invoke_unchecked<'a, I: CpiInstruction + ?Sized>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
) -> ProgramResult {
    unsafe { invoke_signed_unchecked(instruction, account_handles, &[]) }
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
pub unsafe fn invoke_signed_unchecked<'a, 'seeds, I: CpiInstruction + ?Sized>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
) -> ProgramResult {
    unsafe { invoke_signed_unchecked_with_optional_sentinels(instruction, account_handles, signer_seeds, &[]) }
}

/// Like [`invoke_signed_unchecked`], with an explicit optional-sentinel mask.
///
/// # Safety
///
/// Same as [`invoke_signed_unchecked`].
pub unsafe fn invoke_signed_unchecked_with_optional_sentinels<'a, 'seeds, I: CpiInstruction + ?Sized>(
    instruction: &I,
    account_handles: &[CpiHandle<'a>],
    signer_seeds: &'seeds [&'seeds [&'seeds [u8]]],
    optional_sentinel_flags: &[bool],
) -> ProgramResult {
    instruction.with_instruction_accounts(|instruction_accounts| {
        let cpi_account_count = required_cpi_account_count(
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
        let signers = signers(signer_seeds);
        let _borrow_guards = crate::enter_cpi(account_handles);
        let cpi_accounts = cpi_accounts(account_handles);

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

pub(crate) fn validate_instruction_accounts<'a>(
    instruction_accounts: &[InstructionAccount<'a>],
    program_id: &Address,
    account_handles: &[CpiHandle<'a>],
    optional_sentinel_flags: &[bool],
    enforce_signers: bool,
) -> ProgramResult {
    let bindings = resolve_instruction_account_bindings(
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
}

fn is_optional_instruction_account_sentinel_candidate(
    program_id: &Address,
    account: &InstructionAccount<'_>,
) -> bool {
    !account.is_writable && !account.is_signer && address_eq(account.address, program_id)
}

fn is_skippable_optional_sentinel(
    program_id: &Address,
    account: &InstructionAccount<'_>,
    optional_sentinel_flags: &[bool],
    instruction_index: usize,
) -> bool {
    optional_sentinel_flags
        .get(instruction_index)
        .copied()
        .unwrap_or(false)
        && is_optional_instruction_account_sentinel_candidate(program_id, account)
}

fn required_cpi_account_count<'a>(
    instruction_accounts: &[InstructionAccount<'a>],
    program_id: &Address,
    account_handles: &[CpiHandle<'a>],
    optional_sentinel_flags: &[bool],
) -> Result<usize, ProgramError> {
    Ok(resolve_instruction_account_bindings(
        instruction_accounts,
        program_id,
        account_handles,
        optional_sentinel_flags,
        |_, _| Ok(()),
    )
}

fn resolve_instruction_account_bindings<'a>(
    instruction_accounts: &[InstructionAccount<'a>],
    program_id: &Address,
    account_handles: &[CpiHandle<'a>],
    optional_sentinel_flags: &[bool],
) -> Result<Vec<Option<usize>>, ProgramError> {
    let cols = account_handles.len() + 1;
    let rows = instruction_accounts.len() + 1;
    let mut can_match = alloc::vec![false; rows * cols];

    for handle_index in 0..=account_handles.len() {
        can_match[(rows - 1) * cols + handle_index] = true;
    }

    for instruction_index in (0..instruction_accounts.len()).rev() {
        let account = &instruction_accounts[instruction_index];
        for handle_index in (0..=account_handles.len()).rev() {
            let can_consume = handle_index < account_handles.len()
                && instruction_account_matches_handle(account, &account_handles[handle_index])
                && can_match[(instruction_index + 1) * cols + (handle_index + 1)];
            let can_skip = is_skippable_optional_sentinel(
                program_id,
                account,
                optional_sentinel_flags,
                instruction_index,
            ) && can_match[(instruction_index + 1) * cols + handle_index];
            can_match[instruction_index * cols + handle_index] = can_consume || can_skip;
        }
    }

    require!(
        can_match[0],
        if account_handles.len()
            < minimum_required_handle_count(
                instruction_accounts,
                program_id,
                optional_sentinel_flags
            )
        {
            ProgramError::NotEnoughAccountKeys
        } else {
            ProgramError::InvalidArgument
        }
    );

    let mut bindings = Vec::with_capacity(instruction_accounts.len());
    let mut handle_index = 0;
    let mut cpi_account_count = 0;

    for (instruction_index, account) in instruction_accounts.iter().enumerate() {
        let can_skip = is_skippable_optional_sentinel(
            program_id,
            account,
            optional_sentinel_flags,
            instruction_index,
        ) && (handle_index >= account_handles.len()
            || instruction_accounts[instruction_index + 1..]
                .iter()
                .enumerate()
                .any(|(offset, later)| {
                    let later_index = instruction_index + 1 + offset;
                    !is_skippable_optional_sentinel(
                        program_id,
                        later,
                        optional_sentinel_flags,
                        later_index,
                    ) && instruction_account_matches_handle(later, &account_handles[handle_index])
                }));
        if can_skip {
            continue;
        }

        let Some(handle) = account_handles.get(handle_index) else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };
        if !instruction_account_matches_handle(account, handle) {
            return Err(ProgramError::InvalidArgument);
        }

        on_match(account, handle)?;
        handle_index += 1;
        cpi_account_count += 1;
    }

    if handle_index != account_handles.len() {
        return Err(ProgramError::InvalidArgument);
    }

fn minimum_required_handle_count(
    instruction_accounts: &[InstructionAccount<'_>],
    program_id: &Address,
    optional_sentinel_flags: &[bool],
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
    address_eq(account.address, handle.address())
        && (!account.is_writable || handle.is_writable())
}

fn signers<'seeds>(signer_seeds: &'seeds [&'seeds [&'seeds [u8]]]) -> Vec<Signer<'seeds, 'seeds>> {
    signer_seeds
        .iter()
        .map(|seeds| {
            // SAFETY: `Seed` has the same in-memory representation as `&[u8]`;
            // this is the conversion used by `CpiContext::invoke` as well.
            let cpi_seeds: &[Seed] =
                unsafe { from_raw_parts(seeds.as_ptr() as *const Seed, seeds.len()) };
            Signer::from(cpi_seeds)
        })
        .collect()
}

fn cpi_accounts<'a>(
    account_handles: &[CpiHandle<'a>],
) -> Result<crate::CpiAccountBuffer<'a>, ProgramError> {
    let mut accounts = crate::CpiAccountBuffer::new();
    for handle in account_handles {
        let mut slot = MaybeUninit::uninit();
        CpiAccount::init_from_account_view(handle.account_view(), &mut slot);
        #[cfg(not(any(target_os = "solana", target_arch = "bpf")))]
        accounts.push(slot);
        #[cfg(any(target_os = "solana", target_arch = "bpf"))]
        accounts
            .try_push(slot)
            .map_err(|_| ProgramError::InvalidArgument)?;
    }

    Ok(accounts)
}
