use nom::branch::alt;
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
    let (input, event) = alt((
        parse_body_frame,
        map(
            parse_owned_iframe(&header.i_field_encodings),
            BodyFrame::IFrame,
        ),
        map(
            parse_owned_pframe(&header.p_field_encodings),
            BodyFrame::PFrame,
        ),
        map(
            parse_owned_sframe(&header.s_field_encodings),
            BodyFrame::SFrame,
        ),
        map(
            parse_owned_gframe(&header.g_field_encodings),
            BodyFrame::GFrame,
        ),
        map(
            parse_owned_hframe(&header.h_field_encodings),
            BodyFrame::HFrame,
        ),
    ))(input)?;
    Ok((input, event))
}
