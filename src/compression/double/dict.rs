// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use arrow::array::PrimitiveArray;

use arrow::error::Error;
use arrow::error::Result;
use byteorder::{LittleEndian, ReadBytesExt};

use crate::compression::get_bits_needed;
use crate::compression::integer::compress_integer;
use crate::compression::integer::decompress_integer;
use crate::compression::integer::Dict;
use crate::compression::integer::DictEncoder;
use crate::compression::integer::RawNative;
use crate::compression::Compression;
use crate::general_err;
use crate::write::WriteOptions;

use super::traits::DoubleType;
use super::DoubleCompression;
use super::DoubleStats;

impl<T: DoubleType> DoubleCompression<T> for Dict {
    fn compress(
        &self,
        array: &PrimitiveArray<T>,
        _stats: &DoubleStats<T>,
        write_options: &WriteOptions,
        output_buf: &mut Vec<u8>,
    ) -> Result<usize> {
        let start = output_buf.len();
        let mut encoder = DictEncoder::with_capacity(array.len());
        for val in array.iter() {
            match val {
                Some(val) => encoder.push(&RawNative { inner: *val }),
                None => {
                    if encoder.is_empty() {
                        encoder.push(&RawNative {
                            inner: T::default(),
                        });
                    } else {
                        encoder.push_last_index();
                    }
                }
            };
        }
        let indices = encoder.take_indices();

        // dict data use custom encoding
        let mut write_options = write_options.clone();
        write_options.forbidden_compressions.push(Compression::Dict);
        compress_integer(&indices, write_options, output_buf)?;

        let sets = encoder.get_sets();
        output_buf.extend_from_slice(&(sets.len() as u32).to_le_bytes());
        // data page use plain encoding
        for val in sets.iter() {
            let bs = val.inner.to_le_bytes();
            output_buf.extend_from_slice(bs.as_ref());
        }

        Ok(output_buf.len() - start)
    }

    fn decompress(&self, mut input: &[u8], length: usize, output: &mut Vec<T>) -> Result<()> {
        let mut indices: Vec<u32> = Vec::new();
        decompress_integer(&mut input, length, &mut indices, &mut vec![])?;

        let data_size = input.read_u32::<LittleEndian>()? as usize * std::mem::size_of::<T>();
        if input.len() < data_size {
            return Err(general_err!(
                "Invalid data size: {} less than {}",
                input.len(),
                data_size
            ));
        }
        let data: Vec<T> = input[0..data_size]
            .chunks(std::mem::size_of::<T>())
            .map(|chunk| match <T::Bytes>::try_from(chunk) {
                Ok(bs) => T::from_le_bytes(bs),
                Err(_e) => {
                    unreachable!()
                }
            })
            .collect();

        for i in indices.iter() {
            output.push(data[*i as usize]);
        }
        Ok(())
    }

    fn to_compression(&self) -> Compression {
        Compression::Dict
    }

    fn compress_ratio(&self, stats: &super::DoubleStats<T>) -> f64 {
        #[cfg(debug_assertions)]
        {
            if option_env!("STRAWBOAT_DICT_COMPRESSION") == Some("1") {
                return f64::MAX;
            }
        }

        const MIN_DICT_RATIO: usize = 3;
        if stats.unique_count * MIN_DICT_RATIO >= stats.tuple_count {
            return 0.0f64;
        }

        let mut after_size = stats.unique_count * std::mem::size_of::<T>()
            + stats.tuple_count * (get_bits_needed(stats.unique_count as u64) / 8) as usize;
        after_size += (stats.tuple_count) * 2 / 128;
        stats.total_bytes as f64 / after_size as f64
    }
}
