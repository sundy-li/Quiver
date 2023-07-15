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

use std::io::Cursor;
use std::marker::PhantomData;

use crate::compression::integer::{decompress_integer, IntegerType};
use crate::read::{read_basic::*, BufReader, NativeReadBuf, PageIterator};
use crate::PageMeta;
use arrow::array::Array;
use arrow::array::PrimitiveArray;
use arrow::bitmap::MutableBitmap;
use arrow::buffer::Buffer;
use arrow::datatypes::DataType;
use arrow::error::Result;
use arrow::io::parquet::read::{InitNested, NestedState};
use parquet2::metadata::ColumnDescriptor;
use std::convert::TryInto;

pub struct IntegerIter<I, T>
where
    I: Iterator<Item = Result<(u64, Vec<u8>)>> + PageIterator + Send + Sync,
    T: IntegerType,
{
    iter: I,
    is_nullable: bool,
    data_type: DataType,
    scratch: Vec<u8>,
    _phantom: PhantomData<T>,
}

impl<I, T> IntegerIter<I, T>
where
    I: Iterator<Item = Result<(u64, Vec<u8>)>> + PageIterator + Send + Sync,
    T: IntegerType,
{
    pub fn new(iter: I, is_nullable: bool, data_type: DataType) -> Self {
        Self {
            iter,
            is_nullable,
            data_type,
            scratch: vec![],
            _phantom: PhantomData,
        }
    }
}

impl<I, T> IntegerIter<I, T>
where
    I: Iterator<Item = Result<(u64, Vec<u8>)>> + PageIterator + Send + Sync,
    T: IntegerType,
    Vec<u8>: TryInto<T::Bytes>,
{
    fn deserialize(&mut self, num_values: u64, buffer: Vec<u8>) -> Result<Box<dyn Array>> {
        let length = num_values as usize;
        let mut reader = BufReader::with_capacity(buffer.len(), Cursor::new(buffer));
        let validity = if self.is_nullable {
            let mut validity_builder = MutableBitmap::with_capacity(length);
            read_validity(&mut reader, length, &mut validity_builder)?;
            Some(std::mem::take(&mut validity_builder).into())
        } else {
            None
        };
        let mut values: Vec<T> = Vec::with_capacity(length);

        decompress_integer(&mut reader, length, &mut values, &mut self.scratch)?;
        assert_eq!(values.len(), length);

        let mut buffer = reader.into_inner().into_inner();
        self.iter.swap_buffer(&mut buffer);

        let array = PrimitiveArray::<T>::try_new(self.data_type.clone(), values.into(), validity)?;
        Ok(Box::new(array) as Box<dyn Array>)
    }
}

impl<I, T> Iterator for IntegerIter<I, T>
where
    I: Iterator<Item = Result<(u64, Vec<u8>)>> + PageIterator + Send + Sync,
    T: IntegerType,
    Vec<u8>: TryInto<T::Bytes>,
{
    type Item = Result<Box<dyn Array>>;

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        match self.iter.nth(n) {
            Some(Ok((num_values, buffer))) => Some(self.deserialize(num_values, buffer)),
            Some(Err(err)) => Some(Result::Err(err)),
            None => None,
        }
    }

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(Ok((num_values, buffer))) => Some(self.deserialize(num_values, buffer)),
            Some(Err(err)) => Some(Result::Err(err)),
            None => None,
        }
    }
}

#[derive(Debug)]
pub struct IntegerNestedIter<I, T>
where
    I: Iterator<Item = Result<(u64, Vec<u8>)>> + PageIterator + Send + Sync,
    T: IntegerType,
{
    iter: I,
    data_type: DataType,
    leaf: ColumnDescriptor,
    init: Vec<InitNested>,
    scratch: Vec<u8>,
    _phantom: PhantomData<T>,
}

impl<I, T> IntegerNestedIter<I, T>
where
    I: Iterator<Item = Result<(u64, Vec<u8>)>> + PageIterator + Send + Sync,
    T: IntegerType,
{
    pub fn new(
        iter: I,
        data_type: DataType,
        leaf: ColumnDescriptor,
        init: Vec<InitNested>,
    ) -> Self {
        Self {
            iter,
            data_type,
            leaf,
            init,
            scratch: vec![],
            _phantom: PhantomData,
        }
    }
}

