//! Zero-allocation NBT tag detection and wire size estimation for `serde::Serialize` values.
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

use core::{convert::Infallible, fmt};

use serde::{ser, Serialize};

use crate::{Error, Result, TagID};

pub(crate) const PROBE_COMPOUND_HINT: &str = "na_nbt:probe_compound";
pub(crate) const PROBE_LIST_HINT: &str = "na_nbt:probe_list";

const MAX_PROBE_HINT: usize = 64 * 1024 * 1024;

#[inline(always)]
pub(crate) fn is_tag_probe<S: ?Sized>() -> bool {
    typeid::of::<S>() == typeid::of::<FastTagProbe>()
}

#[derive(Debug)]
enum ProbeOutcome {
    Done(TagID, u32),
    Custom,
}

impl fmt::Display for ProbeOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("tag probe")
    }
}

impl std::error::Error for ProbeOutcome {}

impl ser::Error for ProbeOutcome {
    #[cold]
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        Self::Custom
    }
}

struct FastTagProbe;

type NeverOk = ser::Impossible<Infallible, ProbeOutcome>;

macro_rules! impl_fast_probe_primitives {
    ($($method:ident($ty:ty) => ($tag:ident, $hint:expr)),* $(,)?) => {
        $(
            #[inline(always)]
            fn $method(self, _v: $ty) -> core::result::Result<Infallible, ProbeOutcome> {
                Err(ProbeOutcome::Done(TagID::$tag, $hint))
            }
        )*
    };
}

impl ser::Serializer for FastTagProbe {
    type Ok = Infallible;
    type Error = ProbeOutcome;

    type SerializeSeq = NeverOk;
    type SerializeTuple = NeverOk;
    type SerializeTupleStruct = NeverOk;
    type SerializeTupleVariant = NeverOk;
    type SerializeMap = NeverOk;
    type SerializeStruct = NeverOk;
    type SerializeStructVariant = NeverOk;

    impl_fast_probe_primitives!(
        serialize_bool(bool) => (Byte, 1),
        serialize_i8(i8) => (Byte, 1),
        serialize_u8(u8) => (Byte, 1),
        serialize_i16(i16) => (Short, 2),
        serialize_u16(u16) => (Short, 2),
        serialize_i32(i32) => (Int, 4),
        serialize_u32(u32) => (Int, 4),
        serialize_char(char) => (Int, 4),
        serialize_i64(i64) => (Long, 8),
        serialize_u64(u64) => (Long, 8),
        serialize_f32(f32) => (Float, 4),
        serialize_f64(f64) => (Double, 8),
    );

    #[cfg(feature = "i128")]
    impl_fast_probe_primitives!(
        serialize_i128(i128) => (IntArray, 20),
        serialize_u128(u128) => (IntArray, 20),
    );

    #[inline(always)]
    fn serialize_str(self, v: &str) -> core::result::Result<Infallible, ProbeOutcome> {
        let hint = if v.len() <= u16::MAX as usize {
            (2 + v.len()) as u32
        } else {
            0
        };
        Err(ProbeOutcome::Done(TagID::String, hint))
    }

    #[inline(always)]
    fn serialize_bytes(self, v: &[u8]) -> core::result::Result<Infallible, ProbeOutcome> {
        let hint = if v.len() <= MAX_PROBE_HINT {
            (4 + v.len()) as u32
        } else {
            0
        };
        Err(ProbeOutcome::Done(TagID::ByteArray, hint))
    }

    #[inline(always)]
    fn serialize_none(self) -> core::result::Result<Infallible, ProbeOutcome> {
        Err(ProbeOutcome::Done(TagID::Compound, 1))
    }

    #[inline(always)]
    fn serialize_some<T: ?Sized + Serialize>(
        self,
        _v: &T,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        Err(ProbeOutcome::Done(TagID::Compound, 16))
    }

    #[inline(always)]
    fn serialize_unit(self) -> core::result::Result<Infallible, ProbeOutcome> {
        Err(ProbeOutcome::Done(TagID::Compound, 1))
    }

    #[inline(always)]
    fn serialize_unit_struct(
        self,
        _name: &'static str,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        Err(ProbeOutcome::Done(TagID::Compound, 1))
    }

