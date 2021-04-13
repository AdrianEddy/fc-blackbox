use chrono::{DateTime, Utc};
use nom::{IResult, bytes::streaming::{tag, take_until}, combinator::{map, map_res}};

use crate::FieldPredictor;

use super::{RawFieldEncoding, parse_dec_as_bool_list, parse_dec_as_encoding_list, parse_dec_as_predictor_list, parse_i16_dec, parse_str, parse_str_list, parse_u16_dec};

#[derive(Debug)]
pub(crate) enum Frame<'f> {
    Product(&'f str),
    DataVersion(&'f str),
    FieldIName(Vec<&'f str>),
    FieldISignedness(Vec<bool>),
    FieldIEncoding(Vec<RawFieldEncoding>),
    FieldIPredictor(Vec<FieldPredictor>),
    FieldPName(Vec<&'f str>),
    FieldPSignedness(Vec<bool>),
    FieldPEncoding(Vec<RawFieldEncoding>),
    FieldPPredictor(Vec<FieldPredictor>),
    FieldSName(Vec<&'f str>),
    FieldSSignedness(Vec<bool>),
    FieldSEncoding(Vec<RawFieldEncoding>),
    FieldSPredictor(Vec<FieldPredictor>),
    FirmwareType(&'f str),
    FirmwareRevision(&'f str, &'f str, &'f str, &'f str),
    FirmwareDate(DateTime<Utc>),
    BoardInformation(BoardInformation<'f>),
    LogStart(DateTime<Utc>),
    CraftName(&'f str),
    IInterval(i16),
    PInterval(i16),
    PRatio(u16),
    MinThrottle(u16),
    MaxThrottle(u16),
    GyroScale(f32),
    MotorOutput(u16, u16),
    Acc1G(u16),
    VBatScale(u8),
    VBatCellVoltage(VBatCellVoltage),
    VBatRef(u16),
    CurrentSensor(CurrentSensor),
    LoopTime(u32),
    GyroSyncDenom(u8),
    PidProcessDenom(u8),
    ThrottleMid(u8),
    ThrottleExpo(u8),
    TPARate(u8),
    TPABreakpoint(u16),
    RCRates(RollPitchYaw<u8>),
    RCExpo(RollPitchYaw<u8>),
    Rates(RollPitchYaw<u8>),
    RateLimits(RollPitchYaw<u16>),
    RollPID(PID<f32>),
    PitchPID(PID<f32>),
    YawPID(PID<f32>),
    LevelPID(PID<f32>),
    MagP(f32),
    DMin(RollPitchYaw<u8>),
    DMinGain(u8),
    DMinAdvance(u8),
    DTermFilterType(u8),
    DTermLowpassHz(u16),
    DTermLowpassDynHz(u16, u16),

    UnkownHeader(&'f str, &'f str),
}

#[derive(Debug)]
pub struct BoardInformation<'f> {
    manufacturer_id: &'f str,
    board_name: &'f str,
}

#[derive(Debug)]
pub struct VBatCellVoltage {
    min: u16,
    warning: u16,
    max: u16,
}

#[derive(Debug)]
pub struct CurrentSensor {
    offset: u16,
    scale: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct RollPitchYaw<T: Clone + Copy> {
    roll: T,
    pitch: T,
    yaw: T,
}

#[derive(Clone, Copy, Debug)]
pub struct PID<T: Clone + Copy> {
    p: T,
    i: T,
    d: T,
}

pub(crate) fn parse_header(input: &[u8]) -> IResult<&[u8], Frame> {
    let (input, _) = tag("H ")(input)?;
    let (input, name) = map_res(take_until(":"), super::str_from_bytes)(input)?;
    let (input, _) = tag(":")(input)?;
    // let value = ;

    let (input, header_frame) = match name {
        "Product" => map(parse_str, Frame::Product)(input),
        "Data version" => map(parse_str, Frame::DataVersion)(input),
        "I interval" => map(parse_i16_dec, Frame::IInterval)(input),
        "P interval" => map(parse_i16_dec, Frame::PInterval)(input),
        "P ratio" => map(parse_u16_dec, Frame::PRatio,)(input),
        "Field I name" => map(parse_str_list,Frame::FieldIName)(input),
        "Field I signed" => map(parse_dec_as_bool_list, Frame::FieldISignedness)(input),
        "Field I encoding" => map(parse_dec_as_encoding_list, Frame::FieldIEncoding)(input),
        "Field I predictor" => map(parse_dec_as_predictor_list, Frame::FieldIPredictor)(input),
        "Field P name" => map(parse_str_list,Frame::FieldPName)(input),
        "Field P signed" => map(parse_dec_as_bool_list, Frame::FieldPSignedness)(input),
        "Field P encoding" => map(parse_dec_as_encoding_list, Frame::FieldPEncoding)(input),
        "Field P predictor" => map(parse_dec_as_predictor_list, Frame::FieldPPredictor)(input),
        "Field S name" => map(parse_str_list,Frame::FieldSName)(input),
        "Field S signed" => map(parse_dec_as_bool_list, Frame::FieldSSignedness)(input),
        "Field S encoding" => map(parse_dec_as_encoding_list, Frame::FieldSEncoding)(input),
        "Field S predictor" => map(parse_dec_as_predictor_list, Frame::FieldSPredictor)(input),
        name => map(parse_str, |v| {
            Frame::UnkownHeader(name, v)
        })(input),
    }?;

    let (input, _) = tag("\n")(input)?;
    Ok((input, header_frame))
}
