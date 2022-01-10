// Primitives for reading/writing BigTable tables

use {
    crate::{
        access_token::{AccessToken, Scope},
        compression::{compress_best, decompress},
        root_ca_certificate,
    },
    async_stream::try_stream,
    backoff::{future::retry, ExponentialBackoff},
    futures::{
        pin_mut,
        stream::{Stream, StreamExt, TryStreamExt},
        task::{Context, Poll},
    },
    log::*,
    std::{
        collections::VecDeque,
        pin::Pin,
        time::Duration,
    },
    thiserror::Error,
    tonic::{
        codegen::InterceptedService, metadata::MetadataValue, transport::ClientTlsConfig, Request,
        Status,
    },
};

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
pub type RowData = Vec<(CellName, CellValue)>;
pub type RowDataSlice<'a> = &'a [(CellName, CellValue)];
pub type CellName = String;
pub type CellValue = Vec<u8>;
pub enum CellData<B, P> {
    Bincode(B),
    Protobuf(P),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("AccessToken: {0}")]
    AccessToken(String),

    #[error("Certificate: {0}")]
    Certificate(String),

    #[error("I/O: {0}")]
    Io(std::io::Error),

    #[error("Transport: {0}")]
    Transport(tonic::transport::Error),

    #[error("Invalid URI {0}: {1}")]
    InvalidUri(String, String),

    #[error("Row not found")]
    RowNotFound,

    #[error("Row write failed")]
    RowWriteFailed,

    #[error("Row delete failed")]
    RowDeleteFailed,

    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    #[error("Object is corrupt: {0}")]
    ObjectCorrupt(String),

    #[error("RPC: {0}")]
    Rpc(tonic::Status),

    #[error("Timeout")]
    Timeout,
}

impl std::convert::From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl std::convert::From<tonic::transport::Error> for Error {
    fn from(err: tonic::transport::Error) -> Self {
        Self::Transport(err)
    }
}

