//! The loopback listener of the proxy (and of the host's stand-in that refuses everything).
//!
//! It binds `127.0.0.1:0`. On Windows it is bound with `SO_EXCLUSIVEADDRUSE`, so no other
//! process can bind the same port (with `SO_REUSEADDR` or a more specific address) and take
//! connections meant for Krunk's proxy while the engine points at it.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};

/// A listening socket on `127.0.0.1:<random>` that no other process can share.
pub fn bind_loopback() -> io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    #[cfg(windows)]
    exclusive(&socket)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::LOCALHOST, 0)).into())?;
    socket.listen(1024)?;
    Ok(socket.into())
}

/// `SO_EXCLUSIVEADDRUSE` (before `bind`): the port is this socket's alone.
#[cfg(windows)]
#[allow(unsafe_code)]
fn exclusive(socket: &socket2::Socket) -> io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, WSAGetLastError, SOCKET, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
    };
    let on: i32 = 1;
    // SAFETY: a valid socket handle owned by `socket`; the option value is a 4-byte BOOL that
    // lives for the call.
    let r = unsafe {
        setsockopt(
            socket.as_raw_socket() as SOCKET,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            (&on as *const i32).cast(),
            std::mem::size_of::<i32>() as i32,
        )
    };
    if r != 0 {
        // SAFETY: reads the calling thread's last WinSock error.
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn binds_loopback_on_a_random_port() {
        let a = super::bind_loopback().unwrap();
        let b = super::bind_loopback().unwrap();
        let (aa, ba) = (a.local_addr().unwrap(), b.local_addr().unwrap());
        assert!(aa.ip().is_loopback() && aa.port() != 0);
        assert_ne!(aa.port(), ba.port());
        let c = std::net::TcpStream::connect(aa).unwrap();
        let (s, peer) = a.accept().unwrap();
        assert_eq!(peer, c.local_addr().unwrap());
        assert_eq!(s.local_addr().unwrap(), aa);
    }
}
