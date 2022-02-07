use frame::event;
use itertools::Itertools;
use stream::{
    data::parse_next_frame,
    header::{parse_headers, Header},
    predictor::{LogProcessor, LogRecord},
};
use thiserror::Error;

extern crate itertools;

pub mod frame;
pub(crate) mod stream;

pub enum BlackboxRecord<'a> {
    Main(&'a [i64]),
    GNSS(&'a [i64]),
    Slow(Vec<i64>),
    Event(event::Frame),
    Garbage(usize),
}

pub struct BlackboxReader<'a> {
    last_values: Vec<i64>,
    remaining_bytes: &'a [u8],
    original_length: usize,
    pub header: Header,
    processor: LogProcessor,
    pub last_loop_iteration: i64,
    pub last_time: i64,
    loop_iteration_field_ix: usize,
    time_field_ix: usize,
}

#[derive(Error, Debug)]
pub enum BlackboxReaderError {
    #[error("couldn't parse header")]
    ParseHeader,
    #[error("loopIteration or time I/P fields have not been found")]
    NoLoopIterationAndTime,
    #[error("log is truncated")]
    Incomplete,
}

impl<'a> BlackboxReader<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<BlackboxReader<'a>, BlackboxReaderError> {
        let original_length = bytes.len();
        let (remaining_bytes, header) = parse_headers(bytes).map_err(|e| match e {
            nom::Err::Error(_e) => BlackboxReaderError::ParseHeader,
            nom::Err::Failure(_e) => BlackboxReaderError::ParseHeader,
            nom::Err::Incomplete(_) => BlackboxReaderError::Incomplete,
        })?;

        let loop_iteration_field_ix = header
            .ip_fields_in_order
            .iter()
            .find_position(|f| f.name == "loopIteration")
            .ok_or(BlackboxReaderError::NoLoopIterationAndTime)?
            .0;

        let time_field_ix = header
            .ip_fields_in_order
            .iter()
            .find_position(|f| f.name == "time")
            .ok_or(BlackboxReaderError::NoLoopIterationAndTime)?
            .0;

        let last_values = Vec::with_capacity(
            header
                .ip_fields_in_order
                .len()
                .max(header.s_fields_in_order.len())
                .max(header.g_fields_in_order.len()),
        );

        Ok(BlackboxReader {
            remaining_bytes,
            original_length,
            processor: LogProcessor::new(&header),
            last_values,
            loop_iteration_field_ix,
            time_field_ix,
            header,
            last_loop_iteration: 0,
            last_time: 0,
        })
    }

    pub fn next(&mut self) -> Option<BlackboxRecord> {
        loop {
            match parse_next_frame(&self.header, self.remaining_bytes) {
                Ok((remaining_bytes, frame)) => {
                    self.remaining_bytes = remaining_bytes;
                    if let Some(record) = self.processor.process_frame(frame) {
                        return Some(match record {
                            LogRecord::Main(values) => {
                                self.last_loop_iteration = values[self.loop_iteration_field_ix];
                                self.last_time = values[self.time_field_ix];
                                self.last_values.clear();
                                self.last_values.extend_from_slice(values);
                                BlackboxRecord::Main(&self.last_values)
                            }
                            LogRecord::GNSS(values) => {
                                self.last_values.clear();
                                self.last_values.extend_from_slice(values);
                                BlackboxRecord::GNSS(&self.last_values)
                            }
                            LogRecord::Slow(values) => BlackboxRecord::Slow(values),
                            LogRecord::Event(event) => BlackboxRecord::Event(event),
                        });
                    }
                }
                Err(e) => match e {
                    nom::Err::Error(e) => {
                        if e.input.len() > 0 {
                            self.remaining_bytes = &e.input[1..];
                        }
                    }
                    nom::Err::Failure(e) => {
                        if e.input.len() > 0 {
                            self.remaining_bytes = &e.input[1..];
                        }
                    }
                    nom::Err::Incomplete(_) => {
                        return None;
                    }
                },
            }
        }
    }

    pub fn bytes_read(&self) -> usize {
        self.original_length - self.remaining_bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use std::{path::{Path, PathBuf}, fs::File, io::Read};

    use crate::BlackboxReader;

    #[derive(Default)]
    struct LogStats {
        main: usize,
        gnss: usize,
        slow: usize,
        event: usize,
        garbage: usize,
    }

    trait BlackboxReaderExt {
        fn consume(&mut self) -> LogStats;
    }

    impl <'a> BlackboxReaderExt for BlackboxReader<'a> {
        fn consume(&mut self) -> LogStats {
            let mut stats = LogStats::default();

            while let Some(record) = self.next() {
                match record {
                    crate::BlackboxRecord::Main(_) => stats.main += 1,
                    crate::BlackboxRecord::GNSS(_) => stats.gnss += 1,
                    crate::BlackboxRecord::Slow(_) => stats.slow += 1,
                    crate::BlackboxRecord::Event(_) => stats.event += 1,
                    crate::BlackboxRecord::Garbage(_) => stats.garbage += 1,
                }
            }

            stats
        }
    }

    fn with_log(filename: impl AsRef<Path>, f: impl Fn(BlackboxReader)) {
        with_log_result(filename, |r| {
            f(r);
            Ok(())
        }).unwrap()
    }

    fn with_log_result(filename: impl AsRef<Path>, f: impl Fn(BlackboxReader) -> Result<(), anyhow::Error>) -> Result<(), anyhow::Error> {
        let filename = filename.as_ref();
        assert!(filename.is_relative());

        let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).canonicalize()?;
        root.push("test-data");

        let filename = root.join(filename).canonicalize()?;
        assert!(filename.starts_with(root));

        let mut buf = Vec::new();
        File::open(filename)?.read_to_end(&mut buf)?;
        let reader = BlackboxReader::from_bytes(&buf)?;
        f(reader)?;
        Ok(())
    }

    #[test]
    fn emuflight_v3_7_0() {
        with_log("crashing-LOG00002.BFL", |mut r| {
            let stats = r.consume();
            assert_eq!(r.remaining_bytes.len(), 0);
            assert_eq!(stats.main, 171816);
            assert_eq!(stats.garbage, 0);
        })
    }

    #[test]
    fn betaflight_v4_2_6() {
        with_log("LOG00002.BFL", |mut r| {
            let stats = r.consume();
            assert_eq!(r.remaining_bytes.len(), 0);
            assert_eq!(stats.main, 222394);
            assert_eq!(stats.garbage, 0);
        })
    }

    #[test]
    fn inav_v3_0_1() {
        with_log("LOG00004.TXT", |mut r| {
            let stats = r.consume();
            assert_eq!(r.remaining_bytes.len(), 0);
            assert_eq!(stats.main, 200815);
            assert_eq!(stats.garbage, 0);
        })
    }
}
