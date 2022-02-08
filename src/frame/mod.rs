use std::mem::size_of_val;

use nom::{
    branch::alt,
    bytes::streaming::{is_not, tag, take, take_until},
    combinator::{map, map_res},
    error::{Error, ErrorKind, ParseError},
    multi::separated_list0,
    number::{
        complete::be_u8,
        streaming::{le_i16, le_i24, le_i32, le_i8, le_u8},
    },
    IResult,
};
use num_rational::Ratio;
use num_traits::{WrappingShl, WrappingShr};

use crate::stream::predictor::FieldPredictor;

pub(crate) mod data;
pub mod event;
pub(crate) mod header;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RawFieldEncoding {
    SignedVB,
    UnsignedVB,
    Negative14BitVB,
    Tag8_8SVB,
    Tag2_3S32,
    Tag8_4S16,
    Null,
    Tag2_3SVariable,
}

impl Default for RawFieldEncoding {
    fn default() -> Self {
        RawFieldEncoding::Null
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldEncoding {
    SignedVB,
    UnsignedVB,
    Negative14BitVB,
    Tag8_8SVB(usize),
    Tag2_3S32(usize),
    Tag8_4S16(usize),
    Null,
    Tag2_3SVariable(usize),
}

impl Default for FieldEncoding {
    fn default() -> Self {
        FieldEncoding::Null
    }
}

// enum Tag2_3S32_Tag1 {

// }

#[derive(Clone, Copy, Debug)]
pub(crate) enum Field {
    Unsigned(u32),
    Signed(i32),
    SignedTriple([i32; 3]),
    SignedQuadruple([i16; 4]),
    SignedOctuple([i32; 8], usize),
}

#[inline]
fn sign_extend<T: WrappingShl + WrappingShr>(x: T, nbits: u32) -> T {
    let notherbits = size_of_val(&x) as u32 * 8 - nbits;
    x.wrapping_shl(notherbits).wrapping_shr(notherbits)
}

fn sign_extend_14bit(word: u16) -> i32 {
    if (word & 0x2000) != 0 {
        (word | 0xC000) as i16 as i32
    } else {
        word as i32
    }
}

impl FieldEncoding {
    pub(crate) fn parse<'a>(&self, input: &'a [u8]) -> IResult<&'a [u8], Field> {
        Ok(match self {
            FieldEncoding::Null => (input, Field::Unsigned(0)),
            FieldEncoding::UnsignedVB => {
                let (input, varint) = take_varint(input)?;
                (input, Field::Unsigned(varint))
            }
            FieldEncoding::SignedVB => {
                let (input, varint) = take_varint(input)?;
                (input, Field::Signed(zigzag_decode(varint)))
            }
            FieldEncoding::Negative14BitVB => {
                let (input, varint) = take_varint(input)?;
                // -signExtend14Bit(streamReadUnsignedVB(stream));
                (input, Field::Signed(-(sign_extend_14bit(varint as u16))))
            }
            FieldEncoding::Tag2_3S32(_) => {
                let (input, byte1) = be_u8(input)?;

                match byte1 >> 6 {
                    0b00 => (
                        input,
                        Field::SignedTriple([
                            sign_extend((byte1 >> 4) as i32, 2),
                            sign_extend((byte1 >> 2) as i32, 2),
                            sign_extend(byte1 as i32, 2),
                        ]),
                    ),
                    0b01 => {
                        let (input, byte2) = be_u8(input)?;
                        (
                            input,
                            Field::SignedTriple([
                                sign_extend(byte1 as i32, 4),
                                sign_extend((byte2 >> 4) as i32, 4),
                                sign_extend(byte2 as i32, 4),
                            ]),
                        )
                    }
                    0b10 => {
                        let (input, byte2) = be_u8(input)?;
                        let (input, byte3) = be_u8(input)?;
                        (
                            input,
                            Field::SignedTriple([
                                sign_extend(byte1 as i32, 6),
                                sign_extend(byte2 as i32, 6),
                                sign_extend(byte3 as i32, 6),
                            ]),
                        )
                    }
                    0b11 => {
                        let selector1 = byte1 & 0b11;
                        let selector2 = (byte1 >> 2) & 0b11;
                        let selector3 = (byte1 >> 4) & 0b11;

                        fn read_value(selector: u8, input: &[u8]) -> IResult<&[u8], i32> {
                            match selector {
                                0b00 => map(le_i8, i32::from)(input),
                                0b01 => map(le_i16, i32::from)(input),
                                0b10 => le_i24(input),
                                0b11 => le_i32(input),
                                _ => unreachable!(),
                            }
                        }

                        let (input, value1) = read_value(selector1, input)?;
                        let (input, value2) = read_value(selector2, input)?;
                        let (input, value3) = read_value(selector3, input)?;

                        (input, Field::SignedTriple([value1, value2, value3]))
                    }
                    _ => {
                        unreachable!()
                    }
                }
            }
            FieldEncoding::Tag8_4S16(_) => {
                let (input, selectors) = be_u8(input)?;
                let selectors = [
                    selectors & 0b11,
                    (selectors >> 2) & 0b11,
                    (selectors >> 4) & 0b11,
                    (selectors >> 6) & 0b11,
                ];

                fn n_nibbles(selector: u8) -> u8 {
                    match selector {
                        0b00 => 0,
                        0b01 => 1,
                        0b10 => 2,
                        0b11 => 4,
                        _ => unreachable!(),
                    }
                }

                let mut nibbles = [0u8; 4];
                for i in 0..4 {
                    nibbles[i] = n_nibbles(selectors[i]);
                }
                let nibbles = nibbles;

                let total_nibbles: u8 = nibbles.iter().sum();
                let total_bytes = (total_nibbles + 1) / 2;

                let (input, bytes) = take(total_bytes)(input)?;
                let mut current_nibble = 0;

                fn read_value(current_nibble: u8, nibbles_to_read: u8, bytes: &[u8]) -> i16 {
                    let mut v = 0i16;
                    let mut read_pos_nibbles_msn = current_nibble;
                    let mut write_pos_bits_lsb = nibbles_to_read * 4;
                    loop {
                        if write_pos_bits_lsb == 0 {
                            break;
                        }

                        v <<= 4;
                        v |= ({
                            let b = bytes[(read_pos_nibbles_msn / 2) as usize];
                            if read_pos_nibbles_msn % 2 == 0 {
                                b >> 4
                            } else {
                                b
                            }
                        } & 0x0f) as i16;

                        read_pos_nibbles_msn += 1;
                        write_pos_bits_lsb -= 4;
                    }

                    sign_extend(v, (nibbles_to_read * 4).into())
                }

                let mut values = [0i16; 4];
                for i in 0..4 {
                    let nibbles_to_read = nibbles[i];
                    values[i] = read_value(current_nibble, nibbles_to_read, bytes);
                    current_nibble += nibbles_to_read;
                }

                (input, Field::SignedQuadruple(values))
            }
            FieldEncoding::Tag8_8SVB(fields_n) => {
                let mut values = [0i32; 8];

                if *fields_n == 1 {
                    let (input, varint) = take_varint(input)?;
                    values[0] = zigzag_decode(varint);

                    (input, Field::SignedOctuple(values, *fields_n))
                } else {
                    let (mut input, selectors) = be_u8(input)?;

                    for i in 0..*fields_n {
                        if selectors & (1 << i) != 0 {
                            let (remaining_input, varint) = take_varint(input)?;
                            input = remaining_input;
                            values[i] = zigzag_decode(varint);
                        }
                    }

                    (input, Field::SignedOctuple(values, *fields_n))
                }
            }
            e => unimplemented!("{:?}", e),
        })
    }
}