impl<I, T> IntegerNestedIter<I, T>
where
    I: Iterator<Item = Result<(u64, Vec<u8>)>> + PageIterator + Send + Sync,
    T: IntegerType,
    Vec<u8>: TryInto<T::Bytes>,
{
    fn deserialize(
        &mut self,
        num_values: u64,
        buffer: Vec<u8>,
    ) -> Result<(NestedState, Box<dyn Array>)> {
        let mut reader = BufReader::with_capacity(buffer.len(), Cursor::new(buffer));
        let (mut nested, validity) = read_validity_nested(
            &mut reader,
            num_values as usize,
            &self.leaf,
            self.init.clone(),
        )?;
        let length = nested.nested.pop().unwrap().len();

        let mut values = Vec::with_capacity(length);
        decompress_integer(&mut reader, length, &mut values, &mut self.scratch)?;
        assert_eq!(values.len(), length);

        let mut buffer = reader.into_inner().into_inner();
        self.iter.swap_buffer(&mut buffer);

        let array = PrimitiveArray::<T>::try_new(self.data_type.clone(), values.into(), validity)?;

        Ok((nested, Box::new(array) as Box<dyn Array>))
    }
}

impl<I, T> Iterator for IntegerNestedIter<I, T>
where
    I: Iterator<Item = Result<(u64, Vec<u8>)>> + PageIterator + Send + Sync,
    T: IntegerType,
    Vec<u8>: TryInto<T::Bytes>,
{
    type Item = Result<(NestedState, Box<dyn Array>)>;

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        match self.iter.nth(n) {
            Some(Ok((num_values, buffer))) => Some(self.deserialize(num_values, buffer)),
            Some(Err(err)) => Some(Result::Err(err)),
            None => None,
        }
    }

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(Ok((num_values, buffer))) => Some(self.deserialize(num_values, buffer)),
            Some(Err(err)) => Some(Result::Err(err)),
            None => None,
        }
    }
}

pub fn read_integer<T: IntegerType, R: NativeReadBuf>(
    reader: &mut R,
    is_nullable: bool,
    data_type: DataType,
    page_metas: Vec<PageMeta>,
) -> Result<Box<dyn Array>> {
    let num_values = page_metas.iter().map(|p| p.num_values as usize).sum();

    let mut scratch = vec![];
    let mut validity_builder = if is_nullable {
        Some(MutableBitmap::with_capacity(num_values))
    } else {
        None
    };
    let mut out_buffer: Vec<T> = Vec::with_capacity(num_values);
    for page_meta in page_metas {
        let length = page_meta.num_values as usize;
        if let Some(ref mut validity_builder) = validity_builder {
            read_validity(reader, length, validity_builder)?;
        }
        decompress_integer(reader, length, &mut out_buffer, &mut scratch)?;
    }
    let validity =
        validity_builder.map(|mut validity_builder| std::mem::take(&mut validity_builder).into());
    let values: Buffer<T> = std::mem::take(&mut out_buffer).into();

    let array = PrimitiveArray::<T>::try_new(data_type, values, validity)?;
    Ok(Box::new(array) as Box<dyn Array>)
}

pub fn read_nested_integer<T: IntegerType, R: NativeReadBuf>(
    reader: &mut R,
    data_type: DataType,
    leaf: ColumnDescriptor,
    init: Vec<InitNested>,
    page_metas: Vec<PageMeta>,
) -> Result<Vec<(NestedState, Box<dyn Array>)>> {
    let mut scratch = vec![];
    let mut results = Vec::with_capacity(page_metas.len());
    for page_meta in page_metas {
        let num_values = page_meta.num_values as usize;
        let (mut nested, validity) = read_validity_nested(reader, num_values, &leaf, init.clone())?;
        let length = nested.nested.pop().unwrap().len();

        let mut values = Vec::with_capacity(length);
        decompress_integer(reader, length, &mut values, &mut scratch)?;

        let array = PrimitiveArray::<T>::try_new(data_type.clone(), values.into(), validity)?;
        results.push((nested, Box::new(array) as Box<dyn Array>));
    }
    Ok(results)
}
