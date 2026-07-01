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
}

pub type Result<T> = core::result::Result<T, P2pError>;
