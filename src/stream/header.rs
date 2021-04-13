use std::{collections::HashMap, convert::{TryFrom, TryInto}};

use itertools::izip;
use nom::{IResult, error::{ErrorKind, make_error}, multi::fold_many0};

use crate::{FieldPredictor, frame::{FieldEncoding, RawFieldEncoding, header::{Frame, parse_header}}};

use super::predictor::{AnyIPredictor, AnyPPredictor};

#[derive(Debug)]
pub struct Header {
    product: String,
    data_version: String,
    firmware_type: Option<String>,
    firmware_revision: Option<String>,
    firmware_date: Option<String>,
    board_information: Option<String>,
    log_start_datetime: Option<String>,
    craft_name: Option<String>,
    i_interval: i16,
    p_interval: i16,
    p_ratio: u16,

    other_headers: HashMap<String, String>,

    ip_fields: HashMap<String, IPField>,
    s_fields: HashMap<String, SlowField>,

    pub(crate) ip_fields_in_order: Vec<IPField>,

    pub(crate) i_field_encodings: Vec<FieldEncoding>,
    pub(crate) i_field_predictors: Vec<AnyIPredictor>,
    pub(crate) p_field_encodings: Vec<FieldEncoding>,
    pub(crate) p_field_predictors: Vec<AnyPPredictor>,
    pub(crate) s_field_encodings: Vec<FieldEncoding>,
}

impl TryFrom<HeaderBuilder> for Header {
    type Error = ();

    fn try_from(builder: HeaderBuilder) -> Result<Self, Self::Error> {
        let product = builder.product.ok_or(())?;
        let data_version = builder.data_version.ok_or(())?;
        let i_interval = builder.i_interval.ok_or(())?;
        let p_interval = builder.p_interval.ok_or(())?;
        let p_ratio = builder.p_ratio.ok_or(())?;

        let mut ip_fields = HashMap::with_capacity(builder.i_field_names.len());
        let mut ip_fields_in_order = Vec::with_capacity(builder.i_field_names.len());
        let mut i_field_encodings = Vec::with_capacity(builder.i_field_names.len());
        let mut p_field_encodings = Vec::with_capacity(builder.i_field_names.len());
        let mut i_field_predictors = Vec::with_capacity(builder.i_field_names.len());
        let mut p_field_predictors = Vec::with_capacity(builder.i_field_names.len());
        
        fn add_encoding(encodings: &mut Vec<FieldEncoding>, new_encoding: RawFieldEncoding) {
            let new_encoding = match new_encoding {
                RawFieldEncoding::Tag8_8SVB => {
                    if let Some(FieldEncoding::Tag8_8SVB(n_fields)) = encodings.last_mut() {
                        if *n_fields != 8 {
                            *n_fields += 1;
                            return;
                        }
                    }
                    FieldEncoding::Tag8_8SVB(1)
                },
                RawFieldEncoding::Tag2_3S32 => {
                    if let Some(FieldEncoding::Tag2_3S32(n_fields)) = encodings.last_mut() {
                        if *n_fields != 3 {
                            *n_fields += 1;
                            return;
                        }
                    }
                    FieldEncoding::Tag2_3S32(1)
                },
                RawFieldEncoding::Tag2_3SVariable => {
                    if let Some(FieldEncoding::Tag2_3SVariable(n_fields)) = encodings.last_mut() {
                        if *n_fields != 3 {
                            *n_fields += 1;
                            return;
                        }
                    }
                    FieldEncoding::Tag2_3SVariable(1)
                },
                RawFieldEncoding::Tag8_4S16 => {
                    if let Some(FieldEncoding::Tag8_4S16(n_fields)) = encodings.last_mut() {
                        if *n_fields != 4 {
                            *n_fields += 1;
                            return;
                        }
                    }
                    FieldEncoding::Tag8_4S16(1)
                },
                RawFieldEncoding::Null => FieldEncoding::Null,
                RawFieldEncoding::Negative14BitVB => FieldEncoding::Negative14BitVB,
                RawFieldEncoding::SignedVB => FieldEncoding::SignedVB,
                RawFieldEncoding::UnsignedVB => FieldEncoding::UnsignedVB,
            };
            encodings.push(new_encoding);
        }

        for (ix, (name, signedness, i_encoding, p_encoding)) in izip!(builder.i_field_names, builder.i_field_signedness, builder.i_field_encoding, builder.p_field_encoding).enumerate() {
            add_encoding(&mut i_field_encodings, i_encoding);
            add_encoding(&mut p_field_encodings, p_encoding);

            let field = IPField {
                name: name.clone(),
                ix,
                signed: signedness,
            };
            ip_fields.insert(name, field.clone());
            ip_fields_in_order.push(field);
        }

        for (ix, i_predictor) in builder.i_field_predictors.iter().copied().enumerate() {
            i_field_predictors.push(AnyIPredictor::new(i_predictor, &builder.other_headers, &ip_fields, ix));
        }

        for (ix, p_predictor) in builder.p_field_predictors.iter().copied().enumerate() {
            p_field_predictors.push(AnyPPredictor::new(p_predictor, ix));
        }

        let mut s_fields = HashMap::with_capacity(builder.s_field_names.len());
        let mut s_field_encodings = Vec::with_capacity(builder.s_field_names.len());
        for (ix, (name, signedness, encoding, predictor)) in izip!(builder.s_field_names, builder.s_field_signedness, builder.s_field_encoding, builder.s_field_predictors).enumerate() {
            add_encoding(&mut s_field_encodings, encoding);
            s_fields.insert(name.clone(), SlowField {
                name,
                ix: ix as i8,
                predictor: predictor,
                signed: signedness,
            });
        }


        Ok(Header {
            product,
            data_version,
            firmware_type: builder.firmware_type,
            firmware_revision: builder.firmware_revision,
            firmware_date: builder.firmware_date,
            board_information: builder.board_information,
            log_start_datetime: builder.log_start_datetime,
            craft_name: builder.craft_name,
            i_interval,
            p_interval,
            p_ratio,
            other_headers: builder.other_headers,
            ip_fields,
            s_fields,
            ip_fields_in_order,
            i_field_encodings,
            i_field_predictors,
            p_field_encodings,
            p_field_predictors,
            s_field_encodings,
        })
    }
}

