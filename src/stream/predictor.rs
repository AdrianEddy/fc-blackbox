use std::collections::HashMap;

use num_rational::Ratio;

use crate::frame::{
    data::{OwnedGFrame, OwnedHFrame, OwnedIFrame, OwnedPFrame, OwnedSFrame},
    event, BodyFrame,
};

use super::header::{Header, IPField};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldPredictor {
    None,
    Previous,
    StraightLine,
    Average2,
    MinThrottle,
    Motor0,
    Increment,
    HomeCoordinates,
    Around1500,
    VBatRef,
    LastMainFrameTime,
    MinMotor,
}

impl Default for FieldPredictor {
    fn default() -> Self {
        FieldPredictor::None
    }
}

pub(crate) struct History {
    history: [Vec<i64>; 2],
    current: Vec<i64>,
    previous_2_ix: usize,
    previous_ix: usize,
}

pub(crate) struct GNSSHistory {
    gnss_home: [i64; 2],
    pub(crate) history: History,
}

impl GNSSHistory {
    pub fn with_size(cap: usize) -> Self {
        Self {
            gnss_home: Default::default(),
            history: History::with_size(cap),
        }
    }
}

pub(crate) struct Snapshot<'a> {
    previous_2: &'a [i64],
    pub previous: &'a [i64],
    pub current: &'a mut [i64],
}

impl History {
    pub fn with_size(cap: usize) -> Self {
        Self {
            history: [vec![0; cap], vec![0; cap]],
            current: vec![0; cap],
            previous_2_ix: 0,
            previous_ix: 1,
        }
    }

    pub fn values(&self) -> &[i64] {
        &self.history[self.previous_ix]
    }

    pub fn state(&mut self) -> Snapshot {
        Snapshot {
            previous_2: &self.history[self.previous_2_ix],
            previous: &self.history[self.previous_ix],
            current: &mut self.current,
        }
    }

    pub fn advance(&mut self) {
        std::mem::swap(&mut self.previous_ix, &mut self.previous_2_ix);
        self.history[self.previous_ix].copy_from_slice(&self.current);
    }

    pub fn advance_reset(&mut self) {
        self.history[self.previous_2_ix].copy_from_slice(&self.current);
        self.history[self.previous_ix].copy_from_slice(&self.current);
    }
}

