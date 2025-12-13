//! # Error Definitions
//!
//! The central ledger of all operational and protocol failures.

use neopack::Error as NeoError;


/// Operational failures within the RPC mechanism itself.
#[derive(Debug, Clone)]
pub enum RpcError {
    /// The underlying Neopack serialization failed (e.g., buffer exhaustion).
    Serialization(NeoError),
    /// The wire types did not match the expected Wasmtime types.
    TypeMismatch { expected: String, found: String },
    /// A record was missing a required field.
    MissingField(String),
    /// An unknown enum variant, flag, or top-level frame type was encountered.
    UnknownVariant(String),
    /// The internal structure of the message was malformed (e.g., missing Sequence header).
    ProtocolViolation(String),
    /// Attempted to encode/decode a type not supported by RPC (Resource, Future, Stream).
    UnsupportedType(String),
    /// The nested depth of the values exceeded the safety limit.
    RecursionLimitExceeded,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for RpcError {}

impl From<NeoError> for RpcError {
    fn from(e: NeoError) -> Self { Self::Serialization(e) }
}

/// A specialized Result type for RPC operations.
pub type Result<T> = std::result::Result<T, RpcError>;

/// Reasons for an RPC failure (The "Err" side of a Reply).
///
/// These are distinct from `RpcError`; these represent the *remote* system failing,
/// whereas `RpcError` represents the *transport* failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureReason {
    /// The component explicitly trapped (panic/abort).
    AppTrapped,
    /// Execution exhausted the fuel budget.
    OutOfFuel,
    /// Execution exceeded memory limits.
    OutOfMemory,
    /// The target instance ID was not found.
    InstanceNotFound,
    /// The method does not exist on the instance.
    MethodNotFound,
    /// Arguments provided did not match the method signature.
    BadArgumentCount,
    /// The RPC frame was malformed.
    ProtocolViolation(String),
}
