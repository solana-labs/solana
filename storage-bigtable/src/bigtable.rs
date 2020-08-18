// Primitives for reading/writing BigTable tables

use crate::access_token::{AccessToken, Scope};
use crate::compression::{compress_best, decompress};
use crate::root_ca_certificate;
use log::*;
use thiserror::Error;
use tonic::{metadata::MetadataValue, transport::ClientTlsConfig, Request};

mod google {
    mod rpc {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            concat!("/proto/google.rpc.rs")
        ));
    }
    pub mod bigtable {
        pub mod v2 {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                concat!("/proto/google.bigtable.v2.rs")
            ));
        }
    }
}
use google::bigtable::v2::*;

pub type RowKey = String;
pub type CellName = String;
pub type CellValue = Vec<u8>;
pub type RowData = Vec<(CellName, CellValue)>;
pub type RowDataSlice<'a> = &'a [(CellName, CellValue)];

#[derive(Debug, Error)]
pub enum Error {
    #[error("AccessToken error: {0}")]
    AccessTokenError(String),

    #[error("Certificate error: {0}")]
    CertificateError(String),

    #[error("I/O Error: {0}")]
    IoError(std::io::Error),

    #[error("Transport error: {0}")]
    TransportError(tonic::transport::Error),

    #[error("Invalid URI {0}: {1}")]
    InvalidUri(String, String),

    #[error("Row not found")]
    RowNotFound,

    #[error("Row write failed")]
    RowWriteFailed,

    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    #[error("Object is corrupt: {0}")]
    ObjectCorrupt(String),

    #[error("RPC error: {0}")]
    RpcError(tonic::Status),
}

impl std::convert::From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl std::convert::From<tonic::transport::Error> for Error {
    fn from(err: tonic::transport::Error) -> Self {
        Self::TransportError(err)
    }
}

