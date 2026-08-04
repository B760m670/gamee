use thiserror::Error;

#[derive(Debug, Error)]
pub enum P2pError {
    #[error("invalid identity seed: {0}")]
    InvalidSeed(String),

    #[error("failed to build the network transport/behaviour: {0}")]
    Setup(String),

    #[error("failed to listen on {addr}: {source}")]
    Listen {
        addr: String,
        source: libp2p::TransportError<std::io::Error>,
    },

    #[error("failed to dial {peer}: {source}")]
    Dial {
        peer: String,
        source: libp2p::swarm::DialError,
    },

    #[error("the P2P event loop task is no longer running")]
    NodeShutDown,

    #[error("@username ledger storage error: {0}")]
    Ledger(String),

    #[error("mailbox error: {0}")]
    Mailbox(String),

    /// A contact ticket was malformed, unsigned, stale, addressed elsewhere,
    /// or did not carry enough work — see `crate::ticket`.
    #[error("contact ticket rejected: {0}")]
    Ticket(String),

    #[error("mix routing error: {0}")]
    Mix(String),
}

impl From<spiritchat_ledger_core::LedgerError> for P2pError {
    fn from(err: spiritchat_ledger_core::LedgerError) -> Self {
        P2pError::Ledger(err.to_string())
    }
}

pub type Result<T> = core::result::Result<T, P2pError>;
