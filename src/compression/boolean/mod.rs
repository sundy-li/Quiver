mod rle;

use arrow::{
    array::BooleanArray,
    bitmap::{Bitmap, MutableBitmap},
    error::{Error, Result},
};

use crate::{
    read::{read_basic::read_compress_header, NativeReadBuf},
    write::WriteOptions,
};

use super::{basic::CommonCompression, integer::RLE, Compression};

pub fn compress_boolean(
    array: &BooleanArray,
    buf: &mut Vec<u8>,
    write_options: WriteOptions,
) -> Result<()> {
    // choose compressor
    let stats = gen_stats(array);
    let compressor = choose_compressor(array, &stats, &write_options);

    log::info!(
        "choose boolean compression : {:?}",
        compressor.to_compression()
    );

    let codec = u8::from(compressor.to_compression());
    buf.extend_from_slice(&codec.to_le_bytes());
    let pos = buf.len();
    buf.extend_from_slice(&[0u8; 8]);

    let compressed_size = match compressor {
        BooleanCompressor::Basic(c) => {
            let bitmap = array.values();
            let (_, slice_offset, _) = bitmap.as_slice();

            let bitmap = if slice_offset != 0 {
                // case where we can't slice the bitmap as the offsets are not multiple of 8
                Bitmap::from_trusted_len_iter(bitmap.iter())
            } else {
                bitmap.clone()
            };
            let (slice, _, _) = bitmap.as_slice();
            c.compress(slice, buf)
        }
        BooleanCompressor::Extend(c) => c.compress(array, write_options, buf),
    }?;
    buf[pos..pos + 4].copy_from_slice(&(compressed_size as u32).to_le_bytes());
    buf[pos + 4..pos + 8].copy_from_slice(&(array.len() as u32).to_le_bytes());
    Ok(())
}

pub fn decompress_boolean<R: NativeReadBuf>(
    reader: &mut R,
    length: usize,
    output: &mut MutableBitmap,
    scratch: &mut Vec<u8>,
) -> Result<()> {
    let (codec, compressed_size, _uncompressed_size) = read_compress_header(reader)?;
    let compression = Compression::from_codec(codec)?;

    // already fit in buffer
    let mut use_inner = false;
    reader.fill_buf()?;

    let input = if reader.buffer_bytes().len() >= compressed_size {
        use_inner = true;
        reader.buffer_bytes()
    } else {
        scratch.resize(compressed_size, 0);
        reader.read_exact(scratch.as_mut_slice())?;
        scratch.as_slice()
    };

    let compressor = BooleanCompressor::from_compression(compression)?;
    match compressor {
        BooleanCompressor::Basic(c) => {
            let bytes = (length + 7) / 8;
            let mut buffer = vec![0u8; bytes];
            c.decompress(&input[..compressed_size], &mut buffer)?;
            output.extend_from_slice(buffer.as_slice(), 0, length);
        }
        BooleanCompressor::Extend(c) => {
            c.decompress(input, length, output)?;
        }
    }

    if use_inner {
        reader.consume(compressed_size);
    }
    Ok(())
}

pub trait BooleanCompression {
    fn compress(
        &self,
        array: &BooleanArray,
        write_options: WriteOptions,
        output: &mut Vec<u8>,
    ) -> Result<usize>;
    fn decompress(&self, input: &[u8], length: usize, output: &mut MutableBitmap) -> Result<()>;
    fn to_compression(&self) -> Compression;

    fn compress_ratio(&self, stats: &BooleanStats) -> f64;
}

enum BooleanCompressor {
    Basic(CommonCompression),
    Extend(Box<dyn BooleanCompression>),
}

impl BooleanCompressor {
    fn to_compression(&self) -> Compression {
        match self {
            Self::Basic(c) => c.to_compression(),
            Self::Extend(c) => c.to_compression(),
        }
    }

    fn from_compression(compression: Compression) -> Result<Self> {
        if let Ok(c) = CommonCompression::try_from(&compression) {
            return Ok(Self::Basic(c));
        }
        match compression {
            Compression::RLE => Ok(Self::Extend(Box::new(RLE {}))),
            other => Err(Error::OutOfSpec(format!(
                "Unknown compression codec {other:?}",
            ))),
        }
    }
}

#[allow(dead_code)]
pub struct BooleanStats {
    pub rows: usize,
    pub null_count: usize,
    pub false_count: usize,
    pub true_count: usize,
    pub average_run_length: f64,
}

fn gen_stats(array: &BooleanArray) -> BooleanStats {
    let mut null_count = 0;
    let mut false_count = 0;
    let mut true_count = 0;

    let mut is_init_value_initialized = false;
    let mut last_value = false;
    let mut run_count = 0;

    for v in array.iter() {
        if !is_init_value_initialized {
            is_init_value_initialized = true;
            last_value = v.unwrap_or_default();
        }

        match v {
            Some(v) => {
                if v {
                    true_count += 1;
                } else {
                    false_count += 1;
                }

                if last_value != v {
                    run_count += 1;
                    last_value = v;
                }
            }
            None => null_count += 1,
        }
    }

    BooleanStats {
        rows: array.len(),
        null_count,
        false_count,
        true_count,
        average_run_length: array.len() as f64 / 8.0f64 / run_count as f64,
    }
}

fn choose_compressor(
    _array: &BooleanArray,
    stats: &BooleanStats,
    write_options: &WriteOptions,
) -> BooleanCompressor {
    let basic = BooleanCompressor::Basic(write_options.default_compression);
    if let Some(ratio) = write_options.default_compress_ratio {
        let mut max_ratio = ratio as f64;
        let mut result = basic;

        let compressors: Vec<Box<dyn BooleanCompression>> = vec![Box::new(RLE {}) as _];

        for encoder in compressors {
            if write_options
                .forbidden_compressions
                .contains(&encoder.to_compression())
            {
                continue;
            }

            let r = encoder.compress_ratio(stats);
            if r > max_ratio {
                max_ratio = r;
                result = BooleanCompressor::Extend(encoder);
            }
        }
        result
    } else {
        basic
    }
}
