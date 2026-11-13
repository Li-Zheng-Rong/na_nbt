//! Zero-allocation NBT tag detection for `serde::Serialize` values.
//!
//! # Type Mapping Summary
//! - `bool`, `i8`, `u8` -> [`TagID::Byte`]
//! - `i16`, `u16` -> [`TagID::Short`]
//! - `i32`, `u32`, `char`, unit enum variants -> [`TagID::Int`]
//! - `i64`, `u64` -> [`TagID::Long`]
//! - `f32` -> [`TagID::Float`], `f64` -> [`TagID::Double`]
//! - `i128`, `u128` -> [`TagID::IntArray`] (4-int Minecraft UUID layout)
//! - `str` -> [`TagID::String`], `bytes` / `na_nbt::byte_array` -> [`TagID::ByteArray`]
//! - `na_nbt::int_array` -> [`TagID::IntArray`], `na_nbt::long_array` -> [`TagID::LongArray`]
//! - `seq`, `tuple`, `tuple_struct`, `na_nbt::list` -> [`TagID::List`]
//! - `option`, `unit`, `unit_struct`, `map`, `struct`, data enum variants -> [`TagID::Compound`]

use serde::{ser, Serialize};

use crate::{Error, Result, TagID};

struct TagProbe;

macro_rules! impl_probe_primitives {
    ($($method:ident($ty:ty) => $tag:ident),* $(,)?) => {
        $(
            #[inline(always)]
            fn $method(self, _v: $ty) -> Result<TagID> {
                Ok(TagID::$tag)
            }
        )*
    };
}

macro_rules! impl_probe_container {
    ($trait:ident, $item_fn:ident($($arg:ident: $arg_ty:ty),*) => $end_tag:ident) => {
        impl ser::$trait for TagProbe {
            type Ok = TagID;
            type Error = Error;

            #[inline(always)]
            fn $item_fn<T: ?Sized + Serialize>(&mut self, $($arg: $arg_ty,)* _value: &T) -> Result<()> {
                let _ = ($($arg,)*);
                Ok(())
            }

            #[inline(always)]
            fn end(self) -> Result<TagID> {
                Ok(TagID::$end_tag)
            }
        }
    };
}

impl ser::Serializer for TagProbe {
    type Ok = TagID;
    type Error = Error;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    impl_probe_primitives!(
        serialize_bool(bool) => Byte,
        serialize_i8(i8) => Byte,
        serialize_u8(u8) => Byte,
        serialize_i16(i16) => Short,
        serialize_u16(u16) => Short,
        serialize_i32(i32) => Int,
        serialize_u32(u32) => Int,
        serialize_char(char) => Int,
        serialize_i64(i64) => Long,
        serialize_u64(u64) => Long,
        serialize_f32(f32) => Float,
        serialize_f64(f64) => Double,
        serialize_str(&str) => String,
        serialize_bytes(&[u8]) => ByteArray,
    );

    #[cfg(feature = "i128")]
    impl_probe_primitives!(
        serialize_i128(i128) => IntArray,
        serialize_u128(u128) => IntArray,
    );

    #[inline(always)]
    fn serialize_none(self) -> Result<TagID> {
        Ok(TagID::Compound)
    }

    #[inline(always)]
    fn serialize_some<T: ?Sized + Serialize>(self, _v: &T) -> Result<TagID> {
        Ok(TagID::Compound)
    }

    #[inline(always)]
    fn serialize_unit(self) -> Result<TagID> {
        Ok(TagID::Compound)
    }

    #[inline(always)]
    fn serialize_unit_struct(self, _name: &'static str) -> Result<TagID> {
        Ok(TagID::Compound)
    }

    #[inline(always)]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _var: &'static str,
    ) -> Result<TagID> {
        Ok(TagID::Int)
    }

    #[inline(always)]
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<TagID> {
        match name {
            "na_nbt:byte_array" => Ok(TagID::ByteArray),
            "na_nbt:int_array" => Ok(TagID::IntArray),
            "na_nbt:long_array" => Ok(TagID::LongArray),
            "na_nbt:list" => Ok(TagID::List),
            "na_nbt:end" => Ok(TagID::End),
            "na_nbt:compound" => Ok(TagID::Compound),
            "na_nbt:mutf8" => Ok(TagID::String),
            _ if super::parse_empty_list_marker(name).is_some() => Ok(TagID::List),
            _ => value.serialize(self),
        }
    }

    #[inline(always)]
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _idx: u32,
        _var: &'static str,
        _value: &T,
    ) -> Result<TagID> {
        Ok(TagID::Compound)
    }

    #[inline(always)]
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self> {
        Ok(self)
    }

    #[inline(always)]
    fn serialize_tuple(self, _len: usize) -> Result<Self> {
        Ok(self)
    }

    #[inline(always)]
    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Self> {
        Ok(self)
    }

    #[inline(always)]
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _var: &'static str,
        _len: usize,
    ) -> Result<Self> {
        Ok(self)
    }

    #[inline(always)]
    fn serialize_map(self, _len: Option<usize>) -> Result<Self> {
        Ok(self)
    }

    #[inline(always)]
    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self> {
        Ok(self)
    }

    #[inline(always)]
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _var: &'static str,
        _len: usize,
    ) -> Result<Self> {
        Ok(self)
    }
}

impl_probe_container!(SerializeSeq, serialize_element() => List);
impl_probe_container!(SerializeTuple, serialize_element() => List);
impl_probe_container!(SerializeTupleStruct, serialize_field() => List);
impl_probe_container!(SerializeTupleVariant, serialize_field() => Compound);
impl_probe_container!(SerializeStruct, serialize_field(key: &'static str) => Compound);
impl_probe_container!(SerializeStructVariant, serialize_field(key: &'static str) => Compound);

impl ser::SerializeMap for TagProbe {
    type Ok = TagID;
    type Error = Error;

    #[inline(always)]
    fn serialize_key<T: ?Sized + Serialize>(&mut self, _key: &T) -> Result<()> {
        Ok(())
    }

    #[inline(always)]
    fn serialize_value<T: ?Sized + Serialize>(&mut self, _value: &T) -> Result<()> {
        Ok(())
    }

    #[inline(always)]
    fn end(self) -> Result<TagID> {
        Ok(TagID::Compound)
    }
}

/// Returns the [`TagID`] that `value` would serialize to without allocating or writing bytes.
///
/// ```rust
/// use na_nbt::{TagID, tag_of};
///
/// assert_eq!(tag_of(&42i32), TagID::Int);
/// assert_eq!(tag_of(&"hello"), TagID::String);
/// assert_eq!(tag_of(&vec![1i32, 2, 3]), TagID::List);
/// assert_eq!(tag_of(&Some(42i32)), TagID::Compound);
/// assert_eq!(tag_of(&Option::<i32>::None), TagID::Compound);
/// ```
#[inline]
pub fn tag_of<T: ?Sized + Serialize>(value: &T) -> TagID {
    // SAFETY: Every `TagProbe` method unconditionally returns `Ok(TagID)`.
    unsafe { value.serialize(TagProbe).unwrap_unchecked() }
}
