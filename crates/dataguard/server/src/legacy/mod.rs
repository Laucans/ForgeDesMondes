//! The legacy submit chain, one module per DataCapability it still
//! prepares before the engine queues it. A DataCapability leaves the chain
//! when its `Prepare` moves to its crate: its module and its arm in
//! [`crate::submit`] are deleted together, and its submissions fall through
//! to the engine. The separators keep each deletion a hunk of its own.

// ---
pub(crate) mod export_campaign;
// ---
