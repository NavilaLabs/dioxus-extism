use dioxus::prelude::*;
use dioxus_extism_protocol::{ClientCapabilities, OverrideMap, SessionId, SlotContent};

/// Fetch the current `OverrideMap` at boot time.
#[server]
pub async fn get_override_map(
    _caps: ClientCapabilities,
) -> Result<OverrideMap, ServerFnError> {
    Ok(OverrideMap::default())
}

/// Fetch slot contributions for a named slot.
#[server]
pub async fn get_slot_content(
    _slot: String,
    _session_id: SessionId,
    _caps: ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError> {
    Ok(vec![])
}
