use {
    crate::cursor::{AccountCursor, DUPLICATE_METADATA_MASK},
    core::marker::PhantomData,
    pinocchio::{
        account::{AccountView, RuntimeAccount},
        address::Address,
    },
};

/// Instruction-scoped context passed to every handler. Holds the
/// declared accounts, program_id, PDA bumps, and a cursor for lazy
/// `remaining_accounts()` access.
pub struct Context<'a, T: Bumps> {
    /// Program id as a reference — lives for the whole instruction
    /// since it comes from the entrypoint's input buffer.
    pub program_id: &'a Address,

    /// Declared accounts (the `#[derive(Accounts)]` struct).
    pub accounts: T,

    /// Bump seeds found during constraint validation. Provided as a
    /// convenience so handlers don't have to recalculate bump seeds or
    /// pass them in as arguments.
    pub bumps: T::Bumps,

    /// Holds the cursor and the serialized range of trailing accounts after
    /// `try_accounts` has consumed the declared account header.
    remaining_accounts: Option<(usize, usize)>,
    remaining_num: usize,

    cursor: &'a mut AccountCursor,
}

/// Borrowed access to trailing accounts without allocating a view array.
#[derive(Clone, Copy)]
pub struct RemainingAccounts<'a> {
    input_start: *mut u8,
    start: usize,
    len: usize,
    marker: PhantomData<&'a AccountView>,
}

impl<'a> RemainingAccounts<'a> {
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<AccountView> {
        if index >= self.len {
            return None;
        }
        let index = self.start.checked_add(index)?;
        // SAFETY: `remaining_accounts` walks this complete range before
        // constructing the view, and the runtime input lives for `'a`.
        unsafe { AccountCursor::resolve_at(self.input_start, index) }
    }

    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = AccountView> + '_ {
        // SAFETY: `start` identifies a consumed record boundary.
        let ptr = unsafe { AccountCursor::record_at(self.input_start, self.start) };
        RemainingAccountsIter {
            input_start: self.input_start,
            ptr,
            index: self.start,
            remaining: self.len,
            marker: PhantomData,
        }
    }
}

struct RemainingAccountsIter<'a> {
    input_start: *mut u8,
    ptr: *mut RuntimeAccount,
    index: usize,
    remaining: usize,
    marker: PhantomData<&'a AccountView>,
}

impl Iterator for RemainingAccountsIter<'_> {
    type Item = AccountView;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let account = self.ptr;
        let padding = unsafe { u32::from_le_bytes((*account).padding) };
        let view = if padding & DUPLICATE_METADATA_MASK == 0 {
            // SAFETY: ordinary remaining records were consumed by the cursor
            // before this iterator was created.
            Some(unsafe { AccountView::new_unchecked(account) })
        } else {
            // SAFETY: duplicate metadata was validated and preserved by the
            // cursor when the range was consumed.
            unsafe { AccountCursor::resolve_at(self.input_start, self.index) }
        }?;

        // SAFETY: `remaining` bounds this walk to the consumed range.
        self.ptr = unsafe { AccountCursor::next_record(account) }.cast();
        self.index += 1;
        self.remaining -= 1;
        Some(view)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for RemainingAccountsIter<'_> {}

impl<'a, T: Bumps> Context<'a, T> {
    #[inline(always)]
    pub fn new(
        program_id: &'a Address,
        accounts: T,
        bumps: T::Bumps,
        cursor: &'a mut AccountCursor,
        remaining_num: usize,
    ) -> Self {
        Self {
            program_id,
            accounts,
            bumps,
            remaining_accounts: None,
            remaining_num,
            cursor,
        }
    }

    /// Returns a borrowed lazy view of accounts beyond the declared fields.
    /// The cursor advances once; repeated calls return the same range.
    pub fn remaining_accounts(&mut self) -> RemainingAccounts<'a> {
        let (start, len) = match self.remaining_accounts {
            Some(range) => range,
            None => {
                let start = self.cursor.consumed();
                for _ in 0..self.remaining_num {
                    // SAFETY: cursor is positioned at the start of the remaining
                    // region and `remaining_num` is the exact number of accounts
                    // to walk.
                    unsafe { self.cursor.next() };
                }
                let range = (start, self.remaining_num);
                self.remaining_accounts = Some(range);
                range
            }
        };
        RemainingAccounts {
            input_start: self.cursor.input_start(),
            start,
            len,
            marker: PhantomData,
        }
    }
}

/// Trait linking an accounts struct to its generated bumps struct.
pub trait Bumps {
    type Bumps;
}
