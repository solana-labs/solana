//! Small services library with named ports
//! see test for usage

use std::sync::{Arc, Mutex, RwLock};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use streamer;
use result::Result;
use result::Error;

pub enum Port {
    Main,
    PacketReader,
    Accountant,
    PacketSender,
}

impl Port {
    fn to_usize(self) -> usize {
        match self {
            Port::Main => 0,
            Port::PacketReader => 1,
            Port::Accountant => 2,
            Port::PacketSender => 3,
        }
    }
}

#[derive(Clone)]
pub enum Data {
    Signal,
    SharedPacketData(streamer::SharedPacketData),
}

struct Locked {
    ports: Vec<Sender<Data>>,
    readers: Vec<Arc<Mutex<Receiver<Data>>>>,
    threads: Vec<Arc<Option<JoinHandle<Result<()>>>>>,
}

pub struct Services {
    lock: Arc<RwLock<Locked>>,
    exit: Arc<AtomicBool>,
}

pub type Ports = Vec<Sender<Data>>;

impl Services {
    pub fn new() -> Services {
        let (s1, r1) = channel();
        let (s2, r2) = channel();
        let (s3, r3) = channel();
        let (s4, r4) = channel();
        let (s5, r5) = channel();
        let locked = Locked {
            ports: [s1, s2, s3, s4, s5].to_vec(),
            readers: [
                Arc::new(Mutex::new(r1)),
                Arc::new(Mutex::new(r2)),
                Arc::new(Mutex::new(r3)),
                Arc::new(Mutex::new(r4)),
                Arc::new(Mutex::new(r5)),
            ].to_vec(),
            threads: [
                Arc::new(None),
                Arc::new(None),
                Arc::new(None),
                Arc::new(None),
                Arc::new(None),
            ].to_vec(),
        };
        let exit = Arc::new(AtomicBool::new(false));
        Services {
            lock: Arc::new(RwLock::new(locked)),
            exit: exit,
        }
    }
    pub fn source<F>(&self, port: Port, func: F) -> Result<()>
    where
        F: Send + 'static + Fn(&Ports) -> Result<()>,
    {
        let mut w = self.lock.write().unwrap();
        let pz = port.to_usize();
        if w.threads[pz].is_some() {
            return Err(Error::Services);
        }
        let c_ports = w.ports.clone();
        let c_exit = self.exit.clone();
        let j = spawn(move || loop {
            match func(&c_ports) {
                Ok(()) => (),
                e => return e,
            }
            if c_exit.load(Ordering::Relaxed) == true {
                return Ok(());
            }
        });
        w.threads[pz] = Arc::new(Some(j));
        return Ok(());
    }
    pub fn listen<F>(&mut self, port: Port, func: F) -> Result<()>
    where
        F: Send + 'static + Fn(&Ports, Data) -> Result<()>,
    {
        let mut w = self.lock.write().unwrap();
        let pz = port.to_usize();
        if w.threads[pz].is_some() {
            return Err(Error::Services);
        }
        let recv_lock = w.readers[pz].clone();
        let c_ports = w.ports.clone();
        let c_exit = self.exit.clone();
        let j: JoinHandle<Result<()>> = spawn(move || loop {
            let recv = recv_lock.lock().unwrap();
            let timer = Duration::new(0, 500000);
            match recv.recv_timeout(timer) {
                Ok(val) => func(&c_ports, val).expect("services listen"),
                _ => (),
            }
            if c_exit.load(Ordering::Relaxed) {
                return Ok(());
            }
        });
        w.threads[pz] = Arc::new(Some(j));
        return Ok(());
    }
    pub fn send(ports: &Ports, to: Port, m: Data) -> Result<()> {
        ports[to.to_usize()]
            .send(m)
            .or_else(|_| Err(Error::SendError))
    }
    pub fn join(&mut self) -> Result<()> {
        let pz = Port::Main.to_usize();
        let recv = self.lock.write().unwrap().readers[pz].clone();
        recv.lock().unwrap().recv()?;
        self.shutdown()?;
        return Ok(());
    }
    pub fn shutdown(&mut self) -> Result<()> {
        self.exit.store(true, Ordering::Relaxed);
        let r = self.lock.read().unwrap();
        for t in r.threads.iter() {
            match Arc::try_unwrap((*t).clone()) {
                Ok(Some(j)) => j.join()??,
                _ => (),
            };
        }
        return Ok(());
    }
}

#[cfg(test)]
mod test {
    use services::Services;
    use services::Port::{Accountant, Main, PacketReader};
    use services::Data::Signal;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_init() {
        let mut o = Services::new();
        assert_matches!(o.shutdown(), Ok(()));
    }
    #[test]
    fn test_join() {
        let mut o = Services::new();
        assert_matches!(
            o.source(PacketReader, move |ports| Services::send(
                ports,
                Main,
                Signal
            )),
            Ok(())
        );
        assert_matches!(o.join(), Ok(()));
    }
    #[test]
    fn test_source() {
        let mut o = Services::new();
        assert_matches!(
            o.source(PacketReader, move |ports| Services::send(
                ports,
                Main,
                Signal
            )),
            Ok(())
        );
        assert!(o.source(PacketReader, move |_ports| Ok(())).is_err());
        assert!(o.listen(PacketReader, move |_ports, _data| Ok(())).is_err());
        assert_matches!(o.join(), Ok(()));
    }
    #[test]
    fn test_listen() {
        let mut o = Services::new();
        let val = Arc::new(Mutex::new(false));
        assert_matches!(
            o.source(PacketReader, move |ports| Services::send(
                ports,
                Accountant,
                Signal
            )),
            Ok(())
        );
        let c_val = val.clone();
        assert_matches!(
            o.listen(Accountant, move |ports, data| match data {
                Signal => {
                    *c_val.lock().unwrap() = true;
                    Services::send(ports, Main, Signal)
                }
                _ => Ok(()),
            }),
            Ok(())
        );
        assert_matches!(o.join(), Ok(()));
        assert_eq!(*val.lock().unwrap(), true);
    }

}