#[derive(Debug)]
pub(crate) enum BodyFrame {
    Event(event::Frame),
    IFrame(data::OwnedIFrame),
    PFrame(data::OwnedPFrame),
    SFrame(data::OwnedSFrame),
    GFrame(data::OwnedGFrame),
    HFrame(data::OwnedHFrame),
}

pub(crate) fn parse_body_frame(input: &[u8]) -> IResult<&[u8], BodyFrame> {
    let (input, event) = event::parse_event(input)?;
    Ok((input, BodyFrame::Event(event)))
}

fn i16_from_dec(bytes: &[u8]) -> Result<i16, ()> {
    Ok(i16::from_str_radix(std::str::from_utf8(bytes).map_err(|_| ())?, 10).map_err(|_| ())?)
}

fn u16_from_dec(bytes: &[u8]) -> Result<u16, ()> {
    Ok(u16::from_str_radix(std::str::from_utf8(bytes).map_err(|_| ())?, 10).map_err(|_| ())?)
}

fn u32_from_dec(bytes: &[u8]) -> Result<u32, ()> {
    Ok(u32::from_str_radix(std::str::from_utf8(bytes).map_err(|_| ())?, 10).map_err(|_| ())?)
}

fn u32_from_hex(bytes: &[u8]) -> Result<u32, ()> {
    Ok(u32::from_str_radix(std::str::from_utf8(bytes).map_err(|_| ())?, 16).map_err(|_| ())?)
}

fn str_from_bytes(bytes: &[u8]) -> Result<&str, ()> {
    std::str::from_utf8(bytes).map_err(|_| ())
}

fn bool_from_dec(bytes: &[u8]) -> Result<bool, ()> {
    u16_from_dec(bytes).map(|i| i != 0)
}

