//! Miri witnesses for `AccountCursor::next()` — the core dup-index
//! resolution logic that the SBF loader writes into the program's
//! input buffer.
//!
//! Uses `SbfInputBuffer` in `common/` to construct a realistic
//! serialized-input layout (`num_accounts` + per-account records)
//! and walks it via the cursor. Exercises:
//!
//! 1. All-non-dup walk — every account gets a fresh `AccountView`
//! 2. Mixed walk with duplicates — dup indices resolve to earlier views
//! 3. First-account short-circuit — the `consumed == 0` path is
//!    hit when the first record's header has a zero byte (looks like
//!    a dup index but it's the very first record)
//! 4. Consecutive dups — second account dups first, third dups second
//!
//! Run: `MIRIFLAGS="-Zmiri-tree-borrows" cargo +nightly miri test -p anchor-lang --test miri_cursor_walk`
//!
//! Tree Borrows compatible: the cursor's 8-byte-alignment step uses
//! strict-provenance `.addr()` + `.add(delta)` rather than an
//! int-to-ptr mask, so the derivation chain survives Miri's check.

use anchor_lang::{
    cursor::AccountCursor,
    testing::{AccountRecord, SbfInputBuffer},
};

// Run a closure with the same compact cursor generated entrypoints use.
fn with_cursor<F, R>(input: &mut SbfInputBuffer, f: F) -> R
where
    F: FnOnce(&mut AccountCursor) -> R,
{
    let mut cursor = unsafe { AccountCursor::new(input.as_mut_ptr()) };
    f(&mut cursor)
}

// -- Baseline: three non-dup accounts walk correctly ------------------