impl std::convert::From<tonic::Status> for Error {
    fn from(err: tonic::Status) -> Self {
        Self::Rpc(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
type InterceptedRequestResult = std::result::Result<Request<()>, Status>;

#[derive(Clone)]
pub struct BigTableConnection {
    access_token: Option<AccessToken>,
    channel: tonic::transport::Channel,
    table_prefix: String,
    timeout: Option<Duration>,
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
    pub async fn new(
        instance_name: &str,
        read_only: bool,
        timeout: Option<Duration>,
        credential_path: Option<String>,
    ) -> Result<Self> {
        match std::env::var("BIGTABLE_EMULATOR_HOST") {
            Ok(endpoint) => {
                info!("Connecting to bigtable emulator at {}", endpoint);

                Ok(Self {
                    access_token: None,
                    channel: tonic::transport::Channel::from_shared(format!("http://{}", endpoint))
                        .map_err(|err| Error::InvalidUri(endpoint, err.to_string()))?
                        .connect_lazy(),
                    table_prefix: format!("projects/emulator/instances/{}/tables/", instance_name),
                    timeout,
                })
            }

            Err(_) => {
                let access_token = AccessToken::new(
                    if read_only {
                        Scope::BigTableDataReadOnly
                    } else {
                        Scope::BigTableData
                    },
                    credential_path,
                )
                .await
                .map_err(Error::AccessToken)?;

                let table_prefix = format!(
                    "projects/{}/instances/{}/tables/",
                    access_token.project(),
                    instance_name
                );

                let endpoint = {
                    let endpoint =
                        tonic::transport::Channel::from_static("https://bigtable.googleapis.com")
                            .tls_config(
                            ClientTlsConfig::new()
                                .ca_certificate(
                                    root_ca_certificate::load().map_err(Error::Certificate)?,
                                )
                                .domain_name("bigtable.googleapis.com"),
                        )?;

                    if let Some(timeout) = timeout {
                        endpoint.timeout(timeout)
                    } else {
                        endpoint
                    }
                };

                Ok(Self {
                    access_token: Some(access_token),
                    channel: endpoint.connect_lazy(),
                    table_prefix,
                    timeout,
                })
            }
        }
    }

    /// Create a new BigTable client.
    ///
    /// Clients require `&mut self`, due to `Tonic::transport::Channel` limitations, however
    /// creating new clients is cheap and thus can be used as a work around for ease of use.
    pub fn client(&self) -> BigTable<impl FnMut(Request<()>) -> InterceptedRequestResult> {
        let access_token = self.access_token.clone();
        let client = bigtable_client::BigtableClient::with_interceptor(
            self.channel.clone(),
            move |mut req: Request<()>| {
                if let Some(access_token) = &access_token {
                    match MetadataValue::from_str(&access_token.get()) {
                        Ok(authorization_header) => {
                            req.metadata_mut()
                                .insert("authorization", authorization_header);
                        }
                        Err(err) => {
                            warn!("Failed to set authorization header: {}", err);
                        }
                    }
                }
                Ok(req)
            },
        );
        BigTable {
            access_token: self.access_token.clone(),
            client,
            table_prefix: self.table_prefix.clone(),
            timeout: self.timeout,
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
        retry(ExponentialBackoff::default(), || async {
            let mut client = self.client();
            Ok(client.put_bincode_cells(table, cells).await?)
        })
        .await
    }

    pub async fn delete_rows_with_retry(&self, table: &str, row_keys: &[RowKey]) -> Result<()> {
        retry(ExponentialBackoff::default(), || async {
            let mut client = self.client();
            Ok(client.delete_rows(table, row_keys).await?)
        })
        .await
    }

    pub async fn get_bincode_cells_with_retry<T>(
        &self,
        table: &str,
        row_keys: &[RowKey],
    ) -> Result<Vec<(RowKey, Result<T>)>>
    where
        T: serde::de::DeserializeOwned,
    {
        retry(ExponentialBackoff::default(), || async {
            let mut client = self.client();
            Ok(client.get_bincode_cells(table, row_keys).await?)
        })
        .await
    }

    pub async fn put_protobuf_cells_with_retry<T>(
        &self,
        table: &str,
        cells: &[(RowKey, T)],
    ) -> Result<usize>
    where
        T: prost::Message,
    {
        retry(ExponentialBackoff::default(), || async {
            let mut client = self.client();
            Ok(client.put_protobuf_cells(table, cells).await?)
        })
        .await
    }
}

pub struct BigTable<F: FnMut(Request<()>) -> InterceptedRequestResult> {
    access_token: Option<AccessToken>,
    client: bigtable_client::BigtableClient<InterceptedService<tonic::transport::Channel, F>>,
    table_prefix: String,
    timeout: Option<Duration>,
}

impl<F: FnMut(Request<()>) -> InterceptedRequestResult> BigTable<F> {
    fn stream_read_rows_response(
        &self,
        rrr: tonic::codec::Streaming<ReadRowsResponse>,
    ) -> ReadRowStream {
        ReadRowStream::new(rrr)
    }

    async fn decode_read_rows_response(
        &self,
        rrr: tonic::codec::Streaming<ReadRowsResponse>,
    ) -> Result<Vec<(RowKey, RowData)>> {
        let collect_future = self.
            stream_read_rows_response(rrr).
            try_collect();
        if let Some(timeout) = self.timeout {
            if let Ok(res) = tokio::time::timeout(timeout, collect_future).await {
                res
            } else {
                Err(Error::Timeout)
            }
        } else {
            collect_future.await
        }
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
    /// continue until the `rows_limit` is reached or the end of the table, whichever comes first.
    /// If `rows_limit` is zero, the listing will continue until the end of the table.
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

        let rows = self.decode_read_rows_response(response).await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Get latest data from `table`.
    ///
    /// All column families are accepted, and only the latest version of each column cell will be
    /// returned.
    ///
    /// If `start_at` is provided, the row key listing will start with key, or the next key in the
    /// table if the explicit key does not exist. Otherwise the listing will start from the start
    /// of the table.
    ///
    /// If `end_at` is provided, the row key listing will end at the key. Otherwise it will
    /// continue until the `rows_limit` is reached or the end of the table, whichever comes first.
    /// If `rows_limit` is zero, the listing will continue until the end of the table.
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

        self.decode_read_rows_response(response).await
    }

    /// Get latest data from multiple rows of `table`, if those rows exist.
    pub async fn get_multi_row_data(
        &mut self,
        table_name: &str,
        row_keys: &[RowKey],
    ) -> Result<Vec<(RowKey, RowData)>> {
        self.refresh_access_token().await;

        let response = self
            .client
            .read_rows(ReadRowsRequest {
                table_name: format!("{}{}", self.table_prefix, table_name),
                rows_limit: 0, // return all keys
                rows: Some(RowSet {
                    row_keys: row_keys
                        .iter()
                        .map(|k| k.as_bytes().to_vec())
                        .collect::<Vec<_>>(),
                    row_ranges: vec![],
                }),
                filter: Some(RowFilter {
                    // Only return the latest version of each cell
                    filter: Some(row_filter::Filter::CellsPerColumnLimitFilter(1)),
                }),
                ..ReadRowsRequest::default()
            })
            .await?
            .into_inner();

        self.decode_read_rows_response(response).await
    }

    /// Get latest data from a single row of `table`, if that row exists. Returns an error if that
    /// row does not exist.
    ///
    /// All column families are accepted, and only the latest version of each column cell will be
    /// returned.
    pub async fn get_single_row_data(
        &mut self,
        table_name: &str,
        row_key: RowKey,
    ) -> Result<RowData> {
        self.refresh_access_token().await;

        let response = self
            .client
            .read_rows(ReadRowsRequest {
                table_name: format!("{}{}", self.table_prefix, table_name),
                rows_limit: 1,
                rows: Some(RowSet {
                    row_keys: vec![row_key.into_bytes()],
                    row_ranges: vec![],
                }),
                filter: Some(RowFilter {
                    // Only return the latest version of each cell
                    filter: Some(row_filter::Filter::CellsPerColumnLimitFilter(1)),
                }),
                ..ReadRowsRequest::default()
            })
            .await?
            .into_inner();

        let rows = self.decode_read_rows_response(response).await?;
        rows.into_iter()
            .next()
            .map(|r| r.1)
            .ok_or(Error::RowNotFound)
    }

    /// Stream latest data from `table` asynchronously.
    pub fn stream_row_data(
        mut self,
        table_name: String,
        start_at: Option<RowKey>,
        end_at: Option<RowKey>,
        mut rows_limit: i64,
    ) -> impl Stream<Item=Result<(RowKey, RowData)>> {
        try_stream! {
            let mut start_key = start_at.map(|row_key|
                row_range::StartKey::StartKeyClosed(row_key.into_bytes()));
            let end_key = end_at.map(|row_key|
                row_range::EndKey::EndKeyClosed(row_key.into_bytes()));
            let mut last_row_key: Option<RowKey> = None;

            'outer: loop {
                if let Some(ref last_key) = last_row_key {
                    start_key = Some(row_range::StartKey::StartKeyOpen(last_key.clone().into_bytes()));
                }
                let stream: ReadRowStream = self.stream_row_data_at(
                    &table_name,
                    start_key.clone(),
                    end_key.clone(),
                    rows_limit,
                ).await?;
                pin_mut!(stream);
                while let Some(stream_item) = stream.next().await {
                    let (row_key, row_data) = match stream_item {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Error reading row stream item: {}", e);
                            continue 'outer;
                        }
                    };
                    last_row_key = Some(row_key.clone());
                    yield (row_key, row_data);
                    if rows_limit > 0 {
                        rows_limit -= 1;
                        if rows_limit == 0 {
                            break 'outer;
                        }
                    }
                }
                break 'outer;
            }
        }
    }

    async fn stream_row_data_at(
        &mut self,
        table_name: &str,
        start_key: Option<row_range::StartKey>,
        end_key: Option<row_range::EndKey>,
        rows_limit: i64,
    ) -> Result<ReadRowStream> {
        self.refresh_access_token().await;

        let response = self
            .client
            .read_rows(ReadRowsRequest {
                table_name: format!("{}{}", self.table_prefix, table_name),
                rows_limit,
                rows: Some(RowSet {
                    row_keys: vec![],
                    row_ranges: vec![RowRange {start_key, end_key}],
                }),
                filter: Some(RowFilter {
                    // Only return the latest version of each cell
                    filter: Some(row_filter::Filter::CellsPerColumnLimitFilter(1)),
                }),
                ..ReadRowsRequest::default()
            })
            .await?
            .into_inner();

        Ok(self.stream_read_rows_response(response))
    }

    /// Delete one or more `table` rows
    async fn delete_rows(&mut self, table_name: &str, row_keys: &[RowKey]) -> Result<()> {
        self.refresh_access_token().await;

        let mut entries = vec![];
        for row_key in row_keys {
            entries.push(mutate_rows_request::Entry {
                row_key: row_key.as_bytes().to_vec(),
                mutations: vec![Mutation {
                    mutation: Some(mutation::Mutation::DeleteFromRow(
                        mutation::DeleteFromRow {},
                    )),
                }],
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
                        eprintln!("delete_rows error {}: {}", status.code, status.message);
                        warn!("delete_rows error {}: {}", status.code, status.message);
                        return Err(Error::RowDeleteFailed);
                    }
                }
            }
        }

        Ok(())
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
        let row_data = self.get_single_row_data(table, key.clone()).await?;
        deserialize_bincode_cell_data(&row_data, table, key.to_string())
    }

    pub async fn get_bincode_cells<T>(
        &mut self,
        table: &str,
        keys: &[RowKey],
    ) -> Result<Vec<(RowKey, Result<T>)>>
    where
        T: serde::de::DeserializeOwned,
    {
        Ok(self
            .get_multi_row_data(table, keys)
            .await?
            .into_iter()
            .map(|(key, row_data)| {
                let key_str = key.to_string();
                (
                    key,
                    deserialize_bincode_cell_data(&row_data, table, key_str),
                )
            })
            .collect())
    }

    pub async fn get_protobuf_or_bincode_cell<B, P>(
        &mut self,
        table: &str,
        key: RowKey,
    ) -> Result<CellData<B, P>>
    where
        B: serde::de::DeserializeOwned,
        P: prost::Message + Default,
    {
        let row_data = self.get_single_row_data(table, key.clone()).await?;
        deserialize_protobuf_or_bincode_cell_data(&row_data, table, key)
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

    pub async fn put_protobuf_cells<T>(
        &mut self,
        table: &str,
        cells: &[(RowKey, T)],
    ) -> Result<usize>
    where
        T: prost::Message,
    {
        let mut bytes_written = 0;
        let mut new_row_data = vec![];
        for (row_key, data) in cells {
            let mut buf = Vec::with_capacity(data.encoded_len());
            data.encode(&mut buf).unwrap();
            let data = compress_best(&buf)?;
            bytes_written += data.len();
            new_row_data.push((row_key, vec![("proto".to_string(), data)]));
        }

        self.put_row_data(table, "x", &new_row_data).await?;
        Ok(bytes_written)
    }
}

pub(crate) fn deserialize_protobuf_or_bincode_cell_data<B, P>(
    row_data: RowDataSlice,
    table: &str,
    key: RowKey,
) -> Result<CellData<B, P>>
where
    B: serde::de::DeserializeOwned,
    P: prost::Message + Default,
{
    match deserialize_protobuf_cell_data(row_data, table, key.to_string()) {
        Ok(result) => return Ok(CellData::Protobuf(result)),
        Err(err) => match err {
            Error::ObjectNotFound(_) => {}
            _ => return Err(err),
        },
    }
    deserialize_bincode_cell_data(row_data, table, key).map(CellData::Bincode)
}

pub(crate) fn deserialize_protobuf_cell_data<T>(
    row_data: RowDataSlice,
    table: &str,
    key: RowKey,
) -> Result<T>
where
    T: prost::Message + Default,
{
    let value = &row_data
        .iter()
        .find(|(name, _)| name == "proto")
        .ok_or_else(|| Error::ObjectNotFound(format!("{}/{}", table, key)))?
        .1;

    let data = decompress(value)?;
    T::decode(&data[..]).map_err(|err| {
        warn!("Failed to deserialize {}/{}: {}", table, key, err);
        Error::ObjectCorrupt(format!("{}/{}", table, key))
    })
}

pub(crate) fn deserialize_bincode_cell_data<T>(
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

    let data = decompress(value)?;
    bincode::deserialize(&data).map_err(|err| {
        warn!("Failed to deserialize {}/{}: {}", table, key, err);
        Error::ObjectCorrupt(format!("{}/{}", table, key))
    })
}

pub struct ReadRowStream {
    source: tonic::codec::Streaming<ReadRowsResponse>,

    rows: VecDeque<(RowKey, RowData)>,

    row_key: Option<RowKey>,
    row_data: RowData,

    cell_name: Option<CellName>,
    cell_timestamp: i64,
    cell_value: Vec<u8>,
    cell_version_ok: bool,
}

impl ReadRowStream {
    fn new(source: tonic::codec::Streaming<ReadRowsResponse>) -> Self {
        Self {
            source,
            rows: VecDeque::new(),
            row_key: None,
            row_data: vec![],
            cell_name: None,
            cell_timestamp: 0,
            cell_value: vec![],
            cell_version_ok: true,
        }
    }

    fn read_rows_response(&mut self, res: ReadRowsResponse) {
        for (i, mut chunk) in res.chunks.into_iter().enumerate() {
            // The comments for `read_rows_response::CellChunk` provide essential details for
            // understanding how the below decoding works...
            trace!("chunk {}: {:?}", i, chunk);

            // Starting a new row?
            if !chunk.row_key.is_empty() {
                self.row_key = String::from_utf8(chunk.row_key).ok(); // Require UTF-8 for row keys
            }

            // Starting a new cell?
            if let Some(qualifier) = chunk.qualifier {
                if let Some(cell_name) = self.cell_name.take() {
                    self.row_data.push((cell_name, std::mem::take(&mut self.cell_value)));
                    self.cell_value = vec![];
                }
                self.cell_name = String::from_utf8(qualifier).ok(); // Require UTF-8 for cell names
                self.cell_timestamp = chunk.timestamp_micros;
                self.cell_version_ok = true;
            } else {
                // Continuing the existing cell.  Check if this is the start of another version of the cell
                if chunk.timestamp_micros != 0 {
                    if chunk.timestamp_micros < self.cell_timestamp {
                        self.cell_version_ok = false; // ignore older versions of the cell
                    } else {
                        // newer version of the cell, remove the older cell
                        self.cell_version_ok = true;
                        self.cell_value = vec![];
                        self.cell_timestamp = chunk.timestamp_micros;
                    }
                }
            }
            if self.cell_version_ok {
                self.cell_value.append(&mut chunk.value);
            }

            // End of a row?
            if chunk.row_status.is_some() {
                if let Some(read_rows_response::cell_chunk::RowStatus::CommitRow(_)) =
                    chunk.row_status
                {
                    if let Some(cell_name) = self.cell_name.take() {
                        self.row_data.push((cell_name, std::mem::take(&mut self.cell_value)));
                    }

                    if let Some(row_key) = self.row_key.take() {
                        self.rows.push_back((row_key, std::mem::take(&mut self.row_data)))
                    }
                }

                self.row_key = None;
                self.row_data = vec![];
                self.cell_value = vec![];
                self.cell_name = None;
            }
        }
    }
}

impl Stream for ReadRowStream {
    type Item=Result<(RowKey, RowData)>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Self::Item>> {
        // Fill buffer with new chunks if required.
        if self.rows.is_empty() {
            match Pin::new(&mut self.source).poll_next(cx) {
                Poll::Ready(Some(Ok(chunks))) =>
                    self.read_rows_response(chunks),
                Poll::Ready(Some(Err(e))) =>
                    return Poll::Ready(Some(Err(e.into()))),
                Poll::Ready(None) =>
                    return Poll::Ready(None),
                Poll::Pending =>
                    return Poll::Pending,
            }
        }
        // Return next row in buffer.
        if let Some(row) = self.rows.pop_front() {
            Poll::Ready(Some(Ok(row)))
        } else {
            Poll::Pending
        }
    }
}

/*
struct RetryReadRowStream<F: FnMut(Request<()>) -> InterceptedRequestResult> {
    bigtable: BigTable<F>,
    stream: Option<ReadRowStream>,
}

impl<F: FnMut(Request<()>) -> InterceptedRequestResult> RetryReadRowStream<F> {
    fn new(bigtable: BigTable<F>) -> Self {
        Self {
            bigtable,
            stream: None,
        }
    }

    async fn start_stream(&mut self) {
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

        Ok(self.stream_read_rows_response(response))
    }
}

impl<F: FnMut(Request<()>) -> InterceptedRequestResult> Stream for RetryReadRowStream<F> {
    type Item = ();

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {

    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let hint = self.stream.expect("missing stream").size_hint();
        (hint.0, None)
    }
}
*/

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::StoredConfirmedBlock,
        prost::Message,
        solana_sdk::{hash::Hash, signature::Keypair, system_transaction},
        solana_storage_proto::convert::generated,
        solana_transaction_status::{
            ConfirmedBlock, TransactionStatusMeta, TransactionWithStatusMeta,
        },
        std::convert::TryInto,
    };

