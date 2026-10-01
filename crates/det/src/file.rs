//! One core instance's input log as a single file (D99): the length-delimited
//! `omnimarket.det.v1.InputRecord`s of the archive's segments, in log order, compressed with
//! zstd. The pinned replay fixtures in the repository use it.

use std::fmt;

use prost::Message as _;

use crate::record::{InputRecord, InvalidInputRecord};

/// zstd's level for fixtures: written once, read on every test run.
const LEVEL: i32 = 19;

#[derive(Debug)]
pub enum LogFileError {
    /// The bytes aren't zstd.
    Decompress(std::io::Error),
    Decode(prost::DecodeError),
    Invalid(InvalidInputRecord),
    /// A log file holds one unbroken run of `seq` from 0.
    Gap {
        expected: u64,
        found: u64,
    },
}

impl fmt::Display for LogFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decompress(e) => write!(f, "the input log file isn't zstd: {e}"),
            Self::Decode(e) => write!(f, "an input record in the file doesn't decode: {e}"),
            Self::Invalid(e) => write!(f, "an input record in the file is invalid: {e:?}"),
            Self::Gap { expected, found } => write!(
                f,
                "the input log file has a gap: expected seq {expected}, found {found}"
            ),
        }
    }
}

impl std::error::Error for LogFileError {}

/// A log's records, in log order, as file bytes.
pub fn encode_log_file(records: &[InputRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        record
            .to_proto()
            .encode_length_delimited(&mut bytes)
            .expect("a Vec grows to fit");
    }
    zstd::encode_all(bytes.as_slice(), LEVEL).expect("compressing into memory doesn't fail")
}

/// A log file's records, in log order.
pub fn decode_log_file(file: &[u8]) -> Result<Vec<InputRecord>, LogFileError> {
    let bytes = zstd::decode_all(file).map_err(LogFileError::Decompress)?;
    let mut rest = bytes.as_slice();
    let mut records = Vec::new();
    while !rest.is_empty() {
        let proto = proto::det::v1::InputRecord::decode_length_delimited(&mut rest)
            .map_err(LogFileError::Decode)?;
        let record = InputRecord::from_proto(proto).map_err(LogFileError::Invalid)?;
        let expected = records.len() as u64;
        if record.seq != expected {
            return Err(LogFileError::Gap {
                expected,
                found: record.seq,
            });
        }
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use types::Timestamp;

    use super::*;
    use crate::Source;

    fn record(seq: u64) -> InputRecord {
        InputRecord {
            seq,
            source: if seq == 0 {
                Source::Config
            } else {
                Source::Event
            },
            arrived: Timestamp::from_unix_nanos(seq * 7),
            payload: vec![seq as u8; 40],
        }
    }

    #[test]
    fn a_log_round_trips_compressed() {
        let log: Vec<InputRecord> = (0..1_000).map(record).collect();
        let file = encode_log_file(&log);
        let raw: usize = log.iter().map(|r| r.to_proto().encoded_len()).sum();
        assert!(file.len() < raw / 4, "{} bytes from {raw}", file.len());
        assert_eq!(decode_log_file(&file).unwrap(), log);
    }

    #[test]
    fn an_empty_log_round_trips() {
        assert_eq!(decode_log_file(&encode_log_file(&[])).unwrap(), Vec::new());
    }

    #[test]
    fn a_gap_is_rejected() {
        let file = encode_log_file(&[record(0), record(1), record(3)]);
        assert!(matches!(
            decode_log_file(&file),
            Err(LogFileError::Gap {
                expected: 2,
                found: 3
            })
        ));
    }

    #[test]
    fn a_gap_says_where() {
        let error = decode_log_file(&encode_log_file(&[record(0), record(2)])).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the input log file has a gap: expected seq 1, found 2"
        );
    }

    #[test]
    fn bytes_that_arent_zstd_are_rejected() {
        assert!(matches!(
            decode_log_file(b"not a log"),
            Err(LogFileError::Decompress(_))
        ));
    }

    #[test]
    fn a_cut_record_is_rejected() {
        let mut bytes = Vec::new();
        record(0)
            .to_proto()
            .encode_length_delimited(&mut bytes)
            .unwrap();
        bytes.pop();
        let file = zstd::encode_all(bytes.as_slice(), 1).unwrap();
        assert!(matches!(
            decode_log_file(&file),
            Err(LogFileError::Decode(_))
        ));
    }
}
