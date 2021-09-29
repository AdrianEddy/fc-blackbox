use nom::error::{ParseError, ErrorKind};
use nom::{combinator::map, IResult};

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
        b'I' => map(parse_owned_iframe(&header.i_field_encodings), BodyFrame::IFrame)(input),
        b'P' => map(parse_owned_pframe(&header.p_field_encodings), BodyFrame::PFrame)(input),
        b'S' => map(parse_owned_sframe(&header.s_field_encodings), BodyFrame::SFrame)(input),
        b'G' => map(parse_owned_gframe(&header.g_field_encodings), BodyFrame::GFrame)(input),
        b'H' => map(parse_owned_hframe(&header.h_field_encodings), BodyFrame::HFrame)(input),
        b'E' => parse_body_frame(input),
        0xff => Err(nom::Err::Error(ParseError::from_error_kind(input, ErrorKind::Eof))), // 0xff is padding
        _ => Err(nom::Err::Error(ParseError::from_error_kind(input, ErrorKind::Verify)))
    }
}
