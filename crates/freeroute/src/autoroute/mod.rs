//! The autorouter: FreeRouting's maze search over rooms and doors, and
//! what drives it. Ported from `app.freerouting.autoroute`.

pub mod control;
pub mod distance;
pub mod engine;
pub mod locate;
pub mod maze;
pub mod shove;

pub use control::{Control, ViaMask};
pub use distance::DestinationDistance;
