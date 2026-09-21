#![doc = include_str!("../README.md")]

#![deny(clippy::unwrap_used)]
#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod ui;

#[cfg(feature = "atspi")]
mod atspi;
#[cfg(feature = "mcp")]
mod mcp;

pub use ui::Molerat;
