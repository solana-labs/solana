use {
    enum_iterator::IntoEnumIterator,
    rayon::prelude::*,
    std::io::{self, BufReader, Read, Write},
};

#[derive(Debug, Serialize, Deserialize, IntoEnumIterator)]
pub enum CompressionMethod {
    NoCompression,
    Bzip2,
    Gzip,
    Zstd,
}

impl CompressionMethod {
    fn serialized_header_size() -> u64 {
        bincode::serialized_size(&CompressionMethod::NoCompression).unwrap()
    }
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
    let method_size = CompressionMethod::serialized_header_size();
    if (data.len() as u64) < method_size {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("data len too small: {}", data.len()),
        ));
    }
    let method = bincode::deserialize(&data[..method_size as usize]).map_err(|err| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("method deserialize failed: {}", err),
        )
    })?;

    let mut reader = decompress_reader(method, &data[method_size as usize..])?;
    let mut uncompressed_data = vec![];
    reader.read_to_end(&mut uncompressed_data)?;
    Ok(uncompressed_data)
}

pub fn compress(method: CompressionMethod, data: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut compressed_data = bincode::serialize(&method).unwrap();
    compressed_data.extend(
        match method {
            CompressionMethod::Bzip2 => {
                let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
                e.write_all(data)?;
                e.finish()?
            }
            CompressionMethod::Gzip => {
                let mut e =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data)?;
                e.finish()?
            }
            CompressionMethod::Zstd => {
                let mut e = zstd::stream::write::Encoder::new(Vec::new(), 0).unwrap();
                e.write_all(data)?;
                e.finish()?
            }
            CompressionMethod::NoCompression => data.to_vec(),
        }
        .into_iter(),
    );

    Ok(compressed_data)
}

pub fn compress_best(data: &[u8]) -> Result<Vec<u8>, io::Error> {
    if data.len() < 128 {
        compress_best_vec(data)
    } else {
        compress_best_par(data)
    }
}

pub fn compress_best_vec(data: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut candidates = vec![];
    for method in CompressionMethod::into_enum_iter() {
        candidates.push(compress(method, data)?);
    }

    Ok(candidates
        .into_iter()
        .min_by(|a, b| a.len().cmp(&b.len()))
        .unwrap())
}

pub fn compress_best_par(data: &[u8]) -> Result<Vec<u8>, io::Error> {
    let methods: Vec<CompressionMethod> = CompressionMethod::into_enum_iter().collect();
    let candidates: Vec<Vec<u8>> = methods
        .into_par_iter()
        .map(|v| compress(v, data).unwrap())
        .collect();

    Ok(candidates
        .into_iter()
        .min_by(|a, b| a.len().cmp(&b.len()))
        .unwrap())
}

#[allow(dead_code)]
pub fn compress_best_loop(data: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut best = vec![];
    for method in CompressionMethod::into_enum_iter() {
        let c = compress(method, data)?;
        if best.is_empty() || c.len() < best.len() {
            best = c;
        }
    }
    Ok(best)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Instant;

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
        assert!(
            compress_best(&data).expect("compress_best").len()
                <= data.len() + CompressionMethod::serialized_header_size() as usize
        );
    }

    #[test]
    fn test_compress_bench() {
        println!("size, old, loop, par");

        for size in [16, 32, 64, 128, 256, 512, 1024, 2048, 4098, 8196] {
            let data: Vec<u8> = (0..size).map(|_| rand::random::<u8>()).collect();

            let start = Instant::now();
            for _ in 1..100 {
                assert!(
                    compress_best(&data).expect("compress_best").len()
                        <= CompressionMethod::serialized_header_size() as usize + data.len()
                );
            }
            let time_old = start.elapsed();

            let start = Instant::now();
            for _ in 1..100 {
                assert!(
                    compress_best_loop(&data).expect("compress_best").len()
                        <= CompressionMethod::serialized_header_size() as usize + data.len()
                );
            }
            let time_loop = start.elapsed();

            let start = Instant::now();
            for _ in 1..100 {
                assert!(
                    compress_best_par(&data).expect("compress_best").len()
                        <= CompressionMethod::serialized_header_size() as usize + data.len()
                );
            }
            let time_par = start.elapsed();

            println!("{}, {:?}, {:?}, {:?}", size, time_old, time_loop, time_par);
        }
    }
}
