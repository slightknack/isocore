// crates/isorpc/src/types.rs
use std::fmt;
use std::error;
use isopack::types::Error as IsoError;
use isopack::ValueDecoder;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// An error occurred within the underlying serialization layer.
    Iso(IsoError),
    /// The decoded value did not match the expected Wasm Type.
    TypeMismatch {
        expected: String,
        got: String,
    },
    /// The message structure was invalid or violated the protocol.
    Malformed(String),
    /// The requested function name is not known to the decoder.
    UnknownFunction(String),
    /// The remote execution failed (Host Trap).
    Remote(String),
}

impl From<IsoError> for Error {
    fn from(err: IsoError) -> Self {
        Error::Iso(err)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Iso(e) => write!(f, "Isopack error: {}", e),
            Error::TypeMismatch { expected, got } => write!(f, "Type mismatch: expected {}, got {}", expected, got),
            Error::Malformed(msg) => write!(f, "Malformed data: {}", msg),
            Error::UnknownFunction(name) => write!(f, "Unknown function: {}", name),
            Error::Remote(msg) => write!(f, "Remote error: {}", msg),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Iso(e) => Some(e),
            _ => None,
        }
    }
}

/// The wire format envelope.
/// Top-level messages are encoded as Variants:
/// - "call" -> [seq, method, [args...]]
/// - "resp" -> [seq, Result<[vals...], string>]
pub enum MessageHeader<'a> {
    Call {
        seq: u64,
        method: &'a str,
        // The raw decoder positioned at the start of the arguments list
        args_decoder: ValueDecoder<'a>,
    },
    Response {
        seq: u64,
        // The raw decoder positioned at the result (Ok/Err variant)
        result_decoder: ValueDecoder<'a>,
    },
}
