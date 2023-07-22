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

#[allow(dead_code)]
mod bit_pack;
#[allow(dead_code)]
mod bit_util;
mod byte_writer;
pub mod memory;

pub use bit_util::*;
pub use byte_writer::ByteWriter;

#[macro_export]
macro_rules! with_match_primitive_type {(
    $key_type:expr, | $_:tt $T:ident | $($body:tt)*
) => ({
    macro_rules! __with_ty__ {( $_ $T:ident ) => ( $($body)* )}
    use arrow::datatypes::PrimitiveType::*;
    use arrow::types::{days_ms, months_days_ns, f16, i256};
    match $key_type {
        Int8 => __with_ty__! { i8 },
        Int16 => __with_ty__! { i16 },
        Int32 => __with_ty__! { i32 },
        Int64 => __with_ty__! { i64 },
        Int128 => __with_ty__! { i128 },
        Int256 => __with_ty__! { i256 },
        DaysMs => __with_ty__! { days_ms },
        MonthDayNano => __with_ty__! { months_days_ns },
        UInt8 => __with_ty__! { u8 },
        UInt16 => __with_ty__! { u16 },
        UInt32 => __with_ty__! { u32 },
        UInt64 => __with_ty__! { u64 },
        Float16 => __with_ty__! { f16 },
        Float32 => __with_ty__! { f32 },
        Float64 => __with_ty__! { f64 },
    }
})}

#[macro_export]
macro_rules! with_match_integer_double_type {
    (
    $key_type:expr, | $_:tt $I:ident | $body_integer:tt, | $__ :tt $T:ident | $body_primitive:tt
) => {{
        macro_rules! __with_ty__ {
            ( $_ $I:ident ) => {
                $body_integer
            };
        }
        macro_rules! __with_ty_double__ {
            ( $_ $T:ident ) => {
                $body_primitive
            };
        }
        use arrow::datatypes::PrimitiveType::*;
        use arrow::types::i256;
        match $key_type {
            Int8 => __with_ty__! { i8 },
            Int16 => __with_ty__! { i16 },
            Int32 => __with_ty__! { i32 },
            Int64 => __with_ty__! { i64 },
            Int128 => __with_ty__! { i128 },
            Int256 => __with_ty__! { i256 },
            UInt8 => __with_ty__! { u8 },
            UInt16 => __with_ty__! { u16 },
            UInt32 => __with_ty__! { u32 },
            UInt64 => __with_ty__! { u64 },

            Float32 => __with_ty_double__! { f32 },
            Float64 => __with_ty_double__! { f64 },
            Float16 => unreachable! {},
            DaysMs => unreachable!(),
            MonthDayNano => unreachable!(),
        }
    }};
}
