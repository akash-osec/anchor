//! Tests for `AccountCursor` and `Context::remaining_accounts`.
//!
//! These walks are the hot path between the SBF loader's serialized
//! input buffer and the typed-account machinery. Coverage here pins
//! three things that tests elsewhere don't exercise:
//!
//!   1. `AccountCursor::next` advances the raw pointer past each
//!      account record (header + data + padding + rent-epoch + 8-byte
//!      align) so subsequent reads see the next record, not the tail
//!      of the previous one.
//!   2. Duplicate handling: a dup-record (borrow_state ∈ 0..=254)
//!      resolves to the earlier account without a transaction-wide lookup.
//!   3. `Context::remaining_accounts` walks the cursor once on first call,
//!      then resolves entries lazily from the serialized input without
//!      allocating or advancing the cursor again.
//!
//! Run: `cargo test -p anchor-lang --features testing --test cursor_and_remaining_accounts`

use {
    anchor_lang::{
        cursor::AccountCursor,
        testing::{AccountRecord, SbfInputBuffer},
        AccountViewCompat, Bumps, Context,
    },
    pinocchio::{account::AccountView, Resize},
    solana_address::Address,
};

// A placeholder header struct that implements `Bumps` so we can construct
// a `Context<DummyHeader>` without needing the full `#[derive(Accounts)]`
// machinery. Empty `Bumps = ()` — no bumps tracked for remaining-only tests.
struct DummyHeader;
impl Bumps for DummyHeader {
    type Bumps = ();
}
fn unique_addr(i: u8) -> [u8; 32] {
    let mut a = [0u8; 32];
    a[0] = i + 1; // avoid [0;32] which collides with the System program id.
    a
}

fn non_dup(i: u8) -> AccountRecord {
    AccountRecord::NonDup {
        address: unique_addr(i),
        owner: [0xAA; 32],
        lamports: 100 + i as u64,
        is_signer: false,
        is_writable: false,
        executable: false,
        data_len: 0,
    }
}

fn non_dup_with_data(i: u8, data_len: usize) -> AccountRecord {
    AccountRecord::NonDup {
        address: unique_addr(i),
        owner: [0xAA; 32],
        lamports: 100,
        is_signer: false,
        is_writable: false,
        executable: false,
        data_len,
    }
}

fn walk<const N: usize>(cursor: &mut AccountCursor) -> [AccountView; N] {
    core::array::from_fn(|_| unsafe { cursor.next() })
}

// -- AccountCursor::next walks each record ---------------------------------

#[test]
fn cursor_next_advances_across_records() {
    let mut sbf = SbfInputBuffer::build(&[non_dup(0), non_dup(1), non_dup(2)]);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };

    assert_eq!(cursor.consumed(), 0);
    let v0 = unsafe { cursor.next() };
    assert_eq!(v0.address().to_bytes(), unique_addr(0));
    assert_eq!(cursor.consumed(), 1);
    let v1 = unsafe { cursor.next() };
    assert_eq!(v1.address().to_bytes(), unique_addr(1));
    let v2 = unsafe { cursor.next() };
    assert_eq!(v2.address().to_bytes(), unique_addr(2));
    assert_eq!(cursor.consumed(), 3);
}

#[test]
fn cursor_next_walks_past_variable_data_regions() {
    // Non-zero `data_len` values exercise the `ptr += STATIC + data_len`
    // branch plus the 8-byte alignment fixup. If the alignment math is
    // off the next record's header reads as garbage.
    let records = [
        non_dup_with_data(0, 3),  // unaligned data_len → fixup adds 5 bytes
        non_dup_with_data(1, 17), // unaligned → fixup adds 7 bytes
        non_dup_with_data(2, 8),  // aligned → fixup adds 0 bytes
    ];
    let mut sbf = SbfInputBuffer::build(&records);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };

    for i in 0u8..3 {
        let view = unsafe { cursor.next() };
        assert_eq!(
            view.address().to_bytes(),
            unique_addr(i),
            "record {i} address mismatch — alignment-fixup bug?"
        );
        let expected_data_len = match i {
            0 => 3,
            1 => 17,
            _ => 8,
        };
        assert_eq!(view.data_len(), expected_data_len);
    }
}