    #[inline(always)]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _var: &'static str,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        Err(ProbeOutcome::Done(TagID::Int, 4))
    }

    #[inline(always)]
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        match name {
            "na_nbt:byte_array" => Err(ProbeOutcome::Done(TagID::ByteArray, 4)),
            "na_nbt:int_array" => value.serialize(SubProbe::TupleArray(TagID::IntArray, 4)),
            "na_nbt:long_array" => value.serialize(SubProbe::TupleArray(TagID::LongArray, 8)),
            "na_nbt:list" => value.serialize(SubProbe::ListSeq),
            "na_nbt:end" => Err(ProbeOutcome::Done(TagID::End, 0)),
            "na_nbt:compound" => Err(ProbeOutcome::Done(TagID::Compound, 64)),
            "na_nbt:mutf8" => value.serialize(SubProbe::Mutf8),
            PROBE_COMPOUND_HINT => value.serialize(SubProbe::DomHint(TagID::Compound)),
            PROBE_LIST_HINT => value.serialize(SubProbe::DomHint(TagID::List)),
            _ if super::parse_empty_list_marker(name).is_some() => {
                Err(ProbeOutcome::Done(TagID::List, 5))
            }
            _ => value.serialize(self),
        }
    }

    #[inline(always)]
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _idx: u32,
        var: &'static str,
        _value: &T,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        let var_len = var.len().min(64) as u32;
        Err(ProbeOutcome::Done(TagID::Compound, 12 + var_len))
    }

    #[inline(always)]
    fn serialize_seq(
        self,
        len: Option<usize>,
    ) -> core::result::Result<Self::SerializeSeq, ProbeOutcome> {
        let hint = 5 + len.unwrap_or(0).min(1024) as u32 * 8;
        Err(ProbeOutcome::Done(TagID::List, hint))
    }

    #[inline(always)]
    fn serialize_tuple(
        self,
        len: usize,
    ) -> core::result::Result<Self::SerializeTuple, ProbeOutcome> {
        let hint = 5 + len.min(1024) as u32 * 8;
        Err(ProbeOutcome::Done(TagID::List, hint))
    }

    #[inline(always)]
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> core::result::Result<Self::SerializeTupleStruct, ProbeOutcome> {
        let hint = 5 + len.min(1024) as u32 * 8;
        Err(ProbeOutcome::Done(TagID::List, hint))
    }

    #[inline(always)]
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        var: &'static str,
        len: usize,
    ) -> core::result::Result<Self::SerializeTupleVariant, ProbeOutcome> {
        let hint = 9 + var.len().min(64) as u32 + len.min(1024) as u32 * 8;
        Err(ProbeOutcome::Done(TagID::Compound, hint))
    }

    #[inline(always)]
    fn serialize_map(
        self,
        len: Option<usize>,
    ) -> core::result::Result<Self::SerializeMap, ProbeOutcome> {
        let hint = 1 + len.unwrap_or(2).min(1024) as u32 * 20;
        Err(ProbeOutcome::Done(TagID::Compound, hint))
    }

    #[inline(always)]
    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> core::result::Result<Self::SerializeStruct, ProbeOutcome> {
        let hint = 1 + len.min(1024) as u32 * 20;
        Err(ProbeOutcome::Done(TagID::Compound, hint))
    }

    #[inline(always)]
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        var: &'static str,
        len: usize,
    ) -> core::result::Result<Self::SerializeStructVariant, ProbeOutcome> {
        let hint = 5 + var.len().min(64) as u32 + len.min(1024) as u32 * 20;
        Err(ProbeOutcome::Done(TagID::Compound, hint))
    }
}

#[derive(Clone, Copy)]
enum SubProbe {
    TupleArray(TagID, usize),
    ListSeq,
    Mutf8,
    DomHint(TagID),
}

impl SubProbe {
    #[inline(always)]
    fn fallback(self) -> core::result::Result<Infallible, ProbeOutcome> {
        match self {
            Self::TupleArray(tag, _) => Err(ProbeOutcome::Done(tag, 4)),
            Self::ListSeq => Err(ProbeOutcome::Done(TagID::List, 5)),
            Self::Mutf8 => Err(ProbeOutcome::Done(TagID::String, 2)),
            Self::DomHint(tag) => Err(ProbeOutcome::Done(tag, 0)),
        }
    }
}

macro_rules! impl_subprobe_fallback {
    ($($method:ident($ty:ty)),* $(,)?) => {
        $(
            #[inline(always)]
            fn $method(self, _v: $ty) -> core::result::Result<Infallible, ProbeOutcome> {
                self.fallback()
            }
        )*
    };
}

impl ser::Serializer for SubProbe {
    type Ok = Infallible;
    type Error = ProbeOutcome;