impl std::convert::From<tonic::Status> for Error {
    fn from(err: tonic::Status) -> Self {
        Self::RpcError(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct BigTableConnection {
    access_token: Option<AccessToken>,
    channel: tonic::transport::Channel,
    table_prefix: String,
}

impl BigTableConnection {
    /// Establish a connection to the BigTable instance named `instance_name`.  If read-only access
    /// is required, the `read_only` flag should be used to reduce the requested OAuth2 scope.
    ///
    /// The GOOGLE_APPLICATION_CREDENTIALS environment variable will be used to determine the
    /// program name that contains the BigTable instance in addition to access credentials.
    ///
    /// The BIGTABLE_EMULATOR_HOST environment variable is also respected.
    ///
    pub async fn new(instance_name: &str, read_only: bool) -> Result<Self> {
        match std::env::var("BIGTABLE_EMULATOR_HOST") {
            Ok(endpoint) => {
                info!("Connecting to bigtable emulator at {}", endpoint);

                Ok(Self {
                    access_token: None,
                    channel: tonic::transport::Channel::from_shared(format!("http://{}", endpoint))
                        .map_err(|err| Error::InvalidUri(endpoint, err.to_string()))?
                        .connect_lazy()?,
                    table_prefix: format!("projects/emulator/instances/{}/tables/", instance_name),
                })
            }

            Err(_) => {
                let access_token = AccessToken::new(if read_only {
                    Scope::BigTableDataReadOnly
                } else {
                    Scope::BigTableData
                })
                .await
                .map_err(Error::AccessTokenError)?;

                let table_prefix = format!(
                    "projects/{}/instances/{}/tables/",
                    access_token.project(),
                    instance_name
                );

                Ok(Self {
                    access_token: Some(access_token),
                    channel: tonic::transport::Channel::from_static(
                        "https://bigtable.googleapis.com",
                    )
                    .tls_config(
                        ClientTlsConfig::new()
                            .ca_certificate(
                                root_ca_certificate::load().map_err(Error::CertificateError)?,
                            )
                            .domain_name("bigtable.googleapis.com"),
                    )?
                    .connect_lazy()?,
                    table_prefix,
                })
            }
        }
    }

    /// Create a new BigTable client.
    ///
    /// Clients require `&mut self`, due to `Tonic::transport::Channel` limitations, however
    /// creating new clients is cheap and thus can be used as a work around for ease of use.
    pub fn client(&self) -> BigTable {
        let client = if let Some(access_token) = &self.access_token {
            let access_token = access_token.clone();
            bigtable_client::BigtableClient::with_interceptor(
                self.channel.clone(),
                move |mut req: Request<()>| {
                    match MetadataValue::from_str(&access_token.get()) {
                        Ok(authorization_header) => {
                            req.metadata_mut()
                                .insert("authorization", authorization_header);
                        }
                        Err(err) => {
                            warn!("Failed to set authorization header: {}", err);
                        }
                    }
                    Ok(req)
                },
            )
        } else {
            bigtable_client::BigtableClient::new(self.channel.clone())
        };
        BigTable {
            access_token: self.access_token.clone(),
            client,
            table_prefix: self.table_prefix.clone(),
        }
    }

    pub async fn put_bincode_cells_with_retry<T>(
        &self,
        table: &str,
        cells: &[(RowKey, T)],
    ) -> Result<usize>
    where
        T: serde::ser::Serialize,
    {
        use backoff::{future::FutureOperation as _, ExponentialBackoff};
        (|| async {
            let mut client = self.client();
            Ok(client.put_bincode_cells(table, cells).await?)
        })
        .retry(ExponentialBackoff::default())
        .await
    }
}

pub struct BigTable {
    access_token: Option<AccessToken>,
    client: bigtable_client::BigtableClient<tonic::transport::Channel>,
    table_prefix: String,
}

impl BigTable {
    async fn decode_read_rows_response(
        mut rrr: tonic::codec::Streaming<ReadRowsResponse>,
    ) -> Result<Vec<(RowKey, RowData)>> {
        let mut rows: Vec<(RowKey, RowData)> = vec![];

        let mut row_key = None;
        let mut row_data = vec![];

        let mut cell_name = None;
        let mut cell_timestamp = 0;
        let mut cell_value = vec![];
        let mut cell_version_ok = true;
        while let Some(res) = rrr.message().await? {
            for (i, mut chunk) in res.chunks.into_iter().enumerate() {
                // The comments for `read_rows_response::CellChunk` provide essential details for
                // understanding how the below decoding works...
                trace!("chunk {}: {:?}", i, chunk);

                // Starting a new row?
                if !chunk.row_key.is_empty() {
                    row_key = String::from_utf8(chunk.row_key).ok(); // Require UTF-8 for row keys
                }

                // Starting a new cell?
                if let Some(qualifier) = chunk.qualifier {
                    if let Some(cell_name) = cell_name {
                        row_data.push((cell_name, cell_value));
                        cell_value = vec![];
                    }
                    cell_name = String::from_utf8(qualifier).ok(); // Require UTF-8 for cell names
                    cell_timestamp = chunk.timestamp_micros;
                    cell_version_ok = true;
                } else {
                    // Continuing the existing cell.  Check if this is the start of another version of the cell
                    if chunk.timestamp_micros != 0 {
                        if chunk.timestamp_micros < cell_timestamp {
                            cell_version_ok = false; // ignore older versions of the cell
                        } else {
                            // newer version of the cell, remove the older cell
                            cell_version_ok = true;
                            cell_value = vec![];
                            cell_timestamp = chunk.timestamp_micros;
                        }
                    }
                }
                if cell_version_ok {
                    cell_value.append(&mut chunk.value);
                }

                // End of a row?
                if chunk.row_status.is_some() {
                    if let Some(read_rows_response::cell_chunk::RowStatus::CommitRow(_)) =
                        chunk.row_status
                    {
                        if let Some(cell_name) = cell_name {
                            row_data.push((cell_name, cell_value));
                        }

                        if let Some(row_key) = row_key {
                            rows.push((row_key, row_data))
                        }
                    }

                    row_key = None;
                    row_data = vec![];
                    cell_value = vec![];
                    cell_name = None;
                }
            }
        }
        Ok(rows)
    }

    async fn refresh_access_token(&self) {
        if let Some(ref access_token) = self.access_token {
            access_token.refresh().await;
        }
    }

    /// Get `table` row keys in lexical order.
    ///
    /// If `start_at` is provided, the row key listing will start with key.
    /// Otherwise the listing will start from the start of the table.
    ///
    /// If `end_at` is provided, the row key listing will end at the key. Otherwise it will
    /// continue until the `limit` is reached or the end of the table, whichever comes first.
    pub async fn get_row_keys(
        &mut self,
        table_name: &str,
        start_at: Option<RowKey>,
        end_at: Option<RowKey>,
        rows_limit: i64,
    ) -> Result<Vec<RowKey>> {
        self.refresh_access_token().await;
        let response = self
            .client
            .read_rows(ReadRowsRequest {
                table_name: format!("{}{}", self.table_prefix, table_name),
                rows_limit,
                rows: Some(RowSet {
                    row_keys: vec![],
                    row_ranges: vec![RowRange {
                        start_key: start_at.map(|row_key| {
                            row_range::StartKey::StartKeyClosed(row_key.into_bytes())
                        }),
                        end_key: end_at
                            .map(|row_key| row_range::EndKey::EndKeyClosed(row_key.into_bytes())),
                    }],
                }),
                filter: Some(RowFilter {
                    filter: Some(row_filter::Filter::Chain(row_filter::Chain {
                        filters: vec![
                            RowFilter {
                                // Return minimal number of cells
                                filter: Some(row_filter::Filter::CellsPerRowLimitFilter(1)),
                            },
                            RowFilter {
                                // Only return the latest version of each cell
                                filter: Some(row_filter::Filter::CellsPerColumnLimitFilter(1)),
                            },
                            RowFilter {
                                // Strip the cell values
                                filter: Some(row_filter::Filter::StripValueTransformer(true)),
                            },
                        ],
                    })),
                }),
                ..ReadRowsRequest::default()
            })
            .await?
            .into_inner();

        let rows = Self::decode_read_rows_response(response).await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Get latest data from `table`.
    ///
    /// All column families are accepted, and only the latest version of each column cell will be
    /// returned.
    ///
    /// If `start_at` is provided, the row key listing will start with key.
    /// Otherwise the listing will start from the start of the table.
    ///
    /// If `end_at` is provided, the row key listing will end at the key. Otherwise it will
    /// continue until the `limit` is reached or the end of the table, whichever comes first.
    pub async fn get_row_data(
        &mut self,
        table_name: &str,
        start_at: Option<RowKey>,
        end_at: Option<RowKey>,
        rows_limit: i64,
    ) -> Result<Vec<(RowKey, RowData)>> {
        self.refresh_access_token().await;

        let response = self
            .client
            .read_rows(ReadRowsRequest {
                table_name: format!("{}{}", self.table_prefix, table_name),
                rows_limit,
                rows: Some(RowSet {
                    row_keys: vec![],
                    row_ranges: vec![RowRange {
                        start_key: start_at.map(|row_key| {
                            row_range::StartKey::StartKeyClosed(row_key.into_bytes())
                        }),
                        end_key: end_at
                            .map(|row_key| row_range::EndKey::EndKeyClosed(row_key.into_bytes())),
                    }],
                }),
                filter: Some(RowFilter {
                    // Only return the latest version of each cell
                    filter: Some(row_filter::Filter::CellsPerColumnLimitFilter(1)),
                }),
                ..ReadRowsRequest::default()
            })
            .await?
            .into_inner();

        Self::decode_read_rows_response(response).await
    }

    /// Store data for one or more `table` rows in the `family_name` Column family
    async fn put_row_data(
        &mut self,
        table_name: &str,
        family_name: &str,
        row_data: &[(&RowKey, RowData)],
    ) -> Result<()> {
        self.refresh_access_token().await;

        let mut entries = vec![];
        for (row_key, row_data) in row_data {
            let mutations = row_data
                .iter()
                .map(|(column_key, column_value)| Mutation {
                    mutation: Some(mutation::Mutation::SetCell(mutation::SetCell {
                        family_name: family_name.to_string(),
                        column_qualifier: column_key.clone().into_bytes(),
                        timestamp_micros: -1, // server assigned
                        value: column_value.to_vec(),
                    })),
                })
                .collect();

            entries.push(mutate_rows_request::Entry {
                row_key: (*row_key).clone().into_bytes(),
                mutations,
            });
        }

        let mut response = self
            .client
            .mutate_rows(MutateRowsRequest {
                table_name: format!("{}{}", self.table_prefix, table_name),
                entries,
                ..MutateRowsRequest::default()
            })
            .await?
            .into_inner();

        while let Some(res) = response.message().await? {
            for entry in res.entries {
                if let Some(status) = entry.status {
                    if status.code != 0 {
                        eprintln!("put_row_data error {}: {}", status.code, status.message);
                        warn!("put_row_data error {}: {}", status.code, status.message);
                        return Err(Error::RowWriteFailed);
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn get_bincode_cell<T>(&mut self, table: &str, key: RowKey) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let row_data = self.get_row_data(table, Some(key.clone()), None, 1).await?;
        let (row_key, data) = &row_data.get(0).ok_or_else(|| Error::RowNotFound)?;

        deserialize_cell_data(data, table, row_key.to_string())
    }

    pub async fn put_bincode_cells<T>(
        &mut self,
        table: &str,
        cells: &[(RowKey, T)],
    ) -> Result<usize>
    where
        T: serde::ser::Serialize,
    {
        let mut bytes_written = 0;
        let mut new_row_data = vec![];
        for (row_key, data) in cells {
            let data = compress_best(&bincode::serialize(&data).unwrap())?;
            bytes_written += data.len();
            new_row_data.push((row_key, vec![("bin".to_string(), data)]));
        }

        self.put_row_data(table, "x", &new_row_data).await?;
        Ok(bytes_written)
    }
}

pub(crate) fn deserialize_cell_data<T>(
    row_data: RowDataSlice,
    table: &str,
    key: RowKey,
) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let value = &row_data
        .iter()
        .find(|(name, _)| name == "bin")
        .ok_or_else(|| Error::ObjectNotFound(format!("{}/{}", table, key)))?
        .1;

    let data = decompress(&value)?;
    bincode::deserialize(&data).map_err(|err| {
        warn!("Failed to deserialize {}/{}: {}", table, key, err);
        Error::ObjectCorrupt(format!("{}/{}", table, key))
    })
}
