use std::{path::Path, fs::File, io::Read};
use serde_big_array::BigArray;

use insta::{assert_yaml_snapshot, glob};
use serde::{Deserialize, Serialize};

use crate::{BlackboxReader, MultiSegmentBlackboxReader, BlackboxReaderError};

#[test]
fn log_stats() {
    glob!("test-data/*", |path| {
        assert_yaml_snapshot!(multilog_stats(path));
    });
}

#[derive(Deserialize, Serialize)]
struct SignedLog2Histogram<const N: usize, const Strict: bool> {
    #[serde(with = "BigArray")]
    neg: [usize; N],
    zero: usize,
    #[serde(with = "BigArray")]
    pos: [usize; N],
}

impl <const N: usize, const Strict: bool> SignedLog2Histogram<N, Strict> {
    pub fn push(&mut self, v: i64) {
        if v == 0 {
            self.zero += 1;
        } else {
            let is_positive = v.is_positive();
            let v = v.saturating_abs();
            let mut bin = 63usize - v.leading_zeros() as usize;
            if bin >= N {
                if Strict {
                    panic!("");
                } else {
                    bin = N - 1;
                }
            }

            if is_positive {
                self.pos[bin] += 1;
            } else {
                self.neg[N - bin - 1] += 1;
            }
        }
    }
}

impl <const N: usize, const Strict: bool> Default for SignedLog2Histogram<N, Strict> {
    fn default() -> Self {
        Self { 
            neg: [0usize; N],
            zero: 0,
            pos: [0usize; N],
        }
    }
}

#[test]
fn signed_histogram_works_for_0() {
    let mut histo: SignedLog2Histogram<4, false> = Default::default();
    histo.push(0);

    assert_eq!(histo.zero, 1);
    assert_eq!(histo.neg, [0; 4]);
    assert_eq!(histo.pos, [0; 4]);
}

#[test]
fn signed_histogram_works_for_1() {
    let mut histo: SignedLog2Histogram<4, false> = Default::default();
    histo.push(1);

    assert_eq!(histo.zero, 0);
    assert_eq!(histo.neg, [0; 4]);
    assert_eq!(histo.pos, [1, 0, 0, 0]);
}

#[test]
fn signed_histogram_works_for_second_bucket() {
    let mut histo: SignedLog2Histogram<4, false> = Default::default();
    histo.push(2);
    histo.push(3);

    assert_eq!(histo.zero, 0);
    assert_eq!(histo.neg, [0; 4]);
    assert_eq!(histo.pos, [0, 2, 0, 0]);
}

#[test]
fn signed_histogram_works_for_third_bucket() {
    let mut histo: SignedLog2Histogram<4, false> = Default::default();
    histo.push(4);
    histo.push(7);

    assert_eq!(histo.zero, 0);
    assert_eq!(histo.neg, [0; 4]);
    assert_eq!(histo.pos, [0, 0, 2, 0]);
}

#[test]
fn signed_histogram_works_for_the_last_bucket() {
    let mut histo: SignedLog2Histogram<4, false> = Default::default();
    histo.push(i64::MAX);
    histo.push((i64::MAX >> 1) + 1);

    assert_eq!(histo.zero, 0);
    assert_eq!(histo.neg, [0; 4]);
    assert_eq!(histo.pos, [0, 0, 0, 2]);
}

#[test]
#[should_panic]
fn strict_signed_histogram_panics_for_the_last_bucket() {
    let mut histo: SignedLog2Histogram<4, true> = Default::default();
    histo.push(i64::MAX);
    histo.push((i64::MAX >> 1) + 1);

    assert_eq!(histo.zero, 0);
    assert_eq!(histo.neg, [0; 4]);
    assert_eq!(histo.pos, [0, 0, 0, 2]);
}

#[derive(Default, Deserialize, Serialize)]
struct LogStats {
    main: usize,
    gnss: usize,
    slow: usize,
    event: usize,
    garbage: usize,
    remaining_bytes: usize,
    gyro_adc0_histo: SignedLog2Histogram<32, true>,
}

trait BlackboxReaderExt {
    fn consume(&mut self) -> LogStats;
}

impl <'a> BlackboxReaderExt for BlackboxReader<'a> {
    fn consume(&mut self) -> LogStats {
        let mut stats = LogStats::default();

        let gyro_adc0_field_ix = self.header.ip_fields["gyroADC[0]"].ix;

        while let Some(record) = self.next() {
            match record {
                crate::BlackboxRecord::Main(record) => {
                    stats.main += 1;
                    stats.gyro_adc0_histo.push(record[gyro_adc0_field_ix]);
                },
                crate::BlackboxRecord::GNSS(_) => stats.gnss += 1,
                crate::BlackboxRecord::Slow(_) => stats.slow += 1,
                crate::BlackboxRecord::Event(_) => stats.event += 1,
                crate::BlackboxRecord::Garbage(_) => stats.garbage += 1,
            }
        }

        stats.remaining_bytes = self.remaining_bytes.len();

        stats
    }
}

trait MultiSegmentBlackboxReaderExt {
    fn consume(&mut self) -> Vec<Result<LogStats, BlackboxReaderError>>;
}

impl<'a> MultiSegmentBlackboxReaderExt for MultiSegmentBlackboxReader<'a> {
    fn consume(&mut self) -> Vec<Result<LogStats, BlackboxReaderError>> {
        self.map(|r| r.map(|mut r| r.consume())).collect()
    }
}

fn with_log<T>(filename: impl AsRef<Path>, f: impl Fn(BlackboxReader) -> T) -> T {
    with_log_result(filename, |r| {
        Ok(f(r))
    }).unwrap()
}

fn with_log_result<T>(filename: impl AsRef<Path>, f: impl Fn(BlackboxReader) -> Result<T, anyhow::Error>) -> Result<T, anyhow::Error> {
    let mut buf = Vec::new();
    File::open(filename)?.read_to_end(&mut buf)?;
    let reader = BlackboxReader::from_bytes(&buf)?;
    f(reader)
}

fn stats(filename: impl AsRef<Path>) -> LogStats {
    with_log(filename, |mut r| {
        r.consume()
    })
}

fn with_multilog<T>(filename: impl AsRef<Path>, f: impl Fn(MultiSegmentBlackboxReader) -> T) -> T {
    with_multilog_result(filename, |r| {
        Ok(f(r))
    }).unwrap()
}

fn with_multilog_result<T>(filename: impl AsRef<Path>, f: impl Fn(MultiSegmentBlackboxReader) -> Result<T, anyhow::Error>) -> Result<T, anyhow::Error> {
    let mut buf = Vec::new();
    File::open(filename)?.read_to_end(&mut buf)?;
    let reader = MultiSegmentBlackboxReader::from_bytes(&buf);
    f(reader)
}

fn multilog_stats(filename: impl AsRef<Path>) -> Vec<Result<LogStats, BlackboxReaderError>> {
    with_multilog(filename, |mut r| {
        r.consume()
    })
}
