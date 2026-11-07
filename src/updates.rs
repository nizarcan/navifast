//! Update handoff compatibility and the inherited demo updater.
#[cfg(feature = "demo")]
mod legacy;
#[cfg(feature = "demo")]
pub use legacy::*;
#[cfg(not(feature = "demo"))]
mod native;
#[cfg(not(feature = "demo"))]
pub use native::*;
