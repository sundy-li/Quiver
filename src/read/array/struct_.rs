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

use arrow::array::{Array, StructArray};
use arrow::datatypes::{DataType, Field};
use arrow::error::Result;
use arrow::io::parquet::read::NestedState;

use crate::read::deserialize::NestedIters;

type StructValues = Vec<Option<Result<(NestedState, Box<dyn Array>)>>>;

/// An iterator adapter over [`DynIter`] assumed to be encoded as Struct arrays
pub struct StructIterator<'a> {
    iters: Vec<NestedIters<'a>>,
    fields: Vec<Field>,
}

impl<'a> StructIterator<'a> {
    /// Creates a new [`StructIterator`] with `iters` and `fields`.
    pub fn new(iters: Vec<NestedIters<'a>>, fields: Vec<Field>) -> Self {
        assert_eq!(iters.len(), fields.len());
        Self { iters, fields }
    }
}

impl<'a> StructIterator<'a> {
    fn deserialize(
        &mut self,
        values: StructValues,
    ) -> Option<Result<(NestedState, Box<dyn Array>)>> {
        // This code is copied from arrow2 `StructIterator` and adds a custom `nth` method implementation
        // https://github.com/jorgecarleitao/arrow2/blob/main/src/io/parquet/read/deserialize/struct_.rs
        if values.iter().any(|x| x.is_none()) {
            return None;
        }

        // todo: unzip of Result not yet supportted in stable Rust
        let mut nested = vec![];
        let mut new_values = vec![];
        for x in values {
            match x.unwrap() {
                Ok((nest, values)) => {
                    new_values.push(values);
                    nested.push(nest);
                }
                Err(e) => return Some(Err(e)),
            }
        }
        Some(Ok(create_struct(
            self.fields.clone(),
            &mut nested,
            new_values,
        )))
    }
}

impl<'a> Iterator for StructIterator<'a> {
    type Item = Result<(NestedState, Box<dyn Array>)>;

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        let values = self
            .iters
            .iter_mut()
            .map(|iter| iter.nth(n))
            .collect::<Vec<_>>();

        self.deserialize(values)
    }

    fn next(&mut self) -> Option<Self::Item> {
        let values = self
            .iters
            .iter_mut()
            .map(|iter| iter.next())
            .collect::<Vec<_>>();

        self.deserialize(values)
    }
}

pub fn create_struct(
    fields: Vec<Field>,
    nested: &mut Vec<NestedState>,
    values: Vec<Box<dyn Array>>,
) -> (NestedState, Box<dyn Array>) {
    let mut nested = nested.pop().unwrap();
    let (_, validity) = nested.nested.pop().unwrap().inner();
    (
        nested,
        Box::new(StructArray::new(
            DataType::Struct(fields),
            values,
            validity.and_then(|x| x.into()),
        )),
    )
}