#[derive(Clone, Debug, Default)]
struct HeaderBuilder {
    product: Option<String>,
    data_version: Option<String>,
    firmware_type: Option<String>,
    firmware_revision: Option<String>,
    firmware_date: Option<String>,
    board_information: Option<String>,
    log_start_datetime: Option<String>,
    craft_name: Option<String>,
    i_interval: Option<i16>,
    p_interval: Option<i16>,
    p_ratio: Option<u16>,

    other_headers: HashMap<String, String>,

    i_field_names: Vec<String>,
    i_field_signedness: Vec<bool>,
    i_field_encoding: Vec<RawFieldEncoding>,
    i_field_predictors: Vec<FieldPredictor>,
    p_field_encoding: Vec<RawFieldEncoding>,
    p_field_predictors: Vec<FieldPredictor>,

    s_field_names: Vec<String>,
    s_field_signedness: Vec<bool>,
    s_field_encoding: Vec<RawFieldEncoding>,
    s_field_predictors: Vec<FieldPredictor>,
}

#[derive(Clone, Debug)]
pub(crate) struct IPField {
    pub name: String,
    pub ix: usize,
    pub signed: bool,
}

#[derive(Clone, Debug)]
struct SlowField {
    name: String,
    ix: i8,
    signed: bool,
    predictor: FieldPredictor,
}

pub fn parse_headers(input: &[u8]) -> IResult<&[u8], Header> {
    let (input, header) = fold_many0(
        parse_header,
        HeaderBuilder::default(),
        |mut header, header_frame| {
            match header_frame {
                Frame::Product(product) => header.product = Some(product.to_owned()),
                Frame::DataVersion(version) => {
                    header.data_version = Some(version.to_owned())
                }
                Frame::IInterval(i_interval) => header.i_interval = Some(i_interval),
                Frame::FieldIName(i_field_names) => header.i_field_names = i_field_names.into_iter().map(ToOwned::to_owned).collect(),
                Frame::FieldIPredictor(i_field_predictors) => header.i_field_predictors = i_field_predictors,
                Frame::FieldISignedness(i_field_signedness) => header.i_field_signedness = i_field_signedness,
                Frame::FieldIEncoding(i_field_encoding) => header.i_field_encoding = i_field_encoding,
                Frame::PInterval(p_interval) => header.p_interval = Some(p_interval),
                Frame::PRatio(p_ratio) => header.p_ratio = Some(p_ratio),
                Frame::FieldPPredictor(p_field_predictors) => header.p_field_predictors = p_field_predictors,
                Frame::FieldPEncoding(p_field_encoding) => header.p_field_encoding = p_field_encoding,
                Frame::FieldSName(s_field_names) => header.s_field_names = s_field_names.into_iter().map(ToOwned::to_owned).collect(),
                Frame::FieldSPredictor(s_field_predictors) => header.s_field_predictors = s_field_predictors,
                Frame::FieldSSignedness(s_field_signedness) => header.s_field_signedness = s_field_signedness,
                Frame::FieldSEncoding(s_field_encoding) => header.s_field_encoding = s_field_encoding,
                Frame::UnkownHeader(name, value) => {
                    header.other_headers.insert(name.into(), value.into());
                }
                _ => {}
            };
            header
        },
    )(input)?;

    let header = header
        .try_into()
        .map_err(|_| nom::Err::Failure(make_error(input, ErrorKind::Complete)))?;
    Ok((input, header))
}
