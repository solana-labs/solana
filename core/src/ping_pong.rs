use bincode::{serialize, Error};
use lru::LruCache;
use rand::{AsByteSliceMut, CryptoRng, Rng};
use serde::Serialize;
use solana_sdk::hash::{self, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::sanitize::{Sanitize, SanitizeError};
use solana_sdk::signature::{Keypair, Signable, Signature, Signer};
use std::borrow::Cow;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

#[derive(AbiExample, Debug, Deserialize, Serialize)]
pub struct Ping<T> {
    from: Pubkey,
    token: T,
    signature: Signature,
}

#[derive(AbiExample, Debug, Deserialize, Serialize)]
pub struct Pong {
    from: Pubkey,
    hash: Hash, // Hash of received ping token.
    signature: Signature,
}

/// Maintains records of remote nodes which have returned a valid response to a
/// ping message, and on-the-fly ping messages pending a pong response from the
/// remote node.
pub struct PingCache {
    // Time-to-live of received pong messages.
    ttl: Duration,
    // Timestamp of last ping message sent to a remote node.
    // Used to rate limit pings to remote nodes.
    pings: LruCache<(Pubkey, SocketAddr), Instant>,
    // Verified pong responses from remote nodes.
    pongs: LruCache<(Pubkey, SocketAddr), Instant>,
    // Hash of ping tokens sent out to remote nodes,
    // pending a pong response back.
    pending_cache: LruCache<Hash, (Pubkey, SocketAddr)>,
}

impl<T: Serialize> Ping<T> {
    pub fn new(token: T, keypair: &Keypair) -> Result<Self, Error> {
        let signature = keypair.sign_message(&serialize(&token)?);
        let ping = Ping {
            from: keypair.pubkey(),
            token,
            signature,
        };
        Ok(ping)
    }
}

impl<T> Ping<T>
where
    T: Serialize + AsByteSliceMut + Default,
{
    pub fn new_rand<R>(rng: &mut R, keypair: &Keypair) -> Result<Self, Error>
    where
        R: Rng + CryptoRng,
    {
        let mut token = T::default();
        rng.fill(&mut token);
        Ping::new(token, keypair)
    }
}

impl<T> Sanitize for Ping<T> {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        self.from.sanitize()?;
        // TODO Add self.token.sanitize()?; when rust's
        // specialization feature becomes stable.
        self.signature.sanitize()
    }
}

impl<T: Serialize> Signable for Ping<T> {
    fn pubkey(&self) -> Pubkey {
        self.from
    }

    fn signable_data(&self) -> Cow<[u8]> {
        Cow::Owned(serialize(&self.token).unwrap())
    }

    fn get_signature(&self) -> Signature {
        self.signature
    }

    fn set_signature(&mut self, signature: Signature) {
        self.signature = signature;
    }
}

impl Pong {
    pub fn new<T: Serialize>(ping: &Ping<T>, keypair: &Keypair) -> Result<Self, Error> {
        let hash = hash::hash(&serialize(&ping.token)?);
        let pong = Pong {
            from: keypair.pubkey(),
            hash,
            signature: keypair.sign_message(hash.as_ref()),
        };
        Ok(pong)
    }
}

impl Sanitize for Pong {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        self.from.sanitize()?;
        self.hash.sanitize()?;
        self.signature.sanitize()
    }
}

impl Signable for Pong {
    fn pubkey(&self) -> Pubkey {
        self.from
    }

    fn signable_data(&self) -> Cow<[u8]> {
        Cow::Owned(self.hash.as_ref().into())
    }

    fn get_signature(&self) -> Signature {
        self.signature
    }

    fn set_signature(&mut self, signature: Signature) {
        self.signature = signature;
    }
}

impl PingCache {
    pub fn new(ttl: Duration, cap: usize) -> Self {
        Self {
            ttl,
            pings: LruCache::new(cap),
            pongs: LruCache::new(cap),
            pending_cache: LruCache::new(cap),
        }
    }

    /// Checks if the pong hash, pubkey and socket match a ping message sent
    /// out previously. If so records current timestamp for the remote node and
    /// returns true.
    /// Note: Does not verify the signature.
    pub fn add(&mut self, pong: &Pong, socket: SocketAddr, now: Instant) -> bool {
        let node = (pong.pubkey(), socket);
        match self.pending_cache.peek(&pong.hash) {
            Some(value) if *value == node => {
                self.pings.pop(&node);
                self.pongs.put(node, now);
                self.pending_cache.pop(&pong.hash);
                true
            }
            _ => false,
        }
    }

    /// Checks if the remote node has been pinged recently. If not, calls the
    /// given function to generates a new ping message, records current
    /// timestamp and hash of ping token, and returns the ping message.
    fn maybe_ping<T, F>(
        &mut self,
        now: Instant,
        node: (Pubkey, SocketAddr),
        mut pingf: F,
    ) -> Option<Ping<T>>
    where
        T: Serialize,
        F: FnMut() -> Option<Ping<T>>,
    {
        // Rate limit consecutive pings sent to a remote node.
        let delay = self.ttl / 64;
        match self.pings.peek(&node) {
            Some(t) if now.saturating_duration_since(*t) < delay => None,
            _ => {
                let ping = pingf()?;
                let hash = hash::hash(&serialize(&ping.token).ok()?);
                self.pings.put(node, now);
                self.pending_cache.put(hash, node);
                Some(ping)
            }
        }
    }

    /// Returns true if the remote node has responded to a ping message.
    /// Removes expired pong messages. In order to extend verifications before
    /// expiration, if the pong message is not too recent, and the node has not
    /// been pinged recently, calls the given function to generates a new ping
    /// message, records current timestamp and hash of ping token, and returns
    /// the ping message.
    /// Caller should verify if the socket address is valid. (e.g. by using
    /// ContactInfo::is_valid_address).
    pub fn check<T, F>(
        &mut self,
        now: Instant,
        node: (Pubkey, SocketAddr),
        pingf: F,
    ) -> (bool, Option<Ping<T>>)
    where
        T: Serialize,
        F: FnMut() -> Option<Ping<T>>,
    {
        let (check, should_ping) = match self.pongs.get(&node) {
            None => (false, true),
            Some(t) => {
                let age = now.saturating_duration_since(*t);
                // Pop if the pong message has expired.
                if age > self.ttl {
                    self.pongs.pop(&node);
                }
                // If the pong message is not too recent, generate a new ping
                // message to extend remote node verification.
                (true, age > self.ttl / 8)
            }
        };
        let ping = if should_ping {
            self.maybe_ping(now, node, pingf)
        } else {
            None
        };
        (check, ping)
    }
}

impl Clone for PingCache {
    fn clone(&self) -> Self {
        let mut clone = Self {
            ttl: self.ttl,
            pings: LruCache::new(self.pings.cap()),
            pongs: LruCache::new(self.pongs.cap()),
            pending_cache: LruCache::new(self.pending_cache.cap()),
        };
        for (k, v) in self.pongs.iter() {
            clone.pings.put(*k, *v);
        }
        for (k, v) in self.pongs.iter() {
            clone.pongs.put(*k, *v);
        }
        for (k, v) in self.pending_cache.iter() {
            clone.pending_cache.put(*k, *v);
        }
        clone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_rand() {
        let mut rng = rand::thread_rng();
        let keypair = Keypair::new();
        let ping = Ping::<[u8; 32]>::new_rand(&mut rng, &keypair).unwrap();
        assert!(ping.verify());
        let pong = Pong::new(&ping, &keypair).unwrap();
        assert!(pong.verify());
        assert_eq!(hash::hash(&ping.token), pong.hash);
    }
}
