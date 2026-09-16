use anchor_lang_idl::{convert::convert_idl_to_legacy, types::Idl};

fn current_idl(seeds: Vec<serde_json::Value>) -> Idl {
    serde_json::from_value(serde_json::json!({
        "address": "11111111111111111111111111111111",
        "metadata": {
            "name": "seed_types",
            "version": "0.1.0",
            "spec": "0.1.0"
        },
        "instructions": [{
            "name": "useSeeds",
            "discriminator": [1, 2, 3, 4, 5, 6, 7, 8],
            "accounts": [{
                "name": "state",
                "pda": { "seeds": seeds }
            }],
            "args": [
                { "name": "nonce", "type": "u64" },
                { "name": "label", "type": "string" },
                { "name": "blob", "type": "bytes" },
                { "name": "authority", "type": "pubkey" },
                { "name": "details", "type": { "defined": { "name": "Inner" } } }
            ]
        }],
        "accounts": [{ "name": "State", "discriminator": [0, 0, 0, 0, 0, 0, 0, 0] }],
        "types": [{
            "name": "State",
            "type": {
                "kind": "struct",
                "fields": [
                    { "name": "nonce", "type": "u64" },
                    { "name": "nested", "type": { "defined": { "name": "Inner" } } }
                ]
            }
        }, {
            "name": "Inner",
            "type": {
                "kind": "struct",
                "fields": [{ "name": "label", "type": "string" }]
            }
        }]
    }))
    .unwrap()
}

fn legacy_json(current: &Idl) -> serde_json::Value {
    serde_json::from_slice(&convert_idl_to_legacy(current).expect("current -> legacy conversion"))
        .unwrap()
}

#[test]
fn argument_seed_type_is_preserved() {
    let mut current = current_idl(vec![serde_json::json!({
        "kind": "arg",
        "path": "nonce"
    })]);
    let anchor_lang_idl::types::IdlInstructionAccountItem::Single(account) =
        &mut current.instructions[0].accounts[0]
    else {
        panic!("expected a single account");
    };
    account.pda.as_mut().unwrap().program = Some(anchor_lang_idl::types::IdlSeed::Arg(
        anchor_lang_idl::types::IdlSeedArg {
            path: "nonce".into(),
        },
    ));
    let legacy = legacy_json(&current);

    assert_eq!(
        legacy["instructions"][0]["accounts"][0]["pda"]["seeds"][0]["type"],
        "u64"
    );
    assert_eq!(
        legacy["instructions"][0]["accounts"][0]["pda"]["programId"]["type"],
        "u64"
    );
}

#[test]
fn account_field_seed_type_is_preserved() {
    let current = current_idl(vec![serde_json::json!({
        "kind": "account",
        "account": "State",
        "path": "state.nonce"
    })]);
    let legacy = legacy_json(&current);

    assert_eq!(
        legacy["instructions"][0]["accounts"][0]["pda"]["seeds"][0]["type"],
        "u64"
    );
}

#[test]
fn nested_and_scalar_seed_types_are_preserved() {
    let current = current_idl(vec![
        serde_json::json!({ "kind": "arg", "path": "nonce" }),
        serde_json::json!({ "kind": "arg", "path": "label" }),
        serde_json::json!({ "kind": "arg", "path": "blob" }),
        serde_json::json!({ "kind": "arg", "path": "authority" }),
        serde_json::json!({ "kind": "arg", "path": "details.label" }),
        serde_json::json!({
            "kind": "account",
            "account": "State",
            "path": "state.nested.label"
        }),
        serde_json::json!({ "kind": "account", "path": "state" }),
    ]);
    let legacy = legacy_json(&current);
    let seeds = &legacy["instructions"][0]["accounts"][0]["pda"]["seeds"];

    assert_eq!(seeds[0]["type"], "u64");
    assert_eq!(seeds[1]["type"], "string");
    assert_eq!(seeds[2]["type"], "bytes");
    assert_eq!(seeds[3]["type"], "publicKey");
    assert_eq!(seeds[4]["type"], "string");
    assert_eq!(seeds[5]["type"], "string");
    assert_eq!(seeds[6]["type"], "publicKey");
}

#[test]
fn unknown_argument_seed_path_is_rejected() {
    let current = current_idl(vec![serde_json::json!({
        "kind": "arg",
        "path": "missing"
    })]);

    assert!(convert_idl_to_legacy(&current).is_err());
}

#[test]
fn unknown_account_field_seed_path_is_rejected() {
    let current = current_idl(vec![serde_json::json!({
        "kind": "account",
        "account": "State",
        "path": "state.missing"
    })]);

    assert!(convert_idl_to_legacy(&current).is_err());
}

#[test]
fn account_field_seed_without_type_is_rejected() {
    let current = current_idl(vec![serde_json::json!({
        "kind": "account",
        "path": "state.nonce"
    })]);

    assert!(convert_idl_to_legacy(&current).is_err());
}
