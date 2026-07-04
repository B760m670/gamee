//! Group messaging: **Sender Keys**, the scheme Signal and WhatsApp both
//! use in production for groups — not a novel invention, and deliberately
//! not full MLS/TreeKEM (a much larger undertaking this project isn't
//! taking on). See `state.rs`'s own doc comment for the scheme itself and
//! exactly what it does and doesn't buy.

mod distribution;
mod header;
mod skipped;
mod state;

pub use distribution::SenderKeyDistribution;
pub use header::SenderKeyHeader;
pub use state::{SenderKeyReceiverState, SenderKeyState};
