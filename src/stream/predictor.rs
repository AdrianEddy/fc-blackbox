use std::collections::HashMap;

use crate::{FieldPredictor, frame::{BodyFrame, data::{OwnedIFrame, OwnedPFrame}}};

use super::header::{Header, IPField};


pub(crate) struct History {
    history: [Vec<i64>; 2],
    current: Vec<i64>,
    previous_2_ix: usize,
    previous_ix: usize,
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

    pub fn values<'a>(&self) -> &[i64] {
        &self.history[self.previous_ix]
    }

    pub fn state<'a>(&'a mut self) -> Snapshot<'a> {
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

pub struct LogProcessor {
    history: History,
    i_predictors: Vec<AnyIPredictor>,
    p_predictors: Vec<AnyPPredictor>,
}

impl LogProcessor {
    pub fn new(header: &Header) -> Self {
        let i_predictors = header.i_field_predictors.clone();
        let p_predictors = header.p_field_predictors.clone();

        assert_eq!(i_predictors.len(), p_predictors.len());

        Self {
            history: History::with_size(i_predictors.len()),
            i_predictors,
            p_predictors,
        }
    }

    pub fn process_frame(&mut self, frame: BodyFrame) -> Option<&[i64]> {
        match frame {
            BodyFrame::IFrame(OwnedIFrame {buf}) => {
                assert_eq!(buf.len(), self.i_predictors.len());
                let mut snapshot = self.history.state();
                for (in_value, predictor) in buf.into_iter().zip(self.i_predictors.iter()) {
                    predictor.predict(in_value, &mut snapshot);
                }
                self.history.advance_reset();
                Some(self.history.values())
            }
            BodyFrame::PFrame(OwnedPFrame {buf}) => {
                assert_eq!(buf.len(), self.p_predictors.len());
                let mut snapshot = self.history.state();
                for (in_value, predictor) in buf.into_iter().zip(self.p_predictors.iter()) {
                    predictor.predict(in_value, &mut snapshot);
                }
                self.history.advance();
                Some(self.history.values())
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AnyIPredictor {
    AddConstant(AddConstantPredictor),
    AddField(AddFieldPredictor),
}

impl AnyIPredictor {
    pub fn new(predictor: FieldPredictor, settings: &HashMap<String, String>, ip_fields: &HashMap<String, IPField>, field_ix: usize) -> Self {
        match predictor {
            FieldPredictor::None => AnyIPredictor::AddConstant(AddConstantPredictor { base: 0, field_ix }),
            FieldPredictor::Around1500 => AnyIPredictor::AddConstant(AddConstantPredictor { base: 1500, field_ix }),
            FieldPredictor::MinThrottle => AnyIPredictor::AddConstant(AddConstantPredictor { base: settings["minthrottle"].parse().unwrap(), field_ix }),
            FieldPredictor::Motor0 => AnyIPredictor::AddField(AddFieldPredictor { base_field_ix: ip_fields["motor[0]"].ix, field_ix }),
            FieldPredictor::MinMotor => AnyIPredictor::AddConstant(AddConstantPredictor { base: settings["motorOutput"].split(',').next().unwrap().parse().unwrap() , field_ix }),
            FieldPredictor::VBatRef => AnyIPredictor::AddConstant(AddConstantPredictor { base: settings["vbatref"].parse().unwrap(), field_ix }),
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
        snapshot.current[self.field_ix] = self.base + value;
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
    Previous(PreviousPredictor),
    Inc(IncPredictor),
    StraightLine(StraightLinePredictor),
    Average(AveragePredictor),
}

impl AnyPPredictor {
    pub fn new(predictor: FieldPredictor, field_ix: usize) -> Self {
        match predictor {
            FieldPredictor::Previous => AnyPPredictor::Previous(PreviousPredictor { field_ix }),
            FieldPredictor::Increment => AnyPPredictor::Inc(IncPredictor { field_ix }),
            FieldPredictor::StraightLine => AnyPPredictor::StraightLine(StraightLinePredictor { field_ix }),
            FieldPredictor::Average2 => AnyPPredictor::Average(AveragePredictor { field_ix }),
            _ => unimplemented!(),
        }
    }
}

impl PPredictor for AnyPPredictor {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>) {
        match self {
            AnyPPredictor::Previous(p) => p.predict(value, snapshot),
            AnyPPredictor::Inc(p) => p.predict(value, snapshot),
            AnyPPredictor::StraightLine(p) => p.predict(value, snapshot),
            AnyPPredictor::Average(p) => p.predict(value, snapshot),
        }
    }
}

pub(crate) trait PPredictor: Copy + Clone {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>);
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviousPredictor {
    field_ix: usize,
}

impl PPredictor for PreviousPredictor {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>) {
        snapshot.current[self.field_ix] = snapshot.previous[self.field_ix] + value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct IncPredictor {
    field_ix: usize,
}

impl PPredictor for IncPredictor {
    fn predict(&self, _: i64, snapshot: &mut Snapshot<'_>) {
        snapshot.current[self.field_ix] = snapshot.previous[self.field_ix] + 1;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StraightLinePredictor {
    field_ix: usize,
}

impl PPredictor for StraightLinePredictor {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>) {
        // without overflow
        let next = snapshot.previous[self.field_ix] - snapshot.previous_2[self.field_ix] + snapshot.previous[self.field_ix];
        snapshot.current[self.field_ix] = next + value;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AveragePredictor {
    field_ix: usize,
}

impl PPredictor for AveragePredictor {
    fn predict(&self, value: i64, snapshot: &mut Snapshot<'_>) {
        let p2 = snapshot.previous_2[self.field_ix];
        let p1 = snapshot.previous[self.field_ix];
        // compute average without overflowing i64
        let avg = (p1 / 2) + (p2 / 2) + ((p1 % 2 + p2 % 2) / 2);
        snapshot.current[self.field_ix] = avg + value;
    }
}

