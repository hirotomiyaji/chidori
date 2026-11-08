//! macOS audio through Core Audio (via `cpal`): the mic, and the whole system through a process
//! tap on the default output (macOS 14.6+). One app's sound on its own is not done yet.

pub mod capture;
pub mod devices;
