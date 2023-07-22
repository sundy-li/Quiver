use arrow::types::NativeType;
use num::Float;
use ordered_float::OrderedFloat;

use std::{
    hash::Hash,
    ops::{BitXor, Shl, ShlAssign, Shr, ShrAssign},
};

use crate::util::AsBytes;

pub trait DoubleType: AsBytes + Copy + Clone + NativeType + Float {
    type OrderType: std::fmt::Debug
        + std::fmt::Display
        + Eq
        + Hash
        + PartialOrd
        + Hash
        + Copy
        + Clone;

    type BitType: Eq
        + NativeType
        + Hash
        + PartialOrd
        + Hash
        + AsBytes
        + BitXor<Output = Self::BitType>
        + ShlAssign
        + Shl<usize, Output = Self::BitType>
        + Shr<usize, Output = Self::BitType>
        + ShrAssign;

    fn as_order(&self) -> Self::OrderType;

    fn from_order(order: Self::OrderType) -> Self;

    fn as_bits(&self) -> Self::BitType;
    fn from_bits_val(bits: Self::BitType) -> Self;

    fn leading_zeros(bit_value: &Self::BitType) -> u32;
    fn trailing_zeros(bit_value: &Self::BitType) -> u32;
}

macro_rules! double_type {
    ($type:ty, $order_type: ty,  $bit_type: ty) => {
        impl DoubleType for $type {
            type OrderType = $order_type;
            type BitType = $bit_type;

            fn as_order(&self) -> Self::OrderType {
                OrderedFloat(*self)
            }

            fn from_order(order: Self::OrderType) -> Self {
                order.0
            }

            fn as_bits(&self) -> Self::BitType {
                self.to_bits()
            }

            fn from_bits_val(bits: Self::BitType) -> Self {
                Self::from_bits(bits)
            }

            fn leading_zeros(bit_value: &Self::BitType) -> u32 {
                bit_value.leading_zeros()
            }

            fn trailing_zeros(bit_value: &Self::BitType) -> u32 {
                bit_value.trailing_zeros()
            }
        }
    };
}

type F32 = OrderedFloat<f32>;
type F64 = OrderedFloat<f64>;

double_type!(f32, F32, u32);
double_type!(f64, F64, u64);
