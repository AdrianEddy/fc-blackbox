use nom::error::{ ParseError, ErrorKind };
use nom::IResult;

use crate::frame::{
    data::{
        parse_owned_gframe, parse_owned_hframe, parse_owned_iframe, parse_owned_pframe,
        parse_owned_sframe,
    },
    parse_body_frame, BodyFrame,
};

use super::header::Header;

pub(crate) fn parse_next_frame<'h, 'i: 'o, 'o>(
    header: &'h Header,
    input: &'i [u8],
) -> IResult<&'o [u8], BodyFrame> {
    match input[0] {
        b'I' => parse_owned_iframe(&header.i_field_encodings)(input).map(|v| (v.0, BodyFrame::IFrame(v.1))),
        b'P' => parse_owned_pframe(&header.p_field_encodings)(input).map(|v| (v.0, BodyFrame::PFrame(v.1))),
        b'S' => parse_owned_sframe(&header.s_field_encodings)(input).map(|v| (v.0, BodyFrame::SFrame(v.1))),
        b'G' => parse_owned_gframe(&header.g_field_encodings)(input).map(|v| (v.0, BodyFrame::GFrame(v.1))),
        b'H' => parse_owned_hframe(&header.h_field_encodings)(input).map(|v| (v.0, BodyFrame::HFrame(v.1))),
        b'E' => parse_body_frame(input),
        0xff => Err(nom::Err::Error(ParseError::from_error_kind(input, ErrorKind::Eof))), // 0xff is padding
        _ => { panic!("Unknown frame {:02x}", input[0]); }
    }
}
