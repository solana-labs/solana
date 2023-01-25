use {
    crate::connection_cache::ConnectionCache,
    solana_connection_cache::client_connection::ClientConnection,
    solana_sdk::transport::TransportError,
    std::{borrow::Borrow, net::SocketAddr},
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum SendPktsError {
    /// IO Error during send: first error, num failed packets
    #[error("IO Error, some packets could not be sent")]
    TransportError(TransportError, usize),
}

impl From<SendPktsError> for TransportError {
    fn from(err: SendPktsError) -> Self {
        Self::Custom(format!("{err:?}"))
    }
}

pub fn batch_send<S, T>(
    connection_cache: &ConnectionCache,
    packets: &[(T, S)],
) -> Result<(), SendPktsError>
where
    S: Borrow<SocketAddr>,
    T: AsRef<[u8]>,
{
    let mut num_failed = 0;
    let mut erropt = None;
    for (p, a) in packets {
        let address = a.borrow();
        let connection = connection_cache.get_connection(address);
        if let Err(e) = connection.send_data(p.as_ref()) {
            num_failed += 1;
            if erropt.is_none() {
                erropt = Some(e);
            }
        }
    }

    if let Some(err) = erropt {
        Err(SendPktsError::TransportError(err, num_failed))
    } else {
        Ok(())
    }
}
