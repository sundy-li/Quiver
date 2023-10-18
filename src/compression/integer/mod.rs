mod bp;
mod delta;
mod delta_bp;
mod dict;
mod freq;
mod one_value;
mod rle;
mod traits;

use std::collections::HashMap;

use arrow::{
    array::{Array, MutablePrimitiveArray, PrimitiveArray},
    error::{Error, Result},
};
use rand::{thread_rng, Rng};

use crate::{
    read::{read_basic::read_compress_header, NativeReadBuf},
    util::env::{check_bitpack_env, check_dict_env, check_freq_env, check_rle_env},
    write::WriteOptions,
};

use self::delta_bp::DeltaBitpacking;
pub use self::dict::Dict;
pub use self::dict::DictEncoder;
pub use self::dict::RawNative;
pub use self::freq::Freq;
pub use self::one_value::OneValue;
pub use self::rle::Rle;
pub use self::traits::IntegerType;
use self::{bp::Bitpacking, delta::Delta};

use super::{basic::CommonCompression, is_valid, Compression};

pub fn compress_integer<T: IntegerType>(
    array: &PrimitiveArray<T>,
    write_options: WriteOptions,
    buf: &mut Vec<u8>,
) -> Result<()> {
    // choose compressor
    let stats = gen_stats(array);
    let compressor = choose_compressor(array, &stats, &write_options);

    log::info!(
        "choose integer compression : {:?}",
        compressor.to_compression()
    );

    let codec = u8::from(compressor.to_compression());
    buf.extend_from_slice(&codec.to_le_bytes());
    let pos = buf.len();
    buf.extend_from_slice(&[0u8; 8]);

    let compressed_size = match compressor {
        IntCompressor::Basic(c) => {
            let input_buf = bytemuck::cast_slice(array.values());
            c.compress(input_buf, buf)
        }
        IntCompressor::Extend(c) => c.compress(array, &stats, &write_options, buf),
    }?;
    buf[pos..pos + 4].copy_from_slice(&(compressed_size as u32).to_le_bytes());
    buf[pos + 4..pos + 8]
        .copy_from_slice(&((array.len() * std::mem::size_of::<T>()) as u32).to_le_bytes());

    log::debug!(
        "integer compress ratio {}",
        stats.total_bytes as f64 / compressed_size as f64
    );
    Ok(())
}

pub fn decompress_integer<T: IntegerType, R: NativeReadBuf>(
    reader: &mut R,
    length: usize,
    output: &mut Vec<T>,
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

    let compressor = IntCompressor::<T>::from_compression(compression)?;

    match compressor {
        IntCompressor::Basic(c) => {
            output.reserve(length);
            let out_slice = unsafe {
                core::slice::from_raw_parts_mut(
                    output.as_mut_ptr().add(output.len()) as *mut u8,
                    length * std::mem::size_of::<T>(),
                )
            };
            c.decompress(&input[..compressed_size], out_slice)?;
            unsafe { output.set_len(output.len() + length) };
        }
        IntCompressor::Extend(c) => {
            c.decompress(input, length, output)?;
        }
    }

    if use_inner {
        reader.consume(compressed_size);
    }
    Ok(())
}

pub trait IntegerCompression<T: IntegerType> {
    fn compress(
        &self,
        array: &PrimitiveArray<T>,
        stats: &IntegerStats<T>,
        write_options: &WriteOptions,
        output: &mut Vec<u8>,
    ) -> Result<usize>;
    fn decompress(&self, input: &[u8], length: usize, output: &mut Vec<T>) -> Result<()>;

    fn to_compression(&self) -> Compression;
    fn compress_ratio(&self, stats: &IntegerStats<T>) -> f64;
}

enum IntCompressor<T: IntegerType> {
    Basic(CommonCompression),
    Extend(Box<dyn IntegerCompression<T>>),
}

