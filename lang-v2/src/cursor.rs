//! Lazy cursor over the serialized instruction input buffer.
//!
//! [`AccountCursor`] walks account records on demand. Duplicate records are
//! resolved by rescanning previously consumed records, which avoids reserving
//! transaction-wide account-view storage in the entrypoint frame.

use {
    core::mem::MaybeUninit,
    pinocchio::account::{AccountView, RuntimeAccount, MAX_PERMITTED_DATA_INCREASE},
};

/// Sentinel value in the serialized `borrow_state` byte indicating a
/// non-duplicated account. Any other value identifies an earlier account.
pub const NON_DUP_MARKER: u8 = u8::MAX;

const STATIC_ACCOUNT_DATA: usize =
    core::mem::size_of::<RuntimeAccount>() + MAX_PERMITTED_DATA_INCREASE;
const BPF_ALIGN_OF_U128: usize = 8;

/// Marker written into the unused padding of a consumed duplicate record.
/// `AccountView` reuses `borrow_state` as its live borrow counter, so the
/// original duplicate index must be retained elsewhere for later lazy walks.
pub(crate) const DUPLICATE_METADATA_MASK: u32 = 0x8000_0000;

/// Cursor into the runtime's serialized instruction account region.
///
/// # Safety
///
/// The cursor is constructed from the runtime-provided input pointer. It must
/// not outlive that input, and callers must never walk more records than the
/// serialized account count. Duplicate indices are trusted to reference an
/// earlier record, as guaranteed by the runtime serializer.
pub struct AccountCursor {
    ptr: *mut u8,
    input_start: *mut u8,
    consumed: usize,
}

impl AccountCursor {
    /// Create a cursor at the first serialized account record.
    ///
    /// `input_ptr` points to the runtime's account-count prefix.
    ///
    /// # Safety
    ///
    /// See the type-level safety contract.
    #[doc(hidden)]
    #[inline(always)]
    pub unsafe fn new(input_ptr: *mut u8) -> Self {
        let input_start = input_ptr.add(core::mem::size_of::<u64>());
        Self {
            ptr: input_start,
            input_start,
            consumed: 0,
        }
    }

    #[inline(always)]
    pub fn consumed(&self) -> usize {
        self.consumed
    }

    #[inline(always)]
    pub(crate) fn input_start(&self) -> *mut u8 {
        self.input_start
    }

    /// Walk exactly `output.len()` accounts into caller-owned storage.
    ///
    /// # Safety
    ///
    /// `output.len()` must not exceed the number of unconsumed records.
    #[inline(always)]
    pub(crate) unsafe fn walk_into<'views>(
        &mut self,
        output: &'views mut [MaybeUninit<AccountView>],
    ) -> &'views [AccountView] {
        for slot in output.iter_mut() {
            slot.write(self.next());
        }
        core::slice::from_raw_parts(output.as_ptr().cast::<AccountView>(), output.len())
    }

    /// Return the record immediately after `account`.
    #[inline(always)]
    pub(crate) unsafe fn next_record(account: *mut RuntimeAccount) -> *mut u8 {
        let mut ptr = (account as *mut u8).add(core::mem::size_of::<u64>());
        let padding = u32::from_le_bytes((*account).padding);
        if padding & DUPLICATE_METADATA_MASK != 0 {
            return ptr;
        }

        ptr = ptr.add(STATIC_ACCOUNT_DATA + (*account).data_len as usize);
        let addr = ptr.addr();
        let aligned = (addr + (BPF_ALIGN_OF_U128 - 1)) & !(BPF_ALIGN_OF_U128 - 1);
        ptr.add(aligned - addr)
    }

    /// Locate a consumed record by serialized index.
    #[inline(always)]
    pub(crate) unsafe fn record_at(input_start: *mut u8, index: usize) -> *mut RuntimeAccount {
        let mut ptr = input_start;
        let mut current = 0usize;
        while current < index {
            ptr = Self::next_record(ptr.cast::<RuntimeAccount>());
            current += 1;
        }
        ptr.cast::<RuntimeAccount>()
    }

    /// Resolve a consumed serialized slot to its original account.
    ///
    /// The duplicate chain must be strictly decreasing. Returning `None` for
    /// an invalid chain keeps lazy safe APIs from looping on malformed test
    /// input; the runtime-provided input satisfies this invariant.
    #[inline(always)]
    pub(crate) unsafe fn resolve_at(input_start: *mut u8, index: usize) -> Option<AccountView> {
        let mut target = index;
        let mut remaining_steps = index + 1;
        while remaining_steps != 0 {
            let account = Self::record_at(input_start, target);
            let padding = u32::from_le_bytes((*account).padding);
            if padding & DUPLICATE_METADATA_MASK == 0 {
                return Some(AccountView::new_unchecked(account));
            }

            let previous = (padding & !DUPLICATE_METADATA_MASK) as usize;
            if previous >= target {
                return None;
            }
            target = previous;
            remaining_steps -= 1;
        }
        None
    }

    /// Advance past one serialized account record.
    ///
    /// # Safety
    ///
    /// The cursor must not already be at the end of the account region.
    #[inline(always)]
    pub unsafe fn next(&mut self) -> AccountView {
        let account = self.ptr.cast::<RuntimeAccount>();
        let borrow_state = (*account).borrow_state;

        let view = if self.consumed == 0 || borrow_state == NON_DUP_MARKER {
            debug_assert!((*account).data_len < DUPLICATE_METADATA_MASK as u64);
            (*account).padding = u32::to_le_bytes((*account).data_len as u32);
            AccountView::new_unchecked(account)
        } else {
            debug_assert!((borrow_state as usize) < self.consumed);
            (*account).padding = u32::to_le_bytes(DUPLICATE_METADATA_MASK | borrow_state as u32);
            match Self::resolve_at(self.input_start, borrow_state as usize) {
                Some(view) => view,
                // SAFETY: a non-decreasing or out-of-range duplicate index
                // violates the runtime input contract documented above.
                None => core::hint::unreachable_unchecked(),
            }
        };

        self.ptr = Self::next_record(account);
        self.consumed += 1;
        view
    }
}
