//! The input log's archive in object storage (D54, D72): Kafka holds it 30 days, the archive a
//! year. One core instance's log is a series of segment objects,
//! `<prefix>/<core instance>/<first seq, 20 digits>.pb`, each a run of length-delimited
//! `omnimarket.det.v1.InputRecord`s in log order.

use std::fmt;
use std::sync::Arc;

use futures::TryStreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use prost::Message as _;

use crate::record::{InputRecord, InvalidInputRecord};

/// Records per segment object.
const SEGMENT: usize = 10_000;

#[derive(Debug)]
pub enum ArchiveError {
    Store(object_store::Error),
    Decode(prost::DecodeError),
    Invalid(InvalidInputRecord),
    /// Segments must hold one unbroken run of `seq` from 0.
    Gap {
        expected: u64,
        found: u64,
    },
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => write!(f, "object store: {e}"),
            Self::Decode(e) => write!(f, "an archived input record doesn't decode: {e}"),
            Self::Invalid(e) => write!(f, "an archived input record is invalid: {e:?}"),
            Self::Gap { expected, found } => {
                write!(
                    f,
                    "archived input records have a gap: expected seq {expected}, found {found}"
                )
            }
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<object_store::Error> for ArchiveError {
    fn from(e: object_store::Error) -> Self {
        Self::Store(e)
    }
}

pub struct S3Config {
    pub endpoint: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

pub struct InputArchive {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl InputArchive {
    /// An archive under `prefix` (e.g. `inputs/base`) in any object store.
    pub fn new(store: Arc<dyn ObjectStore>, prefix: &str) -> Self {
        Self {
            store,
            prefix: prefix.trim_end_matches('/').to_string(),
        }
    }

    /// An S3-compatible store, such as the local stack's RustFS.
    pub fn s3(config: &S3Config, prefix: &str) -> Result<Self, ArchiveError> {
        let store = AmazonS3Builder::new()
            .with_endpoint(&config.endpoint)
            .with_bucket_name(&config.bucket)
            .with_access_key_id(&config.access_key)
            .with_secret_access_key(&config.secret_key)
            .with_region("us-east-1")
            .with_allow_http(config.endpoint.starts_with("http://"))
            .build()?;
        Ok(Self::new(Arc::new(store), prefix))
    }

    fn segment(&self, core_instance: &str, first_seq: u64) -> Path {
        Path::from(format!(
            "{}/{core_instance}/{first_seq:020}.pb",
            self.prefix
        ))
    }

    /// Writes a core instance's records, in log order. Writing the same records again writes
    /// the same objects, so an interrupted archive can simply run again.
    pub async fn write(
        &self,
        core_instance: &str,
        records: &[InputRecord],
    ) -> Result<usize, ArchiveError> {
        let mut segments = 0;
        for chunk in records.chunks(SEGMENT) {
            let mut bytes = Vec::new();
            for record in chunk {
                record
                    .to_proto()
                    .encode_length_delimited(&mut bytes)
                    .expect("a Vec grows to fit");
            }
            self.store
                .put(
                    &self.segment(core_instance, chunk[0].seq),
                    PutPayload::from(bytes),
                )
                .await?;
            segments += 1;
        }
        Ok(segments)
    }

    /// Reads a core instance's records back, in log order.
    pub async fn read(&self, core_instance: &str) -> Result<Vec<InputRecord>, ArchiveError> {
        let prefix = Path::from(format!("{}/{core_instance}", self.prefix));
        let mut paths: Vec<Path> = self
            .store
            .list(Some(&prefix))
            .map_ok(|meta| meta.location)
            .try_collect()
            .await?;
        // Zero-padded sequence numbers sort in log order.
        paths.sort();
        let mut records = Vec::new();
        for path in paths {
            let bytes = self.store.get(&path).await?.bytes().await?;
            let mut rest = bytes.as_ref();
            while !rest.is_empty() {
                let proto = proto::det::v1::InputRecord::decode_length_delimited(&mut rest)
                    .map_err(ArchiveError::Decode)?;
                let record = InputRecord::from_proto(proto).map_err(ArchiveError::Invalid)?;
                let expected = records.len() as u64;
                if record.seq != expected {
                    return Err(ArchiveError::Gap {
                        expected,
                        found: record.seq,
                    });
                }
                records.push(record);
            }
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use object_store::memory::InMemory;
    use types::Timestamp;

    use super::*;
    use crate::Source;

    fn records(n: u64) -> Vec<InputRecord> {
        (0..n)
            .map(|seq| InputRecord {
                seq,
                source: if seq == 0 {
                    Source::Config
                } else {
                    Source::Clock
                },
                arrived: Timestamp::from_unix_nanos(seq * 7),
                payload: seq.to_le_bytes().to_vec(),
            })
            .collect()
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn a_log_round_trips_across_segments() {
        let archive = InputArchive::new(Arc::new(InMemory::new()), "inputs/base");
        let log = records(25_001);
        let (segments, back) = block_on(async {
            let segments = archive.write("base-1", &log).await.unwrap();
            // Another instance's log in the same prefix is not read back.
            archive.write("base-2", &records(3)).await.unwrap();
            (segments, archive.read("base-1").await.unwrap())
        });
        assert_eq!(segments, 3);
        assert_eq!(back, log);
    }

    #[test]
    fn a_missing_segment_is_a_gap() {
        let store = Arc::new(InMemory::new());
        let archive = InputArchive::new(store.clone(), "inputs/base");
        let result = block_on(async {
            archive.write("base-1", &records(20_000)).await.unwrap();
            store
                .delete(&Path::from("inputs/base/base-1/00000000000000000000.pb"))
                .await
                .unwrap();
            archive.read("base-1").await
        });
        assert!(matches!(
            result,
            Err(ArchiveError::Gap {
                expected: 0,
                found: 10_000
            })
        ));
    }
}