#[test]
fn cursor_walks_all_views_in_order() {
    let mut sbf = SbfInputBuffer::build(&[non_dup(0), non_dup(1), non_dup(2), non_dup(3)]);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };

    let views = walk::<4>(&mut cursor);
    assert_eq!(views.len(), 4);
    for (i, v) in views.iter().enumerate() {
        assert_eq!(v.address().to_bytes(), unique_addr(i as u8));
    }
    assert_eq!(cursor.consumed(), 4);
}

// -- Duplicate resolution --------------------------------------------------

#[test]
fn cursor_dup_resolves_to_earlier_view() {
    // Record 2 is a dup of record 0. The cursor must return the
    // The duplicate must resolve to record 0 rather than the compact dup slot.
    let records = [non_dup(0), non_dup(1), AccountRecord::Dup { index: 0 }];
    let mut sbf = SbfInputBuffer::build(&records);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };

    let views = walk::<3>(&mut cursor);

    assert_eq!(views[0].address().to_bytes(), unique_addr(0));
    assert_eq!(views[1].address().to_bytes(), unique_addr(1));
    // Dup slot — resolved to the original record.
    assert_eq!(views[2].address().to_bytes(), unique_addr(0));
}

// -- Context::remaining_accounts ------------------------------------------

#[test]
fn remaining_accounts_walks_trailing_region() {
    // Full transaction has 5 accounts: 2 declared, 3 trailing.
    let records = [non_dup(0), non_dup(1), non_dup(2), non_dup(3), non_dup(4)];
    let mut sbf = SbfInputBuffer::build(&records);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };

    // Simulate the dispatcher consuming the declared (HEADER_SIZE=2) accounts.
    let _ = walk::<2>(&mut cursor);
    assert_eq!(cursor.consumed(), 2);

    let program_id = Address::new_from_array([0x42; 32]);
    let mut ctx: Context<'_, DummyHeader> = Context::new(
        &program_id,
        DummyHeader,
        (),
        &mut cursor,
        /*remaining_num*/ 3,
    );

    let remaining = ctx.remaining_accounts();
    assert_eq!(remaining.len(), 3);
    assert_eq!(
        remaining.get(0).unwrap().address().to_bytes(),
        unique_addr(2)
    );
    assert_eq!(
        remaining.get(1).unwrap().address().to_bytes(),
        unique_addr(3)
    );
    assert_eq!(
        remaining.get(2).unwrap().address().to_bytes(),
        unique_addr(4)
    );
}

#[test]
fn remaining_accounts_resolves_declared_and_trailing_duplicate_chains() {
    let records = [
        non_dup(0),
        non_dup(1),
        AccountRecord::Dup { index: 1 },
        AccountRecord::Dup { index: 2 },
        AccountRecord::Dup { index: 0 },
    ];
    let mut sbf = SbfInputBuffer::build(&records);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };
    let _ = walk::<1>(&mut cursor);

    let program_id = Address::new_from_array([0x42; 32]);
    let mut ctx: Context<'_, DummyHeader> =
        Context::new(&program_id, DummyHeader, (), &mut cursor, 4);
    let remaining = ctx.remaining_accounts();

    let addresses: Vec<_> = remaining
        .iter()
        .map(|account| account.address().to_bytes())
        .collect();
    assert_eq!(
        addresses,
        vec![
            unique_addr(1),
            unique_addr(1),
            unique_addr(1),
            unique_addr(0),
        ]
    );
    assert_eq!(
        remaining.get(2).unwrap().address().to_bytes(),
        unique_addr(1)
    );
    assert_eq!(
        remaining.get(3).unwrap().address().to_bytes(),
        unique_addr(0)
    );
}

#[test]
fn remaining_account_views_expose_compat_helpers() {
    let records = [
        non_dup(0),
        non_dup_with_data(1, 16),
        non_dup_with_data(2, 32),
    ];
    let mut sbf = SbfInputBuffer::build(&records);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };

    let _ = walk::<1>(&mut cursor);

    let program_id = Address::new_from_array([0x42; 32]);
    let mut ctx: Context<'_, DummyHeader> = Context::new(
        &program_id,
        DummyHeader,
        (),
        &mut cursor,
        /*remaining_num*/ 2,
    );

    let remaining = ctx.remaining_accounts();
    let mut account = remaining.get(0).unwrap();
    assert_eq!(account.key().to_bytes(), unique_addr(1));
    assert!(!account.data_is_empty());
    assert_eq!(account.try_data_len().unwrap(), 16);
    assert_eq!(account.try_borrow_data().unwrap().len(), 16);
    assert_eq!(account.try_borrow_mut_data().unwrap().len(), 16);
}

