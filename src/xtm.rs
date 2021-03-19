use crate::ffi::tarantool as ffi;
use crate::error::{Error, TarantoolError};

use std::os::unix::io::{AsRawFd, RawFd};
use std::ffi::c_void;


/// One-directional, one-reader-one-writer queue
pub struct Queue {
    inner: *mut ffi::XTMQueue,
}

impl Queue {
    pub fn new(size: u32) -> Result<Self, Error> {
        let q = unsafe { ffi::xtm_create(size) };
        if q.is_null() {
            return Err(TarantoolError::last().into());
        }
        Ok(Self { inner: q })
    }

    pub fn delete(&mut self) -> Result<(), Error> {
        let result = unsafe { ffi::xtm_delete(self.inner) };
        if result < 0 {
            return Err(TarantoolError::last().into());
        }
        Ok(())
    }

    pub fn msg_notify(&mut self) -> Result<(), Error> {
        let result = unsafe { ffi::xtm_msg_notify(self.inner) };
        if result < 0 {
            return Err(TarantoolError::last().into());
        }
        Ok(())
    }

    pub fn msg_probe(&mut self) -> Result<(), Error> {
        let result = unsafe { ffi::xtm_msg_probe(self.inner) };
        if result < 0 {
            return Err(TarantoolError::last().into());
        }
        Ok(())
    }

    pub fn msg_count(&mut self) -> u32 {
        unsafe { ffi::xtm_msg_count(self.inner) }
    }

    pub fn msg_send(&mut self, msg: *mut c_void, delayed: bool) -> Result<(), Error> {
        let result = unsafe {
            ffi::xtm_msg_send(
                self.inner,
                msg,
                delayed.into(),
            )
        };
        if result < 0 {
            return Err(TarantoolError::last().into());
        }
        Ok(())
    }
}

impl AsRawFd for Queue {
    fn as_raw_fd(&self) -> RawFd {
        unsafe { ffi::xtm_fd(self.inner) }
    }
}
