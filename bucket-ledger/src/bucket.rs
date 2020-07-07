// This module defines a minimal API for working with bucket storage
use log::*;
use solana_sdk::hash::hash;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("AWSRegionError: {0}")]
    AwsRegionError(awsregion::AwsRegionError),

    #[error("AWSCredsError: {0}")]
    AwsCredsError(awscreds::AwsCredsError),

    #[error("I/O Error: {0}")]
    IoError(std::io::Error),

    #[error("Object get failed with status code {0}")]
    ObjectGetFailure(u16),

    #[error("Object put failed with status code {0}")]
    ObjectPutFailure(u16),

    #[error("Object list failed with status code {0}")]
    ObjectListFailure(u16),

    #[error("S3Error: {0}")]
    S3Error(s3::S3Error),

    #[error("Operation timeout: {0}")]
    Timeout(tokio::time::Elapsed),
}

impl std::convert::From<awsregion::AwsRegionError> for Error {
    fn from(err: awsregion::AwsRegionError) -> Self {
        Self::AwsRegionError(err)
    }
}

impl std::convert::From<awscreds::AwsCredsError> for Error {
    fn from(err: awscreds::AwsCredsError) -> Self {
        Self::AwsCredsError(err)
    }
}

impl std::convert::From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl std::convert::From<s3::S3Error> for Error {
    fn from(err: s3::S3Error) -> Self {
        Self::S3Error(err)
    }
}

impl std::convert::From<tokio::time::Elapsed> for Error {
    fn from(err: tokio::time::Elapsed) -> Self {
        Self::Timeout(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Bucket {
    s3_bucket: s3::bucket::Bucket,
    timeout: Duration,
}

/// Maximum number of objects keys that will be returned by an object listing
pub const MAX_LIST_KEYS: usize = 1_000;

impl Bucket {
    pub fn new(bucket_name: &str, region: &str) -> Result<Self> {
        let credentials = awscreds::Credentials::from_env()?;
        let s3_bucket = s3::bucket::Bucket::new(bucket_name, region.parse()?, credentials)?;
        let timeout = Duration::from_secs(30);
        Ok(Self { s3_bucket, timeout })
    }

    /// Get an object
    pub async fn get_object<S: AsRef<str>>(&self, key: S) -> Result<Vec<u8>> {
        let (data, status_code) =
            tokio::time::timeout(self.timeout, self.s3_bucket.get_object(&key)).await??;
        debug!(
            "get {} -> {} (len={}, hash={})",
            key.as_ref(),
            status_code,
            data.len(),
            hash(&data),
        );
        if status_code != 200 {
            info!(
                "get failed: {}: {}",
                status_code,
                String::from_utf8_lossy(&data)
            );
            Err(Error::ObjectGetFailure(status_code))
        } else {
            Ok(data)
        }
    }

    /// Put an object
    pub async fn put_object<S: AsRef<str>>(&self, key: S, data: &[u8]) -> Result<()> {
        let key = key.as_ref();
        let (response, status_code) = tokio::time::timeout(
            self.timeout,
            self.s3_bucket
                .put_object(key, data, "application/octet-stream"),
        )
        .await??;
        debug!(
            "put {} -> {} (len={}, hash={})",
            key,
            status_code,
            data.len(),
            hash(data)
        );
        if status_code != 200 {
            info!(
                "put failed: {}: {}",
                status_code,
                String::from_utf8_lossy(&response)
            );
            Err(Error::ObjectPutFailure(status_code))
        } else {
            Ok(())
        }
    }

    /// Return objects keys in lexical order
    ///
    /// prefix: only return keys with this prefix
    /// start_after: object key to start the listing at, may be any bucket object
    /// max_keys: number of object keys to return, between 1 and `MAX_LIST_KEYS`.  Default is
    /// `MAX_LIST_KEYS`
    pub async fn object_keys<S: AsRef<str>>(
        &self,
        prefix: S,
        start_after: Option<String>,
        max_keys: Option<usize>,
    ) -> Result<Vec<String>> {
        let (list_results, status_code) = tokio::time::timeout(
            self.timeout,
            self.s3_bucket.list_page(
                prefix.as_ref().to_string(),
                None,
                None,
                start_after.clone(),
                max_keys,
            ),
        )
        .await??;

        debug!(
            "object_keys: prefix='{}',start_after='{:?}' -> {} (len={})",
            prefix.as_ref(),
            start_after,
            status_code,
            list_results.contents.len()
        );
        if status_code != 200 {
            info!("list failed: {}", status_code);
            Err(Error::ObjectListFailure(status_code))
        } else {
            let object_keys = list_results
                .contents
                .into_iter()
                .map(|object| object.key)
                .collect::<Vec<_>>();
            Ok(object_keys)
        }
    }
}
