use {
    enum_iterator::{all, Sequence},
    std::io::{self, BufReader, Read, Write},
};

#[derive(Debug, Serialize, Deserialize, Sequence)]
pub enum CompressionMethod {
    NoCompression,
    Bzip2,
    Gzip,
    Zstd,
}

fn decompress_reader<'a, R: Read + 'a>(
    method: CompressionMethod,
    stream: R,
) -> Result<Box<dyn Read + 'a>, io::Error> {
    let buf_reader = BufReader::new(stream);
    let decompress_reader: Box<dyn Read> = match method {
        CompressionMethod::Bzip2 => Box::new(bzip2::bufread::BzDecoder::new(buf_reader)),
        CompressionMethod::Gzip => Box::new(flate2::read::GzDecoder::new(buf_reader)),
        CompressionMethod::Zstd => Box::new(zstd::stream::read::Decoder::new(buf_reader)?),
        CompressionMethod::NoCompression => Box::new(buf_reader),
    };
    Ok(decompress_reader)
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, io::Error> {
    let method_size = bincode::serialized_size(&CompressionMethod::NoCompression).unwrap();
    if (data.len() as u64) < method_size {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("data len too small: {}", data.len()),
        ));
    }
    let method = bincode::deserialize(&data[..method_size as usize]).map_err(|err| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("method deserialize failed: {err}"),
        )
    })?;

    let mut reader = decompress_reader(method, &data[method_size as usize..])?;
    let mut uncompressed_data = vec![];
    reader.read_to_end(&mut uncompressed_data)?;
    Ok(uncompressed_data)
}

pub fn compress(method: CompressionMethod, data: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut compressed_data = bincode::serialize(&method).unwrap();
    compressed_data.extend(match method {
        CompressionMethod::Bzip2 => {
            let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
            e.write_all(data)?;
            e.finish()?
        }
        CompressionMethod::Gzip => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data)?;
            e.finish()?
        }
        CompressionMethod::Zstd => {
            let mut e = zstd::stream::write::Encoder::new(Vec::new(), 0).unwrap();
            e.write_all(data)?;
            e.finish()?
        }
        CompressionMethod::NoCompression => data.to_vec(),
    });

    Ok(compressed_data)
}

pub fn compress_best(data: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut candidates = vec![];
    for method in all::<CompressionMethod>() {
        candidates.push(compress(method, data)?);
    }

    Ok(candidates
        .into_iter()
        .min_by(|a, b| a.len().cmp(&b.len()))
        .unwrap())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_compress_uncompress() {
        let data = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        assert_eq!(
            decompress(&compress_best(&data).expect("compress_best")).expect("decompress"),
            data
        );
    }

    #[test]
    fn test_compress() {
        let data = vec![0; 256];
        assert!(compress_best(&data).expect("compress_best").len() < data.len());
    }
}
