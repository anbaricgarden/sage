pub mod applicator;
pub mod format;
pub mod parser;

pub use format::{EditBlock, DiffError, DiffResult};
pub use applicator::apply_diff;
