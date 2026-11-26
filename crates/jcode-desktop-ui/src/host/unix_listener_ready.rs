//! Wait for a connection or in-process shutdown without waking idle socket threads.
use std::{
    io,
    os::{
        fd::AsRawFd,
        unix::net::{UnixListener, UnixStream},
    },
};

pub(crate) fn wait(listener: &UnixListener, closing: &UnixStream) -> io::Result<bool> {
    let mut fds = [
        libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: closing.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // Both descriptors stay borrowed across this blocking call.
        let count = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if count >= 0 {
            // Prefer shutdown when both descriptors were signaled.
            return Ok(fds[1].revents == 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::Shutdown, sync::mpsc, thread, time::Duration};

    #[test]
    fn listener_wakes_only_for_a_client_or_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (stop, closing) = UnixStream::pair().unwrap();
        let (result, events) = mpsc::channel();
        thread::scope(move |threads| {
            threads.spawn(move || {
                while wait(&listener, &closing).unwrap() {
                    if result.send(listener.accept().is_ok()).is_err() {
                        return;
                    }
                }
                let _ = result.send(false);
            });
            assert!(matches!(
                events.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            let _client = UnixStream::connect(path).unwrap();
            assert!(events.recv_timeout(Duration::from_secs(2)).unwrap());
            stop.shutdown(Shutdown::Write).unwrap();
            assert!(!events.recv_timeout(Duration::from_secs(2)).unwrap());
        });
    }
}
