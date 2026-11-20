//! Everything that produces a category reading.

pub mod battery;
pub mod kernel;
pub mod smc_categories;

pub use kernel::SystemSensors;
pub use smc_categories::{fan, temperature, voltage};
