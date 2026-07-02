use std::os::fd::AsRawFd;

thread_local! {
    static PIPE: std::cell::RefCell<Option<(std::os::fd::RawFd, std::os::fd::RawFd)>> = std::cell::RefCell::new(None);
}

#[allow(dead_code)]
fn get_pipe() -> std::io::Result<(std::os::fd::RawFd, std::os::fd::RawFd)> {
    PIPE.with(|p| {
        if let Some(pipe) = *p.borrow() {
            return Ok(pipe);
        }
        let mut fds = [0; 2];
        let res = unsafe {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK)
            }
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            {
                let r = libc::pipe(fds.as_mut_ptr());
                if r == 0 {
                    libc::fcntl(fds[0], libc::F_SETFL, libc::O_NONBLOCK);
                    libc::fcntl(fds[1], libc::F_SETFL, libc::O_NONBLOCK);
                    libc::fcntl(fds[0], libc::F_SETFD, libc::FD_CLOEXEC);
                    libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC);
                }
                r
            }
        };
        if res < 0 {
            return Err(std::io::Error::last_os_error());
        }
        *p.borrow_mut() = Some((fds[0], fds[1]));
        Ok((fds[0], fds[1]))
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn sendfile_impl(
    socket_fd: std::os::fd::RawFd,
    file_fd: std::os::fd::RawFd,
    offset: &mut u64,
    count: u64,
) -> std::io::Result<u64> {
    let mut off = *offset as libc::off_t;
    let res = unsafe {
        libc::sendfile(
            socket_fd,
            file_fd,
            &mut off,
            count as libc::size_t,
        )
    };
    if res < 0 {
        let err = std::io::Error::last_os_error();
        Err(err)
    } else {
        *offset = off as u64;
        Ok(res as u64)
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn splice_impl(
    socket_fd: std::os::fd::RawFd,
    file_fd: std::os::fd::RawFd,
    offset: &mut u64,
    count: u64,
) -> std::io::Result<u64> {
    let (pipe_read, pipe_write) = get_pipe()?;
    let mut off = *offset as libc::off_t;

    // 1. Splice from file to pipe write end
    let spliced_in = unsafe {
        libc::splice(
            file_fd,
            &mut off,
            pipe_write,
            std::ptr::null_mut(),
            count as libc::size_t,
            libc::SPLICE_F_MOVE | libc::SPLICE_F_NONBLOCK,
        )
    };
    if spliced_in < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if spliced_in == 0 {
        return Ok(0);
    }

    // 2. Splice from pipe read end to socket
    let mut total_spliced_out = 0;
    let mut to_splice_out = spliced_in;
    while to_splice_out > 0 {
        let spliced_out = unsafe {
            libc::splice(
                pipe_read,
                std::ptr::null_mut(),
                socket_fd,
                std::ptr::null_mut(),
                to_splice_out as libc::size_t,
                libc::SPLICE_F_MOVE | libc::SPLICE_F_NONBLOCK,
            )
        };
        if spliced_out < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                // Poll wait for socket writability
                let mut pollfd = libc::pollfd {
                    fd: socket_fd,
                    events: libc::POLLOUT,
                    revents: 0,
                };
                unsafe { libc::poll(&mut pollfd, 1, -1); }
                continue;
            }
            return Err(err);
        }
        to_splice_out -= spliced_out;
        total_spliced_out += spliced_out;
    }

    *offset = off as u64;
    Ok(total_spliced_out as u64)
}

#[cfg(target_os = "macos")]
pub fn sendfile_impl(
    socket_fd: std::os::fd::RawFd,
    file_fd: std::os::fd::RawFd,
    offset: &mut u64,
    count: u64,
) -> std::io::Result<u64> {
    let mut len = count as libc::off_t;
    let res = unsafe {
        libc::sendfile(
            file_fd,
            socket_fd,
            *offset as libc::off_t,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if res < 0 {
        let err = std::io::Error::last_os_error();
        if len > 0 {
            *offset += len as u64;
            return Ok(len as u64);
        }
        Err(err)
    } else {
        *offset += len as u64;
        Ok(len as u64)
    }
}

/// Retry limit for sendfile EAGAIN before falling back.
const SEND_RETRIES: usize = 16;
/// Microseconds to sleep between sendfile EAGAIN retries.
const RETRY_BACKOFF_US: u64 = 200;

pub async fn try_zero_copy_chunk(
    file: &std::fs::File,
    socket: &tokio::net::TcpStream,
    offset: &mut u64,
    count: u64,
) -> std::io::Result<u64> {
    let file_fd = file.as_raw_fd();
    let socket_fd = socket.as_raw_fd();

    // Wait for writability on socket first to prevent WouldBlock
    socket.writable().await?;

    let mut retries = 0usize;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        loop {
            match sendfile_impl(socket_fd, file_fd, offset, count) {
                Ok(n) => return Ok(n),
                Err(e) => {
                    let code = e.raw_os_error().unwrap_or(0);
                    if code == libc::EINVAL || code == libc::ENOSYS || code == libc::EOPNOTSUPP {
                        return splice_impl(socket_fd, file_fd, offset, count);
                    }
                    if code == libc::EAGAIN || code == libc::EWOULDBLOCK {
                        retries += 1;
                        if retries >= SEND_RETRIES {
                            return Err(e);
                        }
                        // Wait for socket writability before retry
                        let _ = socket.writable().await;
                        tokio::time::sleep(std::time::Duration::from_micros(RETRY_BACKOFF_US)).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        loop {
            match sendfile_impl(socket_fd, file_fd, offset, count) {
                Ok(n) => return Ok(n),
                Err(e) => {
                    let code = e.raw_os_error().unwrap_or(0);
                    if code == libc::EAGAIN || code == libc::EWOULDBLOCK {
                        retries += 1;
                        if retries >= SEND_RETRIES {
                            return Err(e);
                        }
                        let _ = socket.writable().await;
                        tokio::time::sleep(std::time::Duration::from_micros(RETRY_BACKOFF_US)).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    {
        let _ = retries;
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "zero-copy unsupported on this platform"))
    }
}
