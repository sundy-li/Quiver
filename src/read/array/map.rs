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

use arrow::array::Array;
use arrow::datatypes::Field;
use arrow::error::Result;
use arrow::io::parquet::read::{create_map, NestedState};

use crate::read::deserialize::DynIter;

/// An iterator adapter over [`DynIter`] assumed to be encoded as Map arrays
pub struct MapIterator<'a> {
    iter: DynIter<'a, Result<(NestedState, Box<dyn Array>)>>,
    field: Field,
}

impl<'a> MapIterator<'a> {
    /// Creates a new [`MapIterator`] with `iter` and `field`.
    pub fn new(iter: DynIter<'a, Result<(NestedState, Box<dyn Array>)>>, field: Field) -> Self {
        Self { iter, field }
    }
}

impl<'a> MapIterator<'a> {
    fn deserialize(
        &mut self,
        value: Option<Result<(NestedState, Box<dyn Array>)>>,
    ) -> Option<Result<(NestedState, Box<dyn Array>)>> {
        let (mut nested, values) = match value {
            Some(Ok((nested, values))) => (nested, values),
            Some(Err(err)) => return Some(Err(err)),
            None => return None,
        };
        let array = create_map(self.field.data_type().clone(), &mut nested, values);
        Some(Ok((nested, array)))
    }
}

impl<'a> Iterator for MapIterator<'a> {
    type Item = Result<(NestedState, Box<dyn Array>)>;

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        let value = self.iter.nth(n);
        self.deserialize(value)
    }

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.iter.next();
        self.deserialize(value)
    }
}
