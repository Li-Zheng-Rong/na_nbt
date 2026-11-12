//! NBT tag type definitions.
//!
//! This module defines zero-sized marker types for each NBT tag type.
//! These types are used as type parameters to specify the NBT tag at compile time.
//!
//! # Tag Types
//!
//! ## Primitive Tags
//!
//! | Type | Tag ID | Rust Type | Description |
//! |------|--------|-----------|-------------|
//! | [`End`] | 0 | `()` | Marks the end of a compound tag |
//! | [`Byte`] | 1 | `i8` | Signed 8-bit integer |
//! | [`Short`] | 2 | `I16<O>` | Signed 16-bit integer (endianness-aware) |
//! | [`Int`] | 3 | `I32<O>` | Signed 32-bit integer (endianness-aware) |
//! | [`Long`] | 4 | `I64<O>` | Signed 64-bit integer (endianness-aware) |
//! | [`Float`] | 5 | `F32<O>` | 32-bit floating point (endianness-aware) |
//! | [`Double`] | 6 | `F64<O>` | 64-bit floating point (endianness-aware) |
//!
//! ## Array Tags
//!
//! | Type | Tag ID | Element Type | Description |
//! |------|--------|--------------|-------------|
//! | [`ByteArray`] | 7 | `i8` | Array of signed bytes |
//! | [`IntArray`] | 11 | `I32<O>` | Array of 32-bit integers |
//! | [`LongArray`] | 12 | `I64<O>` | Array of 64-bit integers |
//!
//! ## Container Tags
//!
//! | Type | Tag ID | Description |
//! |------|--------|-------------|
//! | [`String`] | 8 | MUTF-8 encoded string |
//! | [`List`] | 9 | Homogeneous  list |
//! | [`Compound`] | 10 | Key-value map (string → NBT value) |
//!
//! ## Typed Lists
//!
//! [`TypedList<T>`] represents a homogeneous list where elements are of type `T`.

use std::marker::PhantomData;

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigMut, ConfigRef, StringMut, VecMut, NBT, NBTBase, CompoundOwn, ListOwn,
    StringOwn, TypedListOwn, VecOwn, TagID,
};

/// Marker type for the End tag (Tag ID: 0).
///
/// Used to mark the end of a compound tag's entries.
/// Has no associated value (represented as `()`).
#[derive(Clone, Copy)]
pub struct End;

/// Marker type for the Byte tag (Tag ID: 1).
///
/// Represents a signed 8-bit integer.
#[derive(Clone, Copy)]
pub struct Byte;

/// Marker type for the Short tag (Tag ID: 2).
///
/// Represents a signed 16-bit integer with endianness awareness.
#[derive(Clone, Copy)]
pub struct Short;

/// Marker type for the Int tag (Tag ID: 3).
///
/// Represents a signed 32-bit integer with endianness awareness.
#[derive(Clone, Copy)]
pub struct Int;

/// Marker type for the Long tag (Tag ID: 4).
///
/// Represents a signed 64-bit integer with endianness awareness.
#[derive(Clone, Copy)]
pub struct Long;

/// Marker type for the Float tag (Tag ID: 5).
///
/// Represents a 32-bit floating point value with endianness awareness.
#[derive(Clone, Copy)]
pub struct Float;

/// Marker type for the Double tag (Tag ID: 6).
///
/// Represents a 64-bit floating point value with endianness awareness.
#[derive(Clone, Copy)]
pub struct Double;

/// Marker type for the ByteArray tag (Tag ID: 7).
///
/// Represents an array of signed bytes (`i8`).
#[derive(Clone, Copy)]
pub struct ByteArray;

/// Marker type for the String tag (Tag ID: 8).
///
/// Represents a MUTF-8 encoded string.
#[derive(Clone, Copy)]
pub struct String;

/// Marker type for the List tag (Tag ID: 9).
///
/// Represents a homogeneous list.
/// For type-safe homogeneous lists, see [`TypedList<T>`].
#[derive(Clone, Copy)]
pub struct List;

/// Marker type for the Compound tag (Tag ID: 10).
///
/// Represents a key-value map where keys are strings and values are NBT tags.
#[derive(Clone, Copy)]
pub struct Compound;

/// Marker type for the IntArray tag (Tag ID: 11).
///
/// Represents an array of signed 32-bit integers with endianness awareness.
#[derive(Clone, Copy)]
pub struct IntArray;

/// Marker type for the LongArray tag (Tag ID: 12).
///
/// Represents an array of signed 64-bit integers with endianness awareness.
#[derive(Clone, Copy)]
pub struct LongArray;

