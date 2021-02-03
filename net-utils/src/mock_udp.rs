pub use udp_socket::*;

#[cfg(not(feature = "mock-udp"))]
mod udp_socket {
    pub use socket2::Socket;
    pub use std::net::UdpSocket;
}

#[cfg(feature = "mock-udp")]
mod udp_socket {
    use crossbeam_channel::{unbounded, Receiver, Sender};
    use lazy_static::lazy_static;
    use log::*;
    use std::collections::HashMap;
    use std::io::Result;
    use std::net::{SocketAddr, ToSocketAddrs};
    use std::sync::{Arc, Mutex, RwLock};
    use thiserror::Error;

    lazy_static! {
        static ref SWITCH: Arc<Mutex<UdpSwitch>> = Arc::new(Mutex::new(UdpSwitch::new()));
    }
    #[derive(Default)]
    pub struct Socket(RwLock<Option<UdpSocketInside>>, bool);

    impl Socket {
        pub fn new(reuseaddr: bool) -> Self {
            Socket(Default::default(), reuseaddr)
        }
        pub fn bind(&self, addr: &socket2::SockAddr) -> Result<()> {
            let addr = addr.as_inet().unwrap();
            let port = addr.port();
            let localhost = addr.ip().is_loopback();
            let reuseaddr = self.1;
            let mut guard = self.0.write().unwrap();
            let inside = &mut *guard;

            let mut switch = SWITCH.lock().unwrap();

            match switch.bind(port, reuseaddr) {
                Ok((port, receiver)) => {
                    if let Some(inside) = inside {
                        inside.port = port;
                        inside.receiver = receiver;
                    } else {
                        *inside = Some(UdpSocketInside {
                            port,
                            connected_port:0,
                            localhost,
                            receiver,
                            nonblocking: false,
                            read_timeout: None,
                        });
                    }
                    Ok(())
                }
                Err(e) => Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, e)),
            }
        }

        pub fn into_udp_socket(self) -> UdpSocket {
            UdpSocket(self.0)
        }
    }

    fn port_to_sockaddr(port: u16, localhost: bool) -> SocketAddr {
        let addr = std::net::IpAddr::V4(if localhost {
            std::net::Ipv4Addr::new(127, 0, 0, 1)
        } else {
            std::net::Ipv4Addr::new(0, 0, 0, 0)
        });
        SocketAddr::new(addr, port)
    }

    #[derive(Debug, Default)]
    pub struct UdpSocket(RwLock<Option<UdpSocketInside>>);

    impl UdpSocket {
        pub fn new() -> UdpSocket {
            UdpSocket::default()
        }
        pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<UdpSocket> {
            // TODO: should we just loop through until one works?
            let addr = addr.to_socket_addrs()?.collect::<Vec<SocketAddr>>();
            let sock = Socket::new(false);
            sock.bind(&addr[0].into()).map(|_| sock.into_udp_socket())
        }

        pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<()> {
            let addrs = addr.to_socket_addrs()?.collect::<Vec<SocketAddr>>();

            let mut guard = self.0.write().unwrap();
            if let Some(inside) = &mut *guard {
                inside.connected_port = addrs[0].port();
                Ok(())
            } else {
                Err(UdpSwitchError::Unbound.into())
            }
        }

        pub fn local_addr(&self) -> Result<SocketAddr> {
            let guard = self.0.read().unwrap();
            let inside = &*guard;
            if let Some(inside) = inside {
                Ok(port_to_sockaddr(inside.port, inside.localhost))
            } else {
                Err(UdpSwitchError::Unbound.into())
            }
        }

        fn connected_port(&self) -> Result<u16> {
            let guard = self.0.read().unwrap();
            let inside = &*guard;
            if let Some(inside) = inside {
                Ok(inside.connected_port)
            } else {
                Err(UdpSwitchError::Unbound.into())
            }
        }

        pub fn send(&self, buf: &[u8]) -> Result<usize> {
            let dest_port = self.connected_port()?;

            let from = self.local_addr()?.port();
            let msg = UdpMessage {
                from,
                bytes: buf.to_vec(),
            };

            let switch = SWITCH.lock().unwrap();
            let dest = switch.get_port(from, dest_port);
            if let Some(dest) = dest {
                let _ = dest.send(msg);
            }
            Ok(buf.len())
        }

        pub fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> Result<usize> {
            // TODO: should we just loop through until one works?
            let from = self.local_addr()?.port();

            let switch = SWITCH.lock().unwrap();

            let addrs = addr.to_socket_addrs()?.collect::<Vec<SocketAddr>>();
            if addrs.len() > 1 {
                trace!("multiple addresses in send: {:#?}", addrs);
            }
            let msgbuf = buf.to_vec();
            for addr in addrs {
                let dest_port = addr.port();

                let dest = switch.get_port(from, dest_port);
                let msg = UdpMessage {
                    from,
                    bytes: msgbuf.clone(),
                };
                if let Some(dest) = dest {
                    let _ = dest.send(msg);
                } else {
                    trace!("send to unknown dest: {:#?}", addr);
                }
            }
            Ok(buf.len())
        }

        pub fn try_clone(&self) -> Result<UdpSocket> {
            Ok(self.clone())
        }

        pub fn read_timeout(&self) -> Result<Option<std::time::Duration>> {
            Ok(self
                .0
                .read()
                .unwrap()
                .as_ref()
                .and_then(|sock| sock.read_timeout))
        }

        pub fn set_read_timeout(&self, dur: Option<std::time::Duration>) -> Result<()> {
            let mut guard = self.0.write().unwrap();
            if let Some(inside) = &mut *guard {
                inside.read_timeout = dur;
                Ok(())
            } else {
                Err(UdpSwitchError::Unbound.into())
            }
        }

        pub fn recv(&self, buf: &mut [u8]) -> Result<usize> {
            self.recv_from(buf).map(|(size, _addr)| size)
        }

        fn clone_inside(&self) -> Option<UdpSocketInside> {
            let guard = self.0.read().unwrap();
            guard.as_ref().cloned()
        }

        pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
            let inside = self.clone_inside();

            if let Some(inside) = inside {
                inside.recv(buf)
            } else {
                Err(UdpSwitchError::Unbound.into())
            }
        }

        pub fn set_nonblocking(&self, nonblocking: bool) -> Result<()> {
            let mut guard = self.0.write().unwrap();
            if let Some(inside) = &mut *guard {
                inside.nonblocking = nonblocking;
                Ok(())
            } else {
                Err(UdpSwitchError::Unbound.into())
            }
        }
    }
    impl Clone for UdpSocket {
        fn clone(&self) -> Self {
            let guard = self.0.read().unwrap();
            let inside = &*guard;
            match inside {
                Some(inside) => Self(RwLock::new(Some(inside.clone()))),
                None => Self(RwLock::new(None)),
            }
        }
    }

    #[derive(Debug)]
    struct UdpSocketInside {
        port: u16,
        connected_port: u16,
        localhost: bool,
        receiver: Receiver<UdpMessage>,
        nonblocking: bool,
        read_timeout: Option<std::time::Duration>,
    }

    impl UdpSocketInside {
        fn recv(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
            let recv_msg = || -> std::result::Result<UdpMessage, UdpSwitchError> {
                Ok(match (self.nonblocking, self.read_timeout) {
                    // no timeout, normal receive
                    (false, None) => self.receiver.recv()?,
                    // receive with timeout
                    (false, Some(timeout)) => self.receiver.recv_timeout(timeout)?,
                    (true, _) => self.receiver.try_recv()?,
                })
            };
            let msg = recv_msg()?;
            let len = std::cmp::min(msg.bytes.len(), buf.len());
            let dest = &mut buf[0..len];
            dest.copy_from_slice(&msg.bytes[0..len]);
            Ok((len, port_to_sockaddr(msg.from, true)))
        }
    }

    impl Drop for UdpSocketInside {
        fn drop(&mut self) {
            SWITCH.lock().unwrap().close(self.port)
        }
    }

    impl Clone for UdpSocketInside {
        fn clone(&self) -> Self {
            SWITCH.lock().unwrap().clone(self.port);
            Self {
                port: self.port,
                connected_port: self.connected_port,
                localhost: self.localhost,
                receiver: self.receiver.clone(),
                nonblocking: self.nonblocking,
                read_timeout: self.read_timeout,
            }
        }
    }

    struct UdpMessage {
        from: u16,
        bytes: Vec<u8>,
    }

    const NO_PORT: u16 = 0;
    const MIN_PORT: u16 = 1024;
    const MAX_PORT: u16 = 65535;

    #[derive(Error, Debug)]
    enum UdpSwitchError {
        #[error("port in use")]
        PortInUse,
        #[error("switch full")]
        Full,
        #[error("unbound socket")]
        Unbound,
        #[error("receive error")]
        RecvError {
            #[from]
            source: crossbeam_channel::RecvError,
        },
        #[error("receive timeout")]
        RecvTimeoutError {
            #[from]
            source: crossbeam_channel::RecvTimeoutError,
        },
        #[error("would block")]
        TryRecvError {
            #[from]
            source: crossbeam_channel::TryRecvError,
        },
    }

    use std::io::ErrorKind;
    impl From<UdpSwitchError> for std::io::Error {
        fn from(e: UdpSwitchError) -> Self {
            let kind = match e {
                UdpSwitchError::PortInUse => ErrorKind::AddrNotAvailable,
                UdpSwitchError::Full => ErrorKind::AddrNotAvailable,
                UdpSwitchError::Unbound => ErrorKind::AddrNotAvailable,
                UdpSwitchError::RecvError { .. } => ErrorKind::NotConnected,
                UdpSwitchError::RecvTimeoutError { .. } => ErrorKind::TimedOut,
                UdpSwitchError::TryRecvError { .. } => ErrorKind::WouldBlock,
            };
            std::io::Error::new(kind, e)
        }
    }

    struct SenderEntry {
        sender: Sender<UdpMessage>,
        ref_count: u16,
    }

    #[derive(Default)]
    struct UdpSwitch {
        senders: HashMap<u16, SenderEntry>,
    }

    impl UdpSwitch {
        fn new() -> Self {
            Default::default()
        }

        fn bind(
            &mut self,
            port: u16,
            reuseaddr: bool,
        ) -> std::result::Result<(u16, Receiver<UdpMessage>), UdpSwitchError> {
            let mut bound_port = port;
            if bound_port == NO_PORT {
                bound_port = MIN_PORT;
                while bound_port < MAX_PORT {
                    if !self.senders.contains_key(&bound_port) {
                        break;
                    }
                    bound_port += 1;
                }
                if bound_port == MAX_PORT {
                    return Err(UdpSwitchError::Full);
                }
            } else if self.senders.contains_key(&port) && !reuseaddr {
                return Err(UdpSwitchError::PortInUse);
            }
            let (tx, rx) = unbounded();
            self.senders.insert(
                bound_port,
                SenderEntry {
                    sender: tx,
                    ref_count: 1,
                },
            );
            Ok((bound_port, rx))
        }

        fn get_port(&self, _from: u16, to: u16) -> Option<Sender<UdpMessage>> {
            // TODO: inject checking connectivity is allowed here
            self.senders.get(&to).map(|entry| entry.sender.clone())
        }

        fn close(&mut self, port: u16) {
            let entry = self.senders.get_mut(&port);
            if let Some(entry) = entry {
                entry.ref_count -= 1;
                if entry.ref_count == 0 {
                    self.senders.remove(&port);
                }
            }
        }

        fn clone(&mut self, port: u16) {
            let entry = self.senders.get_mut(&port);
            if let Some(entry) = entry {
                entry.ref_count += 1;
            }
        }
    }
}