#[allow(clippy::upper_case_acronyms)]
pub enum LogRecord<'a> {
    Main(&'a [i64]),
    GNSS(&'a [i64]),
    Slow(Vec<i64>),
    Event(event::Frame),
}

pub struct LogProcessor {
    ip_history: History,
    gnss_history: GNSSHistory,
    i_predictors: Vec<AnyIPredictor>,
    p_predictors: Vec<AnyPPredictor>,
    g_predictors: Vec<AnyGPredictor>,
}

impl LogProcessor {
    pub fn new(header: &Header) -> Self {
        let i_predictors = header.i_field_predictors.clone();
        let p_predictors = header.p_field_predictors.clone();
        let g_predictors = header.g_field_predictors.clone();

        assert_eq!(i_predictors.len(), p_predictors.len());

        Self {
            ip_history: History::with_size(i_predictors.len()),
            gnss_history: GNSSHistory::with_size(g_predictors.len()),
            i_predictors,
            p_predictors,
            g_predictors,
        }
    }

    pub(crate) fn process_frame(&mut self, frame: BodyFrame) -> Option<LogRecord> {
        match frame {
            BodyFrame::IFrame(OwnedIFrame { buf }) => {
                assert_eq!(buf.len(), self.i_predictors.len());
                let mut snapshot = self.ip_history.state();
                for (in_value, predictor) in buf.into_iter().zip(self.i_predictors.iter()) {
                    predictor.predict(in_value, &mut snapshot);
                }
                self.ip_history.advance_reset();
                Some(LogRecord::Main(self.ip_history.values()))
            }
            BodyFrame::PFrame(OwnedPFrame { buf }) => {
                assert_eq!(buf.len(), self.p_predictors.len());
                let mut snapshot = self.ip_history.state();
                for (in_value, predictor) in buf.into_iter().zip(self.p_predictors.iter_mut()) {
                    predictor.predict(in_value, &mut snapshot);
                }
                self.ip_history.advance();
                Some(LogRecord::Main(self.ip_history.values()))
            }
            BodyFrame::HFrame(OwnedHFrame { buf }) => {
                if buf.len() == 2 {
                    self.gnss_history.gnss_home[0] = buf[0];
                    self.gnss_history.gnss_home[1] = buf[1];
                } else if buf.is_empty() {
                    // TODO: log
                }

                None
            }
            BodyFrame::GFrame(OwnedGFrame { buf }) => {
                assert_eq!(buf.len(), self.g_predictors.len());
                let mut snapshot = self.gnss_history.history.state();
                for (in_value, predictor) in buf.into_iter().zip(self.g_predictors.iter_mut()) {
                    predictor.predict(
                        in_value,
                        &mut snapshot,
                        &self.ip_history.state(),
                        self.gnss_history.gnss_home,
                    );
                }
                self.gnss_history.history.advance();

                Some(LogRecord::GNSS(self.gnss_history.history.values()))
            }
            BodyFrame::SFrame(OwnedSFrame { buf }) => Some(LogRecord::Slow(buf)),
            BodyFrame::Event(frame) => Some(LogRecord::Event(frame)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AnyIPredictor {
    AddConstant(AddConstantPredictor),
    AddField(AddFieldPredictor),
}

impl AnyIPredictor {
    pub fn new(
        predictor: FieldPredictor,
        settings: &HashMap<String, String>,
        ip_fields: &HashMap<String, IPField>,
        field_ix: usize,
    ) -> Self {
        match predictor {
            FieldPredictor::None => {
                AnyIPredictor::AddConstant(AddConstantPredictor { base: 0, field_ix })
            }
            FieldPredictor::Around1500 => AnyIPredictor::AddConstant(AddConstantPredictor {
                base: 1500,
                field_ix,
            }),
            FieldPredictor::MinThrottle => AnyIPredictor::AddConstant(AddConstantPredictor {
                base: settings["minthrottle"].parse().unwrap(),
                field_ix,
            }),
            FieldPredictor::Motor0 => AnyIPredictor::AddField(AddFieldPredictor {
                base_field_ix: ip_fields["motor[0]"].ix,
                field_ix,
            }),
            FieldPredictor::MinMotor => AnyIPredictor::AddConstant(AddConstantPredictor {
                base: settings["motorOutput"]
                    .split(',')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap(),
                field_ix,
            }),
            FieldPredictor::VBatRef => AnyIPredictor::AddConstant(AddConstantPredictor {
                base: settings["vbatref"].parse().unwrap(),
                field_ix,
            }),
            //motorOutput
            p => unimplemented!("{:?}", p),
        }
    }
}

impl IPredictor for AnyIPredictor {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>) {
        match self {
            AnyIPredictor::AddConstant(p) => p.predict(value, snapshot),
            AnyIPredictor::AddField(p) => p.predict(value, snapshot),
        }
    }
}

pub(crate) trait IPredictor: Copy + Clone {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>);
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AddConstantPredictor {
    pub base: i64,
    pub field_ix: usize,
}

impl IPredictor for AddConstantPredictor {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>) {
        snapshot.current[self.field_ix] = (self.base + value) as i64;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AddFieldPredictor {
    pub base_field_ix: usize,
    field_ix: usize,
}

impl IPredictor for AddFieldPredictor {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>) {
        snapshot.current[self.field_ix] = snapshot.current[self.base_field_ix] + value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AnyPPredictor {
    None(NonePredictor),
    Previous(PreviousPredictor),
    Inc(IncPredictor),
    StraightLine(StraightLinePredictor),
    Average(AveragePredictor),
}

impl AnyPPredictor {
    pub fn new(predictor: FieldPredictor, p_interval: Ratio<u16>, field_ix: usize) -> Self {
        match predictor {
            FieldPredictor::None => AnyPPredictor::None(NonePredictor { field_ix }),
            FieldPredictor::Previous => AnyPPredictor::Previous(PreviousPredictor { field_ix }),
            FieldPredictor::Increment => {
                AnyPPredictor::Inc(IncPredictor::new(field_ix, p_interval))
            }
            FieldPredictor::StraightLine => {
                AnyPPredictor::StraightLine(StraightLinePredictor { field_ix })
            }
            FieldPredictor::Average2 => AnyPPredictor::Average(AveragePredictor { field_ix }),
            _ => unimplemented!("Predictor {:?}", predictor),
        }
    }

    pub fn none(field_ix: usize) -> Self {
        AnyPPredictor::None(NonePredictor { field_ix })
    }
}

impl PPredictor for AnyPPredictor {
    fn predict(&mut self, value: i64, snapshot: &mut Snapshot<'_>) {
        match self {
            AnyPPredictor::None(p) => p.predict(value, snapshot),
            AnyPPredictor::Previous(p) => p.predict(value, snapshot),
            AnyPPredictor::Inc(p) => p.predict(value, snapshot),
            AnyPPredictor::StraightLine(p) => p.predict(value, snapshot),
            AnyPPredictor::Average(p) => p.predict(value, snapshot),
        }
    }
}

pub(crate) trait PPredictor: Copy + Clone {
    fn predict(&mut self, value: i64, snapshot: &mut Snapshot<'_>);
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NonePredictor {
    field_ix: usize,
}

impl PPredictor for NonePredictor {
    fn predict(&mut self, value: i64, snapshot: &mut Snapshot<'_>) {
        snapshot.current[self.field_ix] = value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviousPredictor {
    field_ix: usize,
}

impl PPredictor for PreviousPredictor {
    fn predict(&mut self, value: i64, snapshot: &mut Snapshot<'_>) {
        snapshot.current[self.field_ix] = snapshot.previous[self.field_ix] + value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct IncPredictor {
    field_ix: usize,
    increment: Ratio<u16>,
    running_sum: Ratio<u16>,
    base: i64,
    expected_value: i64,
}

impl IncPredictor {
    pub fn new(field_ix: usize, p_interval: Ratio<u16>) -> Self {
        let increment = p_interval.recip();
        Self {
            field_ix,
            running_sum: Ratio::new(0, *increment.denom()),
            increment,
            base: 0,
            expected_value: 0,
        }
    }
}

impl PPredictor for IncPredictor {
    fn predict(&mut self, _: i64, snapshot: &mut Snapshot<'_>) {
        if snapshot.current[self.field_ix] != self.expected_value {
            self.base = snapshot.current[self.field_ix];
            self.running_sum = Ratio::new(0, *self.increment.denom());
        }

        self.running_sum += self.increment;

        let current_value = self.base + (self.running_sum.to_integer() as i64);
        snapshot.current[self.field_ix] = current_value;
        self.expected_value = current_value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StraightLinePredictor {
    field_ix: usize,
}

impl PPredictor for StraightLinePredictor {
    fn predict(&mut self, value: i64, snapshot: &mut Snapshot<'_>) {
        // without overflow
        let next = snapshot.previous[self.field_ix] - snapshot.previous_2[self.field_ix]
            + snapshot.previous[self.field_ix];
        snapshot.current[self.field_ix] = next + value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AveragePredictor {
    field_ix: usize,
}

impl PPredictor for AveragePredictor {
    fn predict(&mut self, value: i64, snapshot: &mut Snapshot<'_>) {
        let p2 = snapshot.previous_2[self.field_ix];
        let p1 = snapshot.previous[self.field_ix];
        let avg = (p1 + p2) / 2;
        snapshot.current[self.field_ix] = avg + value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AnyGPredictor {
    None(NonePredictor),
    HomeCoordinates(HomeCoordinatesPredictor),
    LastMainFrameTime(LastMainFrameTimePredictor),
}

impl AnyGPredictor {
    pub fn new(
        predictor: FieldPredictor,
        field_ix: usize,
        index: usize,
        ip_fields: &HashMap<String, IPField>,
    ) -> Self {
        match predictor {
            FieldPredictor::None => AnyGPredictor::None(NonePredictor { field_ix }),
            FieldPredictor::HomeCoordinates => {
                AnyGPredictor::HomeCoordinates(HomeCoordinatesPredictor {
                    field_ix,
                    gnss_home_ix: index,
                })
            }
            FieldPredictor::LastMainFrameTime => {
                AnyGPredictor::LastMainFrameTime(LastMainFrameTimePredictor {
                    field_ix,
                    time_ix: ip_fields["time"].ix,
                })
            }
            _ => unimplemented!("Predictor {:?}", predictor),
        }
    }
}

impl GPredictor for AnyGPredictor {
    fn predict(
        &mut self,
        value: i64,
        snapshot: &mut Snapshot<'_>,
        ip_snapshot: &Snapshot<'_>,
        gnss_home: [i64; 2],
    ) {
        match self {
            AnyGPredictor::None(p) => p.predict(value, snapshot),
            AnyGPredictor::HomeCoordinates(p) => p.predict(value, snapshot, ip_snapshot, gnss_home),
            AnyGPredictor::LastMainFrameTime(p) => {
                p.predict(value, snapshot, ip_snapshot, gnss_home)
            }
        }
    }
}

pub(crate) trait GPredictor: Copy + Clone {
    fn predict(
        &mut self,
        value: i64,
        snapshot: &mut Snapshot<'_>,
        ip_snapshot: &Snapshot<'_>,
        gnss_home: [i64; 2],
    );
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HomeCoordinatesPredictor {
    field_ix: usize,
    gnss_home_ix: usize,
}

impl GPredictor for HomeCoordinatesPredictor {
    fn predict(
        &mut self,
        value: i64,
        snapshot: &mut Snapshot<'_>,
        _ip_snapshot: &Snapshot<'_>,
        gnss_home: [i64; 2],
    ) {
        snapshot.current[self.field_ix] = gnss_home[self.gnss_home_ix] + value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LastMainFrameTimePredictor {
    field_ix: usize,
    time_ix: usize,
}

impl GPredictor for LastMainFrameTimePredictor {
    fn predict(
        &mut self,
        value: i64,
        snapshot: &mut Snapshot<'_>,
        ip_snapshot: &Snapshot<'_>,
        _gnss_home: [i64; 2],
    ) {
        snapshot.current[self.field_ix] = ip_snapshot.current[self.time_ix] + value;
    }
}