#[test]
fn all_non_dup_walk() {
    let mut input = SbfInputBuffer::build(&[
        AccountRecord::NonDup {
            address: [0x11; 32],
            owner: [0xAA; 32],
            lamports: 100,
            is_signer: true,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
        AccountRecord::NonDup {
            address: [0x22; 32],
            owner: [0xBB; 32],
            lamports: 200,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 16,
        },
        AccountRecord::NonDup {
            address: [0x33; 32],
            owner: [0xCC; 32],
            lamports: 300,
            is_signer: false,
            is_writable: false,
            executable: false,
            data_len: 32,
        },
    ]);

    with_cursor(&mut input, |cursor| {
        let v0 = unsafe { cursor.next() };
        let v1 = unsafe { cursor.next() };
        let v2 = unsafe { cursor.next() };

        assert_eq!(v0.address().to_bytes(), [0x11; 32]);
        assert_eq!(v1.address().to_bytes(), [0x22; 32]);
        assert_eq!(v2.address().to_bytes(), [0x33; 32]);

        assert_eq!(v0.lamports(), 100);
        assert_eq!(v1.lamports(), 200);
        assert_eq!(v2.lamports(), 300);

        assert!(v0.is_signer());
        assert!(!v1.is_signer());

        assert_eq!(cursor.consumed(), 3);
    });
}

// -- Duplicate: second account is a dup of the first -----------------

#[test]
fn dup_resolves_to_earlier_view() {
    let mut input = SbfInputBuffer::build(&[
        AccountRecord::NonDup {
            address: [0x11; 32],
            owner: [0xAA; 32],
            lamports: 500,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
        AccountRecord::Dup { index: 0 },
        AccountRecord::NonDup {
            address: [0x33; 32],
            owner: [0xCC; 32],
            lamports: 300,
            is_signer: false,
            is_writable: false,
            executable: false,
            data_len: 0,
        },
    ]);

    with_cursor(&mut input, |cursor| {
        let v0 = unsafe { cursor.next() };
        let v1 = unsafe { cursor.next() };
        let v2 = unsafe { cursor.next() };

        // v1 should resolve to the same AccountView as v0 (same address,
        // same lamports — actually same raw pointer internally).
        assert_eq!(v1.address().to_bytes(), [0x11; 32]);
        assert_eq!(v1.lamports(), 500);
        assert_eq!(v0.address(), v1.address());

        // v2 is a separate non-dup account.
        assert_eq!(v2.address().to_bytes(), [0x33; 32]);
        assert_ne!(v1.address(), v2.address());
    });
}

// -- Chained dup: third account dups second which dups first ---------
//
// In the SBF wire format, each dup record carries the *original*
// index of the duplicated account (not the most recent reference).
// So [non_dup_A, dup→A, dup→A] is valid; [non_dup_A, dup→A, dup→1]
// (where 1 points to the previous dup slot) is also valid and
// should resolve to A.

#[test]
fn chained_dup_resolves_transitively() {
    let mut input = SbfInputBuffer::build(&[
        AccountRecord::NonDup {
            address: [0x77; 32],
            owner: [0xBB; 32],
            lamports: 42,
            is_signer: true,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
        AccountRecord::Dup { index: 0 }, // dups index 0 directly
        AccountRecord::Dup { index: 1 }, // dups index 1 (which was already dup of 0)
    ]);

    with_cursor(&mut input, |cursor| {
        let v0 = unsafe { cursor.next() };
        let v1 = unsafe { cursor.next() };
        let v2 = unsafe { cursor.next() };

        // All three must refer to the same underlying account.
        assert_eq!(v0.address(), v1.address());
        assert_eq!(v1.address(), v2.address());
        assert_eq!(v0.lamports(), 42);
        assert_eq!(v2.lamports(), 42);
    });
}

#[test]
fn non_consecutive_duplicate_resolves_to_the_original_account() {
    let mut input = SbfInputBuffer::build(&[
        AccountRecord::NonDup {
            address: [0x41; 32],
            owner: [0; 32],
            lamports: 41,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
        AccountRecord::NonDup {
            address: [0x42; 32],
            owner: [0; 32],
            lamports: 42,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
        AccountRecord::NonDup {
            address: [0x43; 32],
            owner: [0; 32],
            lamports: 43,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
        AccountRecord::Dup { index: 0 },
    ]);

    with_cursor(&mut input, |cursor| {
        let original = unsafe { cursor.next() };
        let _middle_a = unsafe { cursor.next() };
        let _middle_b = unsafe { cursor.next() };
        let duplicate = unsafe { cursor.next() };

        assert_eq!(duplicate.address(), original.address());
        assert_eq!(duplicate.lamports(), 41);
    });
}

#[test]
fn highest_valid_prior_index_resolves_without_a_lookup_table() {
    let mut records = Vec::with_capacity(pinocchio::MAX_TX_ACCOUNTS);
    records.push(AccountRecord::NonDup {
        address: [0x55; 32],
        owner: [0; 32],
        lamports: 55,
        is_signer: false,
        is_writable: true,
        executable: false,
        data_len: 0,
    });
    // A transaction contains at most 255 account records (indices 0..=254).
    // The final record can therefore point no later than prior index 253;
    // marker 255 is reserved for a non-duplicate record.
    for index in 0..=253u8 {
        records.push(AccountRecord::Dup { index });
    }

    let mut input = SbfInputBuffer::build(&records);
    with_cursor(&mut input, |cursor| {
        let first = unsafe { cursor.next() };
        let mut last = first;
        for _ in 1..records.len() {
            last = unsafe { cursor.next() };
        }

        assert_eq!(last.address(), first.address());
        assert_eq!(last.lamports(), 55);
        assert_eq!(cursor.consumed(), pinocchio::MAX_TX_ACCOUNTS);
    });
}

#[test]
fn sequential_walk_preserves_record_order() {
    let mut input = SbfInputBuffer::build(&[
        AccountRecord::NonDup {
            address: [0xAA; 32],
            owner: [0; 32],
            lamports: 1,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
        AccountRecord::NonDup {
            address: [0xBB; 32],
            owner: [0; 32],
            lamports: 2,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
        AccountRecord::NonDup {
            address: [0xCC; 32],
            owner: [0; 32],
            lamports: 3,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
    ]);

    with_cursor(&mut input, |cursor| {
        let views = core::array::from_fn::<_, 3, _>(|_| unsafe { cursor.next() });
        assert_eq!(views.len(), 3);
        assert_eq!(views[0].address().to_bytes(), [0xAA; 32]);
        assert_eq!(views[1].address().to_bytes(), [0xBB; 32]);
        assert_eq!(views[2].address().to_bytes(), [0xCC; 32]);
    });
}

// -- Non-zero data_len paths -----------------------------------------
//
// The cursor advances `STATIC_ACCOUNT_DATA + data_len + alignment`
// for non-dup records. Verify the pointer math holds across varied
// data_len values (otherwise subsequent accounts would be read from
// the wrong offset).

#[test]
fn non_dup_with_varied_data_len_advances_correctly() {
    let mut input = SbfInputBuffer::build(&[
        AccountRecord::NonDup {
            address: [0x10; 32],
            owner: [0; 32],
            lamports: 1,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 7, // odd-size triggers alignment padding
        },
        AccountRecord::NonDup {
            address: [0x20; 32],
            owner: [0; 32],
            lamports: 2,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 1000,
        },
        AccountRecord::NonDup {
            address: [0x30; 32],
            owner: [0; 32],
            lamports: 3,
            is_signer: false,
            is_writable: true,
            executable: false,
            data_len: 0,
        },
    ]);

    with_cursor(&mut input, |cursor| {
        let v0 = unsafe { cursor.next() };
        let v1 = unsafe { cursor.next() };
        let v2 = unsafe { cursor.next() };

        assert_eq!(v0.address().to_bytes(), [0x10; 32]);
        assert_eq!(v1.address().to_bytes(), [0x20; 32]);
        assert_eq!(v2.address().to_bytes(), [0x30; 32]);
    });
}

// -- First-account short-circuit (consumed == 0) ---------------------
//
// The code at cursor.rs:137 has: `if self.consumed == 0 || borrow_state == NON_DUP_MARKER`.
// If the first record's borrow_state byte were accidentally set to
// something OTHER than NON_DUP_MARKER (which should never happen per
// the SBF ABI — first record is always non-dup), the `consumed == 0`
// short-circuit keeps the walk from reading a phantom dup slot.
//
// Test: manually corrupt the first record's borrow_state byte to 0
// (which would look like "dup of index 0" — itself) and verify the
// walk still treats it as non-dup.

#[test]
fn first_account_short_circuit_overrides_wrong_borrow_state_byte() {
    let mut input = SbfInputBuffer::build(&[AccountRecord::NonDup {
        address: [0xEE; 32],
        owner: [0; 32],
        lamports: 99,
        is_signer: false,
        is_writable: true,
        executable: false,
        data_len: 0,
    }]);

    // Corrupt the borrow_state byte at the first record's offset — if
    // the short-circuit didn't fire, the cursor would interpret the
    // record as a self-reference instead of an account header.
    let offset = input.record_offsets[0];
    input.bytes_mut()[offset] = 0; // simulate "dup of index 0"

    with_cursor(&mut input, |cursor| {
        // Should NOT UB — the `consumed == 0` short-circuit takes the
        // non-dup path regardless of the byte value.
        let v = unsafe { cursor.next() };
        assert_eq!(v.address().to_bytes(), [0xEE; 32]);
        assert_eq!(v.lamports(), 99);
    });
}
