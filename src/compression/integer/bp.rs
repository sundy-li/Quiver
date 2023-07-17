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

use std::io::BufRead;

use arrow::array::PrimitiveArray;
use bitpacking::{BitPacker, BitPacker4x};

use arrow::error::Result;
use byteorder::ReadBytesExt;

use crate::{
    compression::{Compression, SAMPLE_COUNT, SAMPLE_SIZE},
    write::WriteOptions,
};

use super::{compress_sample_ratio, IntegerCompression, IntegerStats, IntegerType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bitpacking {}

impl<T: IntegerType> IntegerCompression<T> for Bitpacking {
    fn compress(
        &self,
        array: &PrimitiveArray<T>,
        _stats: &IntegerStats<T>,
        _write_options: &WriteOptions,
        output: &mut Vec<u8>,
    ) -> Result<usize> {
        let start: usize = output.len();
        let bitpacker = BitPacker4x::new();
        let my_data = bytemuck::cast_slice(array.values().as_slice());

        for chunk in my_data.chunks(BitPacker4x::BLOCK_LEN) {
            let num_bits: u8 = bitpacker.num_bits(chunk);
            output.push(num_bits);
            output.reserve(BitPacker4x::BLOCK_LEN * 4);

            let out_slice = unsafe {
                core::slice::from_raw_parts_mut(
                    output.as_mut_ptr().add(output.len()) as *mut u8,
                    BitPacker4x::BLOCK_LEN * 4,
                )
            };

            let size = bitpacker.compress(chunk, out_slice, num_bits);
            unsafe { output.set_len(output.len() + size) };
        }

        Ok(output.len() - start)
    }

    fn decompress(&self, mut input: &[u8], length: usize, output: &mut Vec<T>) -> Result<()> {
        let bitpacker = BitPacker4x::new();

        for _ in (0..length).step_by(BitPacker4x::BLOCK_LEN) {
            let num_bits = input.read_u8()?;
            output.reserve(BitPacker4x::BLOCK_LEN);
            let out_slice = unsafe {
                core::slice::from_raw_parts_mut(
                    output.as_mut_ptr().add(output.len()) as *mut u32,
                    BitPacker4x::BLOCK_LEN,
                )
            };
            let size = bitpacker.decompress(input, out_slice, num_bits);
            input.consume(size);

            unsafe { output.set_len(output.len() + BitPacker4x::BLOCK_LEN) };
        }
        Ok(())
    }

    fn to_compression(&self) -> Compression {
        Compression::Bitpacking
    }

    fn compress_ratio(&self, stats: &IntegerStats<T>) -> f64 {
        if stats.min.as_i64() < 0
            || std::mem::size_of::<T>() != 4
            || stats.src.len() % BitPacker4x::BLOCK_LEN != 0
        {
            return 0.0f64;
        }

        #[cfg(debug_assertions)]
        {
            if option_env!("STRAWBOAT_BITPACKING_COMPRESSION") == Some("1") {
                return f64::MAX;
            }
        }

        compress_sample_ratio(self, stats, SAMPLE_COUNT, SAMPLE_SIZE)
    }
}
