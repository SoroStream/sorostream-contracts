use soroban_sdk::contracterror;

/// Typed errors returned by streaming contract queries and maintenance calls.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum StreamingError {
    StreamNotFound = 1,
    ProposalNotFound = 2,
}
