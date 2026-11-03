//! Live config updates: the packed config travels as the state of a Darwin notification, which
//! every loaded payload watches. Works for sandboxed apps and needs no root.

use std::ffi::CStr;

use eris_inject::notify::Channel;

pub const NAME: &CStr = c"io.github.eriskii.sharpenerer.config";

/// A registration for the config notification. Holding one keeps its state alive.
pub fn channel() -> Result<Channel, String> {
    Channel::open(NAME)
}
