//! Live config updates: the packed config travels as the state of a Darwin notification, which
//! every loaded payload watches. Works for sandboxed apps and needs no root.

use std::ffi::{CStr, c_char, c_int};

pub const NAME: &CStr = c"io.github.eriskii.sharpenerer.config";

unsafe extern "C" {
    fn notify_register_check(name: *const c_char, token: *mut c_int) -> u32;
    fn notify_set_state(token: c_int, state: u64) -> u32;
    fn notify_get_state(token: c_int, state: *mut u64) -> u32;
    fn notify_post(name: *const c_char) -> u32;
    fn notify_cancel(token: c_int) -> u32;
}

/// A registration for the config notification. Holding one keeps its state alive.
pub struct Channel(c_int);

impl Channel {
    pub fn open() -> Result<Self, String> {
        let mut token = 0;
        match unsafe { notify_register_check(NAME.as_ptr(), &mut token) } {
            0 => Ok(Channel(token)),
            status => Err(format!("can't register for config notifications (status {status})")),
        }
    }

    /// Stores `packed` and tells every payload to pick it up.
    pub fn publish(&self, packed: u64) -> Result<(), String> {
        match unsafe { (notify_set_state(self.0, packed), notify_post(NAME.as_ptr())) } {
            (0, 0) => Ok(()),
            (a, b) => Err(format!("can't publish config (status {a}/{b})")),
        }
    }

    pub fn current(&self) -> Option<u64> {
        let mut state = 0;
        (unsafe { notify_get_state(self.0, &mut state) } == 0 && state != 0).then_some(state)
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        unsafe { notify_cancel(self.0) };
    }
}
