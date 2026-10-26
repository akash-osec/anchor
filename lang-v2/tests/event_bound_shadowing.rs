use anchor_lang::{event, AnchorDeserialize, AnchorSerialize, Event};

#[derive(anchor_lang::IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct Address {
    pub bytes: [u8; 64],
}

#[event]
pub struct ShadowedAddressEvent {
    pub address: Address,
}

#[test]
fn shadowed_address_uses_the_conservative_event_capacity() {
    let event = ShadowedAddressEvent {
        address: Address { bytes: [7; 64] },
    };

    assert!(Event::__emit(&event).is_ok());
}