    #[test]
    fn test_deserialize_protobuf_or_bincode_cell_data() {
        let from = Keypair::new();
        let recipient = solana_sdk::pubkey::new_rand();
        let transaction = system_transaction::transfer(&from, &recipient, 42, Hash::default());
        let with_meta = TransactionWithStatusMeta {
            transaction,
            meta: Some(TransactionStatusMeta {
                status: Ok(()),
                fee: 1,
                pre_balances: vec![43, 0, 1],
                post_balances: vec![0, 42, 1],
                inner_instructions: Some(vec![]),
                log_messages: Some(vec![]),
                pre_token_balances: Some(vec![]),
                post_token_balances: Some(vec![]),
                rewards: Some(vec![]),
            }),
        };
        let block = ConfirmedBlock {
            transactions: vec![with_meta],
            parent_slot: 1,
            blockhash: Hash::default().to_string(),
            previous_blockhash: Hash::default().to_string(),
            rewards: vec![],
            block_time: Some(1_234_567_890),
            block_height: Some(1),
        };
        let bincode_block = compress_best(
            &bincode::serialize::<StoredConfirmedBlock>(&block.clone().into()).unwrap(),
        )
        .unwrap();

        let protobuf_block = generated::ConfirmedBlock::from(block.clone());
        let mut buf = Vec::with_capacity(protobuf_block.encoded_len());
        protobuf_block.encode(&mut buf).unwrap();
        let protobuf_block = compress_best(&buf).unwrap();

        let deserialized = deserialize_protobuf_or_bincode_cell_data::<
            StoredConfirmedBlock,
            generated::ConfirmedBlock,
        >(
            &[("proto".to_string(), protobuf_block.clone())],
            "",
            "".to_string(),
        )
        .unwrap();
        if let CellData::Protobuf(protobuf_block) = deserialized {
            assert_eq!(block, protobuf_block.try_into().unwrap());
        } else {
            panic!("deserialization should produce CellData::Protobuf");
        }

        let deserialized = deserialize_protobuf_or_bincode_cell_data::<
            StoredConfirmedBlock,
            generated::ConfirmedBlock,
        >(
            &[("bin".to_string(), bincode_block.clone())],
            "",
            "".to_string(),
        )
        .unwrap();
        if let CellData::Bincode(bincode_block) = deserialized {
            let mut block = block;
            if let Some(meta) = &mut block.transactions[0].meta {
                meta.inner_instructions = None; // Legacy bincode implementation does not support inner_instructions
                meta.log_messages = None; // Legacy bincode implementation does not support log_messages
                meta.pre_token_balances = None; // Legacy bincode implementation does not support token balances
                meta.post_token_balances = None; // Legacy bincode implementation does not support token balances
                meta.rewards = None; // Legacy bincode implementation does not support rewards
            }
            assert_eq!(block, bincode_block.into());
        } else {
            panic!("deserialization should produce CellData::Bincode");
        }

        let result = deserialize_protobuf_or_bincode_cell_data::<
            StoredConfirmedBlock,
            generated::ConfirmedBlock,
        >(&[("proto".to_string(), bincode_block)], "", "".to_string());
        assert!(result.is_err());

        let result = deserialize_protobuf_or_bincode_cell_data::<
            StoredConfirmedBlock,
            generated::ConfirmedBlock,
        >(
            &[("proto".to_string(), vec![1, 2, 3, 4])],
            "",
            "".to_string(),
        );
        assert!(result.is_err());

        let result = deserialize_protobuf_or_bincode_cell_data::<
            StoredConfirmedBlock,
            generated::ConfirmedBlock,
        >(&[("bin".to_string(), protobuf_block)], "", "".to_string());
        assert!(result.is_err());

        let result = deserialize_protobuf_or_bincode_cell_data::<
            StoredConfirmedBlock,
            generated::ConfirmedBlock,
        >(&[("bin".to_string(), vec![1, 2, 3, 4])], "", "".to_string());
        assert!(result.is_err());
    }
}