fn field_encoding_from_dec(bytes: &[u8]) -> Result<RawFieldEncoding, ()> {
    let i = u16_from_dec(bytes)?;
    Ok(match i {
        0 => RawFieldEncoding::SignedVB,
        1 => RawFieldEncoding::UnsignedVB,
        3 => RawFieldEncoding::Negative14BitVB,
        6 => RawFieldEncoding::Tag8_8SVB,
        7 => RawFieldEncoding::Tag2_3S32,
        8 => RawFieldEncoding::Tag8_4S16,
        9 => RawFieldEncoding::Null,
        10 => RawFieldEncoding::Tag2_3SVariable,
        _ => return Err(()),
    })
}

fn field_predictor_from_dec(bytes: &[u8]) -> Result<FieldPredictor, ()> {
    let i = u16_from_dec(bytes)?;
    Ok(match i {
        0 => FieldPredictor::None,
        1 => FieldPredictor::Previous,
        2 => FieldPredictor::StraightLine,
        3 => FieldPredictor::Average2,
        4 => FieldPredictor::MinThrottle,
        5 => FieldPredictor::Motor0,
        6 => FieldPredictor::Increment,
        7 => FieldPredictor::HomeCoordinates,
        8 => FieldPredictor::Around1500,
        9 => FieldPredictor::VBatRef,
        10 => FieldPredictor::LastMainFrameTime,
        11 => FieldPredictor::MinMotor,
        _ => return Err(()),
    })
}

fn parse_str(input: &[u8]) -> IResult<&[u8], &str> {
    map_res(take_until("\n"), str_from_bytes)(input)
}

fn parse_i16_dec(input: &[u8]) -> IResult<&[u8], i16> {
    map_res(take_until("\n"), i16_from_dec)(input)
}

fn parse_u16_ratio_dec(input: &[u8]) -> IResult<&[u8], Ratio<u16>> {
    let (input, numer) = map_res(take_until("/"), u16_from_dec)(input)?;
    let (input, _) = tag("/")(input)?;
    let (input, denom) = map_res(take_until("\n"), u16_from_dec)(input)?;
    Ok((input, Ratio::new(numer, denom)))
}

fn parse_u16_dec(input: &[u8]) -> IResult<&[u8], u16> {
    map_res(take_until("\n"), u16_from_dec)(input)
}

fn parse_u32_dec(input: &[u8]) -> IResult<&[u8], u32> {
    map_res(take_until("\n"), u32_from_dec)(input)
}

fn parse_u16_ratio_dec_or_inverse_dec(input: &[u8]) -> IResult<&[u8], Ratio<u16>> {
    alt((
        parse_u16_ratio_dec,
        map(parse_u16_dec, |denom| Ratio::new(1, denom)),
    ))(input)
}

fn parse_u32_hex(input: &[u8]) -> IResult<&[u8], u32> {
    let (input, _) = tag("0x")(input)?;
    map_res(take_until("\n"), u32_from_hex)(input)
}

fn parse_list<'a, F, T, E: ParseError<&'a [u8]>>(
    input: &'a [u8],
    parser: F,
) -> IResult<&'a [u8], Vec<T>>
where
    F: Fn(&'a [u8]) -> Result<T, E>,
{
    separated_list0(tag(","), map_res(is_not(",\n"), parser))(input)
}

fn parse_str_list(input: &[u8]) -> IResult<&[u8], Vec<&str>> {
    parse_list(input, str_from_bytes)
}

fn parse_dec_as_bool_list(input: &[u8]) -> IResult<&[u8], Vec<bool>> {
    parse_list(input, bool_from_dec)
}

fn parse_dec_as_encoding_list(input: &[u8]) -> IResult<&[u8], Vec<RawFieldEncoding>> {
    parse_list(input, field_encoding_from_dec)
}

fn parse_dec_as_predictor_list(input: &[u8]) -> IResult<&[u8], Vec<FieldPredictor>> {
    parse_list(input, field_predictor_from_dec)
}

fn take_varint(input: &[u8]) -> IResult<&[u8], u32> {
    let mut res: u32 = 0;
    let mut input = input;

    for position in 0..5 {
        let (remaining_input, byte) = le_u8(input)?;
        input = remaining_input;
        let value = byte & 0b0111_1111;
        res |= (value as u32) << (position * 7);
        if (byte & 0b1000_0000) == 0 {
            return Ok((input, res));
        }
    }
    Err(nom::Err::Failure(Error::from_error_kind(
        input,
        ErrorKind::TooLarge,
    )))
}

#[inline]
fn zigzag_decode(from: u32) -> i32 {
    ((from >> 1) ^ (-((from & 1) as i32)) as u32) as i32
}