impl<T: IntegerType> IntCompressor<T> {
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
            Compression::Rle => Ok(Self::Extend(Box::new(Rle {}))),
            Compression::Dict => Ok(Self::Extend(Box::new(Dict {}))),
            Compression::OneValue => Ok(Self::Extend(Box::new(OneValue {}))),
            Compression::Freq => Ok(Self::Extend(Box::new(Freq {}))),
            Compression::Bitpacking => Ok(Self::Extend(Box::new(Bitpacking {}))),
            Compression::DeltaBitpacking => Ok(Self::Extend(Box::new(DeltaBitpacking {}))),
            Compression::Delta => Ok(Self::Extend(Box::new(Delta {}))),
            other => Err(Error::OutOfSpec(format!(
                "Unknown compression codec {other:?}",
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IntegerStats<T: IntegerType> {
    pub src: PrimitiveArray<T>,
    pub tuple_count: usize,
    pub total_bytes: usize,
    pub null_count: usize,
    pub average_run_length: f64,
    pub is_sorted: bool,
    pub min: T,
    pub max: T,
    pub distinct_values: HashMap<T, usize>,
    pub unique_count: usize,
    pub set_count: usize,
}

fn gen_stats<T: IntegerType>(array: &PrimitiveArray<T>) -> IntegerStats<T> {
    let mut stats = IntegerStats::<T> {
        src: array.clone(),
        tuple_count: array.len(),
        total_bytes: array.len() * std::mem::size_of::<T>(),
        null_count: array.null_count(),
        average_run_length: 0.0,
        is_sorted: true,
        min: T::default(),
        max: T::default(),
        distinct_values: HashMap::new(),
        unique_count: 0,
        set_count: array.len() - array.null_count(),
    };

    let mut is_init_value_initialized = false;
    let mut last_value = T::default();
    let mut run_count = 0;

    let validity = array.validity();
    for (i, current_value) in array.values().iter().cloned().enumerate() {
        if is_valid(&validity, i) {
            if current_value < last_value {
                stats.is_sorted = false;
            }

            if last_value != current_value {
                run_count += 1;
                last_value = current_value;
            }
        }

        *stats.distinct_values.entry(current_value).or_insert(0) += 1;

        if !is_init_value_initialized {
            is_init_value_initialized = true;
            stats.min = current_value;
            stats.max = current_value;
        }

        if current_value > stats.max {
            stats.max = current_value;
        } else if current_value < stats.min {
            stats.min = current_value;
        }
    }
    stats.unique_count = stats.distinct_values.len();
    stats.average_run_length = array.len() as f64 / run_count as f64;

    stats
}

fn choose_compressor<T: IntegerType>(
    _value: &PrimitiveArray<T>,
    stats: &IntegerStats<T>,
    write_options: &WriteOptions,
) -> IntCompressor<T> {
    #[cfg(debug_assertions)]
    {
        if check_freq_env()
            && !write_options
                .forbidden_compressions
                .contains(&Compression::Freq)
        {
            return IntCompressor::Extend(Box::new(Freq {}));
        }
        if check_dict_env()
            && !write_options
                .forbidden_compressions
                .contains(&Compression::Dict)
        {
            return IntCompressor::Extend(Box::new(Dict {}));
        }
        if check_rle_env()
            && !write_options
                .forbidden_compressions
                .contains(&Compression::Rle)
        {
            return IntCompressor::Extend(Box::new(Rle {}));
        }
        if check_bitpack_env()
            && !write_options
                .forbidden_compressions
                .contains(&Compression::Bitpacking)
        {
            return IntCompressor::Extend(Box::new(Bitpacking {}));
        }
    }
    let basic = IntCompressor::Basic(write_options.default_compression);
    if let Some(ratio) = write_options.default_compress_ratio {
        let mut max_ratio = ratio;
        let mut result = basic;
        let compressors: Vec<Box<dyn IntegerCompression<T>>> = vec![
            Box::new(OneValue {}) as _,
            Box::new(Delta {}) as _, //order matters
            Box::new(Freq {}) as _,
            Box::new(Dict {}) as _,
            Box::new(Rle {}) as _,
            Box::new(Bitpacking {}) as _,
            Box::new(DeltaBitpacking {}) as _,
        ];
        for c in compressors {
            if write_options
                .forbidden_compressions
                .contains(&c.to_compression())
            {
                continue;
            }
            let r = c.compress_ratio(stats);

            log::debug!(
                "compress ratio {:?} : {}, max_ratio: {}",
                c.to_compression(),
                r,
                max_ratio
            );

            if r > max_ratio {
                max_ratio = r;
                result = IntCompressor::Extend(c);

                if r == stats.tuple_count as f64 {
                    break;
                }
            }
        }
        result
    } else {
        basic
    }
}

fn compress_sample_ratio<T: IntegerType, C: IntegerCompression<T>>(
    c: &C,
    stats: &IntegerStats<T>,
    sample_count: usize,
    sample_size: usize,
) -> f64 {
    let mut rng = thread_rng();

    let stats = if stats.src.len() / sample_count <= sample_size {
        stats.clone()
    } else {
        let array = &stats.src;
        let separator = array.len() / sample_count;
        let remainder = array.len() % sample_count;
        let mut builder = MutablePrimitiveArray::with_capacity(sample_count * sample_size);
        for sample_i in 0..sample_count {
            let range_end = if sample_i == sample_count - 1 {
                separator + remainder
            } else {
                separator
            } - sample_size;

            let partition_begin = sample_i * separator + rng.gen_range(0..range_end);

            let mut s = array.clone();
            s.slice(partition_begin, sample_size);
            builder.extend_trusted_len(s.into_iter());
        }
        let sample_array: PrimitiveArray<T> = builder.into();
        gen_stats(&sample_array)
    };

    let size = c
        .compress(&stats.src, &stats, &WriteOptions::default(), &mut vec![])
        .unwrap_or(stats.total_bytes);

    stats.total_bytes as f64 / size as f64
}
