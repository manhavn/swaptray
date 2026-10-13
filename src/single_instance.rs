//! Single-instance guard using an abstract Unix socket (released by the
//! kernel when the process exits).

use std::io::ErrorKind;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener};

pub struct Lock {
    // Keeps the socket bound for the process lifetime.
    _listener: Option<UnixListener>,
}

/// `Some(lock)` if this process now owns the instance slot (or locking is
/// unavailable, in which case we log and continue); `None` if another
/// instance is already running.
pub fn acquire() -> Option<Lock> {
    let addr = match SocketAddr::from_abstract_name(b"swaptray-single-instance") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[swaptray] cannot build instance lock address: {e}");
            return Some(Lock { _listener: None });
        }
    };
    match UnixListener::bind_addr(&addr) {
        Ok(listener) => Some(Lock {
            _listener: Some(listener),
        }),
        Err(e) if e.kind() == ErrorKind::AddrInUse => None,
        Err(e) => {
            eprintln!("[swaptray] cannot bind instance lock: {e}");
            Some(Lock { _listener: None })
        }
    }
}
