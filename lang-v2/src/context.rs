use {
    crate::cursor::AccountCursor,
    core::marker::PhantomData,
    pinocchio::{account::AccountView, address::Address},
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
    cursor: *const AccountCursor,
    start: usize,
    len: usize,
    marker: PhantomData<&'a AccountView>,
}

impl<'a> RemainingAccounts<'a> {
    #[inline]
    pub const fn len(self) -> usize {
        self.len
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn get(self, index: usize) -> Option<AccountView> {
        if index >= self.len {
            return None;
        }
        let index = self
            .start
            .checked_add(index)
            .and_then(|index| u8::try_from(index).ok())?;
        // SAFETY: the cursor remains valid for the view lifetime.
        Some(unsafe { (*self.cursor).view_at(index) })
    }

    #[inline]
    pub fn iter(self) -> RemainingAccountsIter<'a> {
        RemainingAccountsIter {
            accounts: self,
            index: 0,
        }
    }
}

pub struct RemainingAccountsIter<'a> {
    accounts: RemainingAccounts<'a>,
    index: usize,
}

impl Iterator for RemainingAccountsIter<'_> {
    type Item = AccountView;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.accounts.len {
            return None;
        }
        let index = self.index;
        self.index += 1;
        self.accounts.get(index)
    }
}

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
                let start = self.cursor.consumed() as usize;
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
            cursor: self.cursor as *const AccountCursor,
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
