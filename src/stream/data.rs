use std::collections::HashMap;

use nom::{IResult, combinator::map};
use nom::branch::alt;

use crate::frame::{BodyFrame, data::{parse_owned_iframe, parse_owned_pframe, parse_owned_sframe}, parse_body_frame};

use super::header::Header;

pub(crate) fn parse_next_frame<'a: 'i, 'i>(header: &'a Header, input: &'i [u8]) -> IResult<&'i [u8], BodyFrame> {
    let (input, event) = alt((
        parse_body_frame,
        map(parse_owned_iframe(&header.i_field_encodings), BodyFrame::IFrame),
        map(parse_owned_pframe(&header.p_field_encodings), BodyFrame::PFrame),
        map(parse_owned_sframe(&header.s_field_encodings), BodyFrame::SFrame),
    ))(input)?;
    Ok((input, event))
}

fn parse_data_into_ndarray(_fields: HashMap<String, Vec<i64>>) {

}