macro_rules! impl_nbt_tag {
    ($(
        $tag:ident => {
            id: $tag_id:ident,
            elem: $elem:ident,
            ref_ty($($ref_lt:lifetime, $ref_cfg:ident)?): $ref_ty:ty,
            mut_ty($mut_lt:lifetime, $mut_cfg:ident): $mut_ty:ty,
            own_ty($own_ord:ident): $own_ty:ty,
        }
    ),* $(,)?) => {
        $(
            impl NBTBase for $tag {
                const TAG_ID: TagID = TagID::$tag_id;
                type Element = $elem;
                type TypeRef<'a, Config: ConfigRef> = $ref_ty;
                type TypeMut<$mut_lt, $mut_cfg: ConfigMut> = $mut_ty;
                type Type<$own_ord: ByteOrder> = $own_ty;
            }
        )*
    };
}

impl_nbt_tag! {
    End => {
        id: End,
        elem: End,
        ref_ty(): (),
        mut_ty('a, Config): &'a mut (),
        own_ty(O): (),
    },
    Byte => {
        id: Byte,
        elem: Byte,
        ref_ty(): i8,
        mut_ty('a, Config): &'a mut i8,
        own_ty(O): i8,
    },
    Short => {
        id: Short,
        elem: Short,
        ref_ty(): i16,
        mut_ty('a, Config): &'a mut byteorder::I16<Config::ByteOrder>,
        own_ty(O): byteorder::I16<O>,
    },
    Int => {
        id: Int,
        elem: Int,
        ref_ty(): i32,
        mut_ty('a, Config): &'a mut byteorder::I32<Config::ByteOrder>,
        own_ty(O): byteorder::I32<O>,
    },
    Long => {
        id: Long,
        elem: Long,
        ref_ty(): i64,
        mut_ty('a, Config): &'a mut byteorder::I64<Config::ByteOrder>,
        own_ty(O): byteorder::I64<O>,
    },
    Float => {
        id: Float,
        elem: Float,
        ref_ty(): f32,
        mut_ty('a, Config): &'a mut byteorder::F32<Config::ByteOrder>,
        own_ty(O): byteorder::F32<O>,
    },
    Double => {
        id: Double,
        elem: Double,
        ref_ty(): f64,
        mut_ty('a, Config): &'a mut byteorder::F64<Config::ByteOrder>,
        own_ty(O): byteorder::F64<O>,
    },
    ByteArray => {
        id: ByteArray,
        elem: Byte,
        ref_ty('a, Config): Config::ByteArray<'a>,
        mut_ty('a, Config): VecMut<'a, i8>,
        own_ty(O): VecOwn<i8>,
    },
    String => {
        id: String,
        elem: Byte,
        ref_ty('a, Config): Config::String<'a>,
        mut_ty('a, Config): StringMut<'a>,
        own_ty(O): StringOwn,
    },
    List => {
        id: List,
        elem: List,
        ref_ty('a, Config): Config::List<'a>,
        mut_ty('a, Config): Config::ListMut<'a>,
        own_ty(O): ListOwn<O>,
    },
    Compound => {
        id: Compound,
        elem: Compound,
        ref_ty('a, Config): Config::Compound<'a>,
        mut_ty('a, Config): Config::CompoundMut<'a>,
        own_ty(O): CompoundOwn<O>,
    },
    IntArray => {
        id: IntArray,
        elem: Int,
        ref_ty('a, Config): Config::IntArray<'a>,
        mut_ty('a, Config): VecMut<'a, byteorder::I32<Config::ByteOrder>>,
        own_ty(O): VecOwn<byteorder::I32<O>>,
    },
    LongArray => {
        id: LongArray,
        elem: Long,
        ref_ty('a, Config): Config::LongArray<'a>,
        mut_ty('a, Config): VecMut<'a, byteorder::I64<Config::ByteOrder>>,
        own_ty(O): VecOwn<byteorder::I64<O>>,
    },
}

/// Marker type for a typed list (Tag ID: 9, same as List).
#[derive(Clone, Copy)]
pub struct TypedList<T: NBTBase>(PhantomData<T>);

impl<T: NBT> NBTBase for TypedList<T> {
    const TAG_ID: TagID = TagID::List;
    type Element = T;
    type TypeRef<'a, Config: ConfigRef> = Config::TypedList<'a, T>;
    type TypeMut<'a, Config: ConfigMut> = Config::TypedListMut<'a, T>;
    type Type<O: ByteOrder> = TypedListOwn<O, T>;
}