    type SerializeSeq = NeverOk;
    type SerializeTuple = NeverOk;
    type SerializeTupleStruct = NeverOk;
    type SerializeTupleVariant = NeverOk;
    type SerializeMap = NeverOk;
    type SerializeStruct = NeverOk;
    type SerializeStructVariant = NeverOk;

    #[inline(always)]
    fn is_human_readable(&self) -> bool {
        !matches!(self, Self::Mutf8)
    }

    impl_subprobe_fallback!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_u8(u8),
        serialize_i16(i16),
        serialize_u16(u16),
        serialize_i32(i32),
        serialize_u32(u32),
        serialize_char(char),
        serialize_i64(i64),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_str(&str),
    );

    #[cfg(feature = "i128")]
    impl_subprobe_fallback!(serialize_i128(i128), serialize_u128(u128),);

    #[inline(always)]
    fn serialize_u64(self, v: u64) -> core::result::Result<Infallible, ProbeOutcome> {
        match self {
            Self::DomHint(tag) => {
                let hint = (v as usize).min(MAX_PROBE_HINT) as u32;
                Err(ProbeOutcome::Done(tag, hint))
            }
            _ => self.fallback(),
        }
    }

    #[inline(always)]
    fn serialize_bytes(self, v: &[u8]) -> core::result::Result<Infallible, ProbeOutcome> {
        match self {
            Self::Mutf8 => {
                let hint = if v.len() <= u16::MAX as usize {
                    (2 + v.len()) as u32
                } else {
                    0
                };
                Err(ProbeOutcome::Done(TagID::String, hint))
            }
            _ => self.fallback(),
        }
    }

    #[inline(always)]
    fn serialize_none(self) -> core::result::Result<Infallible, ProbeOutcome> {
        self.fallback()
    }
    #[inline(always)]
    fn serialize_some<T: ?Sized + Serialize>(
        self,
        _v: &T,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        self.fallback()
    }
    #[inline(always)]
    fn serialize_unit(self) -> core::result::Result<Infallible, ProbeOutcome> {
        self.fallback()
    }
    #[inline(always)]
    fn serialize_unit_struct(
        self,
        _name: &'static str,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        self.fallback()
    }
    #[inline(always)]
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        self.fallback()
    }
    #[inline(always)]
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        self.fallback()
    }
    #[inline(always)]
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> core::result::Result<Infallible, ProbeOutcome> {
        self.fallback()
    }

    #[inline(always)]
    fn serialize_seq(
        self,
        len: Option<usize>,
    ) -> core::result::Result<Self::SerializeSeq, ProbeOutcome> {
        match self {
            Self::ListSeq => {
                let hint = 5 + len.unwrap_or(0).min(1024) as u32 * 8;
                Err(ProbeOutcome::Done(TagID::List, hint))
            }
            _ => Err(match self.fallback() {
                Ok(never) => match never {},
                Err(e) => e,
            }),
        }
    }

    #[inline(always)]
    fn serialize_tuple(
        self,
        len: usize,
    ) -> core::result::Result<Self::SerializeTuple, ProbeOutcome> {
        match self {
            Self::TupleArray(tag, elem_size) => {
                let hint = match len.checked_mul(elem_size) {
                    Some(bytes) if bytes <= MAX_PROBE_HINT => (4 + bytes) as u32,
                    _ => 0,
                };
                Err(ProbeOutcome::Done(tag, hint))
            }
            _ => Err(match self.fallback() {
                Ok(never) => match never {},
                Err(e) => e,
            }),
        }
    }

    #[inline(always)]
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> core::result::Result<Self::SerializeTupleStruct, ProbeOutcome> {
        Err(match self.fallback() {
            Ok(never) => match never {},
            Err(e) => e,
        })
    }
    #[inline(always)]
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> core::result::Result<Self::SerializeTupleVariant, ProbeOutcome> {
        Err(match self.fallback() {
            Ok(never) => match never {},
            Err(e) => e,
        })
    }
    #[inline(always)]
    fn serialize_map(
        self,
        _: Option<usize>,
    ) -> core::result::Result<Self::SerializeMap, ProbeOutcome> {
        Err(match self.fallback() {
            Ok(never) => match never {},
            Err(e) => e,
        })
    }
    #[inline(always)]
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> core::result::Result<Self::SerializeStruct, ProbeOutcome> {
        Err(match self.fallback() {
            Ok(never) => match never {},
            Err(e) => e,
        })
    }
    #[inline(always)]
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> core::result::Result<Self::SerializeStructVariant, ProbeOutcome> {
        Err(match self.fallback() {
            Ok(never) => match never {},
            Err(e) => e,
        })
    }
}

