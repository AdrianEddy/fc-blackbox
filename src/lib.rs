extern crate itertools;

pub(crate) mod frame;
pub mod stream;

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

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use crate::stream::{data::parse_next_frame, header::parse_headers};
    use std::{fs::File, io::Read};
    use crate::stream::predictor::LogProcessor;
    use std::io::Write;

    use ndarray::prelude::*;
    use ndarray::Array;

    #[test]
    fn it_works() -> Result<(), anyhow::Error> {
        let mut bytes = Vec::new();

        File::open("/home/hajile/Dropbox/Betaflight/Blackbox/btfl_059.bbl")?
            .read_to_end(&mut bytes)?;
        let total_size = bytes.len();

        let (bytes, header) = parse_headers(&bytes[..]).map_err(|e| match e {
            nom::Err::Error(e) => {
                anyhow::anyhow!("Error({:?})", e.code)
            }
            nom::Err::Failure(e) => {
                anyhow::anyhow!("Failure({:?})", e.code)
            }
            nom::Err::Incomplete(needed) => {
                anyhow::anyhow!("Incomplete({:?})", needed)
            }
        })?;

        let _parsed_size = total_size - bytes.len();

        // println!("Bytes parsed: {}", parsed_size);
        // println!("Next bytes: {:#?}", &bytes[0..3]);

        println!("Headers: {:#?}", &header);

        let mut bytes = bytes;

        let mut processor = LogProcessor::new(&header);

        let mut out = File::create("/home/hajile/Dropbox/Betaflight/Blackbox/btfl_059.csv")?;

        writeln!(out, "{}", header.ip_fields_in_order.iter().map(|f| &f.name).join(","))?;


        let fields_n = header.ip_fields_in_order.len();
        let mut data = vec![Vec::new(); fields_n];

        loop {
            match parse_next_frame(&header, bytes) {
                Ok((remaining_bytes, frame)) => {
                    // println!("Frame: {:?}", frame);
                    if let Some(values) = processor.process_frame(frame) {
                        assert_eq!(values.len(), data.len());
                        for (dst, src) in data.iter_mut().zip(values) {
                            dst.push(*src);
                        }

                        // writeln!(out, "{}", values.iter().map(|v| v.to_string()).join(","))?;
                        // println!("Values: {:?}", values);
                    }
                    
                    bytes = remaining_bytes;
                }
                Err(e) => {
                    match e {
                        nom::Err::Error(e) => {
                            println!("Error({:?})", e.code);
                        }
                        nom::Err::Failure(e) => {
                            println!("Failure({:?})", e.code);
                        }
                        nom::Err::Incomplete(needed) => {
                            println!("Incomplete({:?})", needed);
                        }
                    }
                    break;
                }
            }
        }

        drop(out);

        

        Ok(())
    }
}
