use frame::event;
use itertools::Itertools;
use stream::{
    data::parse_next_frame,
    header::{parse_headers, Header},
    predictor::{LogProcessor, LogRecord},
};
use thiserror::Error;

extern crate itertools;

pub(crate) mod frame;
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
        let (remaining_bytes, header) = parse_headers(bytes).map_err(|e| match e {
            nom::Err::Error(e) => BlackboxReaderError::ParseHeader,
            nom::Err::Failure(e) => BlackboxReaderError::ParseHeader,
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
}

#[cfg(test)]
mod tests {}