/// Non-short-circuiting fallback probe used only when a custom `Serialize` implementation
/// intercepts `S::Error` and constructs a custom error via `ser::Error::custom`.
struct FallbackTagProbe;

macro_rules! impl_fallback_primitives {
    ($($method:ident($ty:ty) => $tag:ident),* $(,)?) => {
        $(
            fn $method(self, _v: $ty) -> Result<TagID> {
                Ok(TagID::$tag)
            }
        )*
    };
}

macro_rules! impl_fallback_container {
    ($trait:ident, $item_fn:ident($($arg:ident: $arg_ty:ty),*) => $end_tag:ident) => {
        impl ser::$trait for FallbackTagProbe {
            type Ok = TagID;
            type Error = Error;

            fn $item_fn<T: ?Sized + Serialize>(&mut self, $($arg: $arg_ty,)* _value: &T) -> Result<()> {
                let _ = ($($arg,)*);
                Ok(())
            }

            fn end(self) -> Result<TagID> {
                Ok(TagID::$end_tag)
            }
        }
    };
}

impl ser::Serializer for FallbackTagProbe {
    type Ok = TagID;
    type Error = Error;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    impl_fallback_primitives!(
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
    impl_fallback_primitives!(
        serialize_i128(i128) => IntArray,
        serialize_u128(u128) => IntArray,
    );

    fn serialize_none(self) -> Result<TagID> {
        Ok(TagID::Compound)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _v: &T) -> Result<TagID> {
        Ok(TagID::Compound)
    }
    fn serialize_unit(self) -> Result<TagID> {
        Ok(TagID::Compound)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<TagID> {
        Ok(TagID::Compound)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<TagID> {
        Ok(TagID::Int)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<TagID> {
        match name {
            "na_nbt:byte_array" => Ok(TagID::ByteArray),
            "na_nbt:int_array" => Ok(TagID::IntArray),
            "na_nbt:long_array" => Ok(TagID::LongArray),
            "na_nbt:list" | PROBE_LIST_HINT => Ok(TagID::List),
            "na_nbt:end" => Ok(TagID::End),
            "na_nbt:compound" | PROBE_COMPOUND_HINT => Ok(TagID::Compound),
            "na_nbt:mutf8" => Ok(TagID::String),
            _ if super::parse_empty_list_marker(name).is_some() => Ok(TagID::List),
            _ => value.serialize(self),
        }
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<TagID> {
        Ok(TagID::Compound)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self> {
        Ok(self)
    }
}

impl_fallback_container!(SerializeSeq, serialize_element() => List);
impl_fallback_container!(SerializeTuple, serialize_element() => List);
impl_fallback_container!(SerializeTupleStruct, serialize_field() => List);
impl_fallback_container!(SerializeTupleVariant, serialize_field() => Compound);
impl_fallback_container!(SerializeStruct, serialize_field(key: &'static str) => Compound);
impl_fallback_container!(SerializeStructVariant, serialize_field(key: &'static str) => Compound);

impl ser::SerializeMap for FallbackTagProbe {
    type Ok = TagID;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, _key: &T) -> Result<()> {
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, _value: &T) -> Result<()> {
        Ok(())
    }
    fn end(self) -> Result<TagID> {
        Ok(TagID::Compound)
    }
}

#[cold]
#[inline(never)]
fn probe_fallback<T: ?Sized + Serialize>(value: &T) -> Result<(TagID, usize)> {
    value.serialize(FallbackTagProbe).map(|tag| (tag, 0))
}

#[inline(always)]
pub(crate) fn probe_of<T: ?Sized + Serialize>(value: &T) -> Result<(TagID, usize)> {
    match value.serialize(FastTagProbe) {
        Ok(never) => match never {},
        Err(ProbeOutcome::Done(tag, hint)) => Ok((tag, hint as usize)),
        Err(ProbeOutcome::Custom) => probe_fallback(value),
    }
}

#[inline(always)]
pub(crate) fn probe_tag_id<T: ?Sized + Serialize>(value: &T) -> Result<TagID> {
    match value.serialize(FastTagProbe) {
        Ok(never) => match never {},
        Err(ProbeOutcome::Done(tag, _)) => Ok(tag),
        Err(ProbeOutcome::Custom) => probe_fallback(value).map(|(tag, _)| tag),
    }
}

/// Returns the [`TagID`] that `value` would serialize to without allocating or writing bytes,
/// or [`TagID::End`] if `value`'s [`Serialize`] implementation fails.
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
    probe_tag_id(value).unwrap_or(TagID::End)
}