#[test]
fn remaining_accounts_returns_empty_when_nothing_trails() {
    let mut sbf = SbfInputBuffer::build(&[non_dup(0), non_dup(1)]);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };
    let _ = walk::<2>(&mut cursor);

    let program_id = Address::new_from_array([0x42; 32]);
    let mut ctx: Context<'_, DummyHeader> =
        Context::new(&program_id, DummyHeader, (), &mut cursor, 0);

    assert!(ctx.remaining_accounts().is_empty());
    // Second call on empty — still empty, no cache bookkeeping bug.
    assert!(ctx.remaining_accounts().is_empty());
}

#[test]
fn remaining_accounts_reuses_range_and_does_not_re_walk_cursor() {
    // If the stored range were bypassed, the second `remaining_accounts` call
    // would re-enter the cursor past its current position and read
    // garbage (or undefined behaviour) past the input buffer tail.
    let mut sbf = SbfInputBuffer::build(&[non_dup(0), non_dup(1), non_dup(2)]);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };
    let _ = walk::<1>(&mut cursor);

    let program_id = Address::new_from_array([0x42; 32]);
    let consumed_before = cursor.consumed();
    let mut ctx: Context<'_, DummyHeader> = Context::new(
        &program_id,
        DummyHeader,
        (),
        &mut cursor,
        /*remaining_num*/ 2,
    );

    let first = ctx.remaining_accounts();
    let second = ctx.remaining_accounts();

    // Structural equality via address, since each lazy view resolves the
    // same serialized account slot.
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(a.address().to_bytes(), b.address().to_bytes());
    }
    // Cursor must have advanced by exactly `remaining_num` (2), not
    // twice that — verifies the stored range short-circuits the walk.
    //
    // NB: `consumed()` reads through the `&mut AccountCursor` stored
    // inside `ctx` so we can't call it on the outer `cursor` binding
    // directly; drop `ctx` first to release the borrow.
    drop(ctx);
    assert_eq!(cursor.consumed(), consumed_before + 2);
}

#[test]
fn remaining_accounts_rescan_uses_original_boundaries_after_account_growth() {
    let program_id = Address::new_from_array([0x42; 32]);
    let records = [
        non_dup_with_data(0, 8),
        non_dup(1),
        AccountRecord::Dup { index: 0 },
        non_dup(2),
    ];
    let mut sbf = SbfInputBuffer::build(&records);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };
    let mut declared = unsafe { cursor.next() };
    declared.resize(17).unwrap();

    let mut ctx: Context<'_, DummyHeader> =
        Context::new(&program_id, DummyHeader, (), &mut cursor, 3);
    let remaining = ctx.remaining_accounts();

    assert_eq!(
        remaining.get(0).unwrap().address().to_bytes(),
        unique_addr(1)
    );
    assert_eq!(
        remaining.get(1).unwrap().address().to_bytes(),
        unique_addr(0)
    );
    assert_eq!(
        remaining.get(2).unwrap().address().to_bytes(),
        unique_addr(2)
    );
}

#[test]
fn remaining_accounts_rescan_uses_original_boundaries_after_account_shrink() {
    let program_id = Address::new_from_array([0x42; 32]);
    let mut sbf = SbfInputBuffer::build(&[non_dup_with_data(0, 17), non_dup(1), non_dup(2)]);
    let mut cursor = unsafe { AccountCursor::new(sbf.as_mut_ptr()) };
    let mut declared = unsafe { cursor.next() };
    declared.resize(3).unwrap();

    let mut ctx: Context<'_, DummyHeader> =
        Context::new(&program_id, DummyHeader, (), &mut cursor, 2);
    let addresses: Vec<_> = ctx
        .remaining_accounts()
        .iter()
        .map(|account| account.address().to_bytes())
        .collect();

    assert_eq!(addresses, vec![unique_addr(1), unique_addr(2)]);
}
