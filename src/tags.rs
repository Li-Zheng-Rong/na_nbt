use std::{hint::assert_unchecked, marker::PhantomData, ptr};

use zerocopy::{byteorder, BigEndian};

use crate::{
    cold_path,
    owned::slot::{slot_list_ptr, slot_read, slot_write, SIZE_DYN},
    write::ByteSink,
    ByteOrder, Compound, CompoundRef, Error, List, ListRef, MUTF8Str, MUTF8String, Mut, Result,
    ScalarMut, TypedList, TypedListRef, Value, ValueMut, ValueRef,
};

const TAG_ELEM_SIZES: [u8; 13] = [0, 1, 2, 4, 8, 4, 8, 1, 0, 0, 0, 4, 8];

/// NBT tag type identifier (`0..=12`).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum TagID {
    /// `TAG_End` (`0`): marks the end of a compound tag or an empty document/list.
    End = 0,
    /// `TAG_Byte` (`1`): signed 8-bit integer (`i8`).
    Byte = 1,
    /// `TAG_Short` (`2`): signed 16-bit integer (`i16`).
    Short = 2,
    /// `TAG_Int` (`3`): signed 32-bit integer (`i32`).
    Int = 3,
    /// `TAG_Long` (`4`): signed 64-bit integer (`i64`).
    Long = 4,
    /// `TAG_Float` (`5`): 32-bit IEEE-754 float (`f32`).
    Float = 5,
    /// `TAG_Double` (`6`): 64-bit IEEE-754 float (`f64`).
    Double = 6,
    /// `TAG_Byte_Array` (`7`): length-prefixed array of signed 8-bit integers (`[i8]`).
    ByteArray = 7,
    /// `TAG_String` (`8`): length-prefixed MUTF-8 string (`MUTF8Str`).
    String = 8,
    /// `TAG_List` (`9`): homogeneous length-prefixed list of payloads.
    List = 9,
    /// `TAG_Compound` (`10`): map of MUTF-8 named tags terminated by `TAG_End`.
    Compound = 10,
    /// `TAG_Int_Array` (`11`): length-prefixed array of signed 32-bit integers (`[i32]`).
    IntArray = 11,
    /// `TAG_Long_Array` (`12`): length-prefixed array of signed 64-bit integers (`[i64]`).
    LongArray = 12,
}

impl TagID {
    #[inline(always)]
    pub(crate) const unsafe fn from_u8_unchecked(value: u8) -> Self {
        unsafe { std::mem::transmute(value) }
    }

    #[inline(always)]
    pub(crate) const unsafe fn elem_size_raw(tag_id: u8) -> usize {
        unsafe { assert_unchecked(tag_id < 13) };
        TAG_ELEM_SIZES[tag_id as usize] as usize
    }

    /// Converts a raw byte (`0..=12`) into a [`TagID`], returning [`Error::INVALID`] otherwise.
    #[inline]
    pub const fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::End),
            1 => Ok(Self::Byte),
            2 => Ok(Self::Short),
            3 => Ok(Self::Int),
            4 => Ok(Self::Long),
            5 => Ok(Self::Float),
            6 => Ok(Self::Double),
            7 => Ok(Self::ByteArray),
            8 => Ok(Self::String),
            9 => Ok(Self::List),
            10 => Ok(Self::Compound),
            11 => Ok(Self::IntArray),
            12 => Ok(Self::LongArray),
            _ => {
                cold_path();
                Err(Error::INVALID(value))
            }
        }
    }

    /// Returns `true` if this tag is [`TagID::End`].
    #[inline]
    pub const fn is_end(self) -> bool {
        matches!(self, Self::End)
    }

    /// Returns `true` for `End`, `Byte`, `Short`, `Int`, `Long`, `Float`, `Double`.
    ///
    /// ```
    /// use na_nbt::TagID;
    /// assert!(TagID::Int.is_primitive());
    /// assert!(!TagID::List.is_primitive());
    /// ```
    #[inline]
    pub const fn is_primitive(self) -> bool {
        (self as u8) <= 6
    }

    /// Returns `true` for `ByteArray`, `IntArray`, `LongArray`.
    ///
    /// ```
    /// use na_nbt::TagID;
    /// assert!(TagID::IntArray.is_array());
    /// assert!(!TagID::List.is_array());
    /// ```
    #[inline]
    pub const fn is_array(self) -> bool {
        matches!(self, Self::ByteArray | Self::IntArray | Self::LongArray)
    }

    /// Returns `true` for `List`, `Compound`.
    ///
    /// ```
    /// use na_nbt::TagID;
    /// assert!(TagID::Compound.is_composite());
    /// assert!(!TagID::Int.is_composite());
    /// ```
    #[inline]
    pub const fn is_composite(self) -> bool {
        matches!(self, Self::List | Self::Compound)
    }
}

mod private {
    pub trait SealedSource {}
    pub trait SealedTag {}
}

/// Zero-copy borrowed document source marker for [`ValueRef`], [`CompoundRef`], and [`ListRef`].
pub struct Borrowed<O: ByteOrder = BigEndian>(PhantomData<fn() -> O>);

impl<O: ByteOrder> private::SealedSource for Borrowed<O> {}
impl<O: ByteOrder> Copy for Borrowed<O> {}
impl<O: ByteOrder> Clone for Borrowed<O> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

/// Owned DOM source marker for [`ValueRef`], [`CompoundRef`], and [`ListRef`].
pub struct Owned<O: ByteOrder = BigEndian>(PhantomData<fn() -> O>);

impl<O: ByteOrder> private::SealedSource for Owned<O> {}
impl<O: ByteOrder> Copy for Owned<O> {}
impl<O: ByteOrder> Clone for Owned<O> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

/// Backing storage marker for read-only NBT views ([`Borrowed<O>`] or [`Owned<O>`]).
pub trait Source: private::SealedSource + Copy + Clone + Send + Sync + Sized + 'static {
    /// Wire byte order used by multi-byte integers and floats in this source.
    type Order: ByteOrder;

    #[doc(hidden)]
    type CompoundCursor<'a>: Copy + Clone + Send + Sync;
    #[doc(hidden)]
    type ListCursor<'a>: Copy + Clone + Send + Sync;
    #[doc(hidden)]
    type IterCursor: Copy + Clone + Send + Sync;

    #[doc(hidden)]
    fn empty_compound<'a>() -> Self::CompoundCursor<'a>;
    #[doc(hidden)]
    fn empty_list<'a>() -> Self::ListCursor<'a>;
    #[doc(hidden)]
    fn empty_iter() -> Self::IterCursor;

    #[doc(hidden)]
    unsafe fn compound_is_empty(cursor: Self::CompoundCursor<'_>) -> bool;
    #[doc(hidden)]
    unsafe fn compound_to_iter(cursor: Self::CompoundCursor<'_>) -> Self::IterCursor;
    #[doc(hidden)]
    unsafe fn compound_get<'a>(
        cursor: Self::CompoundCursor<'a>,
        key: &MUTF8Str,
    ) -> Option<(TagID, Self::IterCursor)>;
    #[doc(hidden)]
    unsafe fn compound_next<'a>(iter: &mut Self::IterCursor) -> Option<(&'a MUTF8Str, TagID)>;

    #[doc(hidden)]
    unsafe fn list_header(cursor: Self::ListCursor<'_>) -> (TagID, usize);
    #[doc(hidden)]
    unsafe fn list_primitive_ptr(cursor: Self::ListCursor<'_>) -> *const u8;
    #[doc(hidden)]
    unsafe fn list_to_iter(cursor: Self::ListCursor<'_>) -> Self::IterCursor;
    #[doc(hidden)]
    unsafe fn list_seek_primitive(
        cursor: Self::ListCursor<'_>,
        index: usize,
        elem_size: usize,
    ) -> Self::IterCursor;
    #[doc(hidden)]
    unsafe fn list_seek_array(
        cursor: Self::ListCursor<'_>,
        index: usize,
        elem_size: usize,
    ) -> Self::IterCursor;
    #[doc(hidden)]
    unsafe fn list_seek_string(cursor: Self::ListCursor<'_>, index: usize) -> Self::IterCursor;
    #[doc(hidden)]
    unsafe fn list_seek_composite(cursor: Self::ListCursor<'_>, index: usize) -> Self::IterCursor;
    #[doc(hidden)]
    unsafe fn list_seek_tag(
        cursor: Self::ListCursor<'_>,
        tag_id: TagID,
        index: usize,
    ) -> Self::IterCursor;

    #[doc(hidden)]
    unsafe fn read_primitive_advance(iter: &mut Self::IterCursor, size: usize) -> *const u8;
    #[doc(hidden)]
    unsafe fn read_byte_array_advance<'a>(iter: &mut Self::IterCursor) -> &'a [i8];
    #[doc(hidden)]
    unsafe fn read_string_advance<'a>(iter: &mut Self::IterCursor) -> &'a MUTF8Str;
    #[doc(hidden)]
    unsafe fn read_list_advance<'a>(iter: &mut Self::IterCursor) -> ListRef<'a, Self>;
    #[doc(hidden)]
    unsafe fn read_compound_advance<'a>(iter: &mut Self::IterCursor) -> CompoundRef<'a, Self>;
    #[doc(hidden)]
    unsafe fn read_int_array_advance<'a>(
        iter: &mut Self::IterCursor,
    ) -> &'a [byteorder::I32<Self::Order>];
    #[doc(hidden)]
    unsafe fn read_long_array_advance<'a>(
        iter: &mut Self::IterCursor,
    ) -> &'a [byteorder::I64<Self::Order>];

    #[doc(hidden)]
    unsafe fn compound_to_owned<TARGET: ByteOrder>(
        cursor: Self::CompoundCursor<'_>,
    ) -> Compound<TARGET>;
    #[doc(hidden)]
    unsafe fn list_to_owned<TARGET: ByteOrder>(cursor: Self::ListCursor<'_>) -> List<TARGET>;

    #[doc(hidden)]
    unsafe fn compound_size_hint(cursor: Self::CompoundCursor<'_>) -> usize;
    #[doc(hidden)]
    unsafe fn list_size_hint(cursor: Self::ListCursor<'_>) -> usize;
    #[doc(hidden)]
    unsafe fn write_compound_with_header<TARGET: ByteOrder, Sink: ByteSink>(
        hdr: &[u8],
        cursor: Self::CompoundCursor<'_>,
        sink: &mut Sink,
    ) -> core::result::Result<(), Sink::Error>;
    #[doc(hidden)]
    unsafe fn write_list_with_header<TARGET: ByteOrder, Sink: ByteSink>(
        hdr: &[u8],
        cursor: Self::ListCursor<'_>,
        sink: &mut Sink,
    ) -> core::result::Result<(), Sink::Error>;
}

/// Compile-time NBT tag mapping implemented directly for Rust types (`i8`, `i16`, `i32`, `i64`,
/// `f32`, `f64`, `[i8]`, `str` / [`MUTF8Str`], `[i32]`, `[i64]`, [`Compound`], [`List`], and
/// [`TypedList<T>`]).
pub trait Tag: private::SealedTag + 'static {
    /// Wire [`TagID`] corresponding to this Rust type.
    const TAG_ID: TagID;
    /// Read-only view type returned by `get_as::<T>`.
    type Ref<'a, S: Source>: Copy;
    /// Mutable guard/reference type returned by `get_mut_as::<T>`.
    type Mut<'a, O: ByteOrder>;
    /// Owned value type returned by `remove_as::<T>` / `into_::<T>`.
    type Own<O: ByteOrder>: Default;

    #[doc(hidden)]
    unsafe fn read_ref<'a, S: Source>(iter: &mut S::IterCursor) -> Option<Self::Ref<'a, S>>;
    #[doc(hidden)]
    fn from_value_ref<'a, S: Source>(val: ValueRef<'a, S>) -> Option<Self::Ref<'a, S>>;
    #[doc(hidden)]
    unsafe fn read_mut<'a, O: ByteOrder>(slot: *mut u8) -> Option<Self::Mut<'a, O>>;
    #[doc(hidden)]
    fn into_value_mut<'a, O: ByteOrder>(val: ValueMut<'a, O>) -> Option<Self::Mut<'a, O>>;
    #[doc(hidden)]
    fn into_value_own<O: ByteOrder>(val: Value<O>) -> Option<Self::Own<O>>;
    #[doc(hidden)]
    unsafe fn can_take_from_slot<O: ByteOrder>(slot: *const u8) -> bool;
    #[doc(hidden)]
    unsafe fn take_own_from_slot<O: ByteOrder>(slot: *mut u8) -> Self::Own<O>;
}

/// Canonical NBT list/compound element tag (`()`, `i8`, `i16`, `i32`, `i64`, `f32`, `f64`,
/// `[i8]`, `str`, `[i32]`, `[i64]`, [`List`], [`Compound`]).
pub trait ElementTag: Tag {
    #[doc(hidden)]
    const INLINE_SIZE: usize;

    #[doc(hidden)]
    unsafe fn read_elem_ref<'a, S: Source>(iter: &mut S::IterCursor) -> Self::Ref<'a, S>;
    #[doc(hidden)]
    unsafe fn seek_list_elem<'a, S: Source>(
        cursor: S::ListCursor<'a>,
        index: usize,
    ) -> S::IterCursor;
    #[doc(hidden)]
    unsafe fn read_elem_mut<'a, O: ByteOrder>(slot: *mut u8) -> Self::Mut<'a, O>;
    #[doc(hidden)]
    unsafe fn write_own_to_slot<O: ByteOrder>(dst: *mut u8, val: Self::Own<O>);
    #[doc(hidden)]
    unsafe fn read_own_from_slot<O: ByteOrder>(src: *const u8) -> Self::Own<O>;
}

impl private::SealedTag for () {}
impl Tag for () {
    const TAG_ID: TagID = TagID::End;
    type Ref<'a, S: Source> = ();
    type Mut<'a, O: ByteOrder> = ();
    type Own<O: ByteOrder> = ();

    #[inline(always)]
    unsafe fn read_ref<'a, S: Source>(_iter: &mut S::IterCursor) -> Option<Self::Ref<'a, S>> {
        Some(())
    }
    #[inline(always)]
    fn from_value_ref<'a, S: Source>(val: ValueRef<'a, S>) -> Option<Self::Ref<'a, S>> {
        matches!(val, ValueRef::End).then_some(())
    }
    #[inline(always)]
    unsafe fn read_mut<'a, O: ByteOrder>(_slot: *mut u8) -> Option<Self::Mut<'a, O>> {
        Some(())
    }
    #[inline(always)]
    fn into_value_mut<'a, O: ByteOrder>(val: ValueMut<'a, O>) -> Option<Self::Mut<'a, O>> {
        matches!(val, ValueMut::End).then_some(())
    }
    #[inline(always)]
    fn into_value_own<O: ByteOrder>(val: Value<O>) -> Option<()> {
        matches!(val, Value::End).then_some(())
    }
    #[inline(always)]
    unsafe fn can_take_from_slot<O: ByteOrder>(_slot: *const u8) -> bool {
        true
    }
    #[inline(always)]
    unsafe fn take_own_from_slot<O: ByteOrder>(_slot: *mut u8) {}
}

impl ElementTag for () {
    const INLINE_SIZE: usize = 0;

    #[inline(always)]
    unsafe fn read_elem_ref<'a, S: Source>(_iter: &mut S::IterCursor) -> Self::Ref<'a, S> {}
    #[inline(always)]
    unsafe fn seek_list_elem<'a, S: Source>(
        cursor: S::ListCursor<'a>,
        index: usize,
    ) -> S::IterCursor {
        unsafe { S::list_seek_primitive(cursor, index, 0) }
    }
    #[inline(always)]
    unsafe fn read_elem_mut<'a, O: ByteOrder>(_slot: *mut u8) -> Self::Mut<'a, O> {}
    #[inline(always)]
    unsafe fn write_own_to_slot<O: ByteOrder>(_dst: *mut u8, _val: ()) {}
    #[inline(always)]
    unsafe fn read_own_from_slot<O: ByteOrder>(_src: *const u8) {}
}

impl private::SealedTag for i8 {}
impl Tag for i8 {
    const TAG_ID: TagID = TagID::Byte;
    type Ref<'a, S: Source> = i8;
    type Mut<'a, O: ByteOrder> = &'a mut i8;
    type Own<O: ByteOrder> = i8;

    #[inline(always)]
    unsafe fn read_ref<'a, S: Source>(iter: &mut S::IterCursor) -> Option<Self::Ref<'a, S>> {
        Some(unsafe { Self::read_elem_ref::<S>(iter) })
    }
    #[inline(always)]
    fn from_value_ref<'a, S: Source>(val: ValueRef<'a, S>) -> Option<Self::Ref<'a, S>> {
        match val {
            ValueRef::Byte(v) => Some(v),
            _ => None,
        }
    }
    #[inline(always)]
    unsafe fn read_mut<'a, O: ByteOrder>(slot: *mut u8) -> Option<&'a mut i8> {
        Some(unsafe { Self::read_elem_mut::<O>(slot) })
    }
    #[inline(always)]
    fn into_value_mut<'a, O: ByteOrder>(val: ValueMut<'a, O>) -> Option<&'a mut i8> {
        match val {
            ValueMut::Byte(v) => Some(v),
            _ => None,
        }
    }
    #[inline(always)]
    fn into_value_own<O: ByteOrder>(val: Value<O>) -> Option<i8> {
        match val {
            Value::Byte(v) => Some(v),
            _ => None,
        }
    }
    #[inline(always)]
    unsafe fn can_take_from_slot<O: ByteOrder>(_slot: *const u8) -> bool {
        true
    }
    #[inline(always)]
    unsafe fn take_own_from_slot<O: ByteOrder>(slot: *mut u8) -> i8 {
        unsafe { Self::read_own_from_slot::<O>(slot) }
    }
}

impl ElementTag for i8 {
    const INLINE_SIZE: usize = 1;

    #[inline(always)]
    unsafe fn read_elem_ref<'a, S: Source>(iter: &mut S::IterCursor) -> Self::Ref<'a, S> {
        unsafe { *S::read_primitive_advance(iter, 1).cast() }
    }
    #[inline(always)]
    unsafe fn seek_list_elem<'a, S: Source>(
        cursor: S::ListCursor<'a>,
        index: usize,
    ) -> S::IterCursor {
        unsafe { S::list_seek_primitive(cursor, index, 1) }
    }
    #[inline(always)]
    unsafe fn read_elem_mut<'a, O: ByteOrder>(slot: *mut u8) -> &'a mut i8 {
        unsafe { &mut *slot.cast() }
    }
    #[inline(always)]
    unsafe fn write_own_to_slot<O: ByteOrder>(dst: *mut u8, val: i8) {
        unsafe { dst.write(val as u8) };
    }
    #[inline(always)]
    unsafe fn read_own_from_slot<O: ByteOrder>(src: *const u8) -> i8 {
        unsafe { *src.cast() }
    }
}

macro_rules! impl_scalar_tag {
    ($prim:ty, $variant:ident, $wrapper:ident, $size:literal) => {
        impl private::SealedTag for $prim {}
        impl Tag for $prim {
            const TAG_ID: TagID = TagID::$variant;
            type Ref<'a, S: Source> = $prim;
            type Mut<'a, O: ByteOrder> = ScalarMut<'a, $prim>;
            type Own<O: ByteOrder> = $prim;

            #[inline(always)]
            unsafe fn read_ref<'a, S: Source>(
                iter: &mut S::IterCursor,
            ) -> Option<Self::Ref<'a, S>> {
                Some(unsafe { Self::read_elem_ref::<S>(iter) })
            }
            #[inline(always)]
            fn from_value_ref<'a, S: Source>(val: ValueRef<'a, S>) -> Option<Self::Ref<'a, S>> {
                match val {
                    ValueRef::$variant(v) => Some(v),
                    _ => None,
                }
            }
            #[inline(always)]
            unsafe fn read_mut<'a, O: ByteOrder>(slot: *mut u8) -> Option<ScalarMut<'a, $prim>> {
                Some(unsafe { Self::read_elem_mut::<O>(slot) })
            }
            #[inline(always)]
            fn into_value_mut<'a, O: ByteOrder>(
                val: ValueMut<'a, O>,
            ) -> Option<ScalarMut<'a, $prim>> {
                match val {
                    ValueMut::$variant(v) => Some(v),
                    _ => None,
                }
            }
            #[inline(always)]
            fn into_value_own<O: ByteOrder>(val: Value<O>) -> Option<$prim> {
                match val {
                    Value::$variant(v) => Some(v),
                    _ => None,
                }
            }
            #[inline(always)]
            unsafe fn can_take_from_slot<O: ByteOrder>(_slot: *const u8) -> bool {
                true
            }
            #[inline(always)]
            unsafe fn take_own_from_slot<O: ByteOrder>(slot: *mut u8) -> $prim {
                unsafe { Self::read_own_from_slot::<O>(slot) }
            }
        }

        impl ElementTag for $prim {
            const INLINE_SIZE: usize = $size;

            #[inline(always)]
            unsafe fn read_elem_ref<'a, S: Source>(iter: &mut S::IterCursor) -> Self::Ref<'a, S> {
                let p = unsafe { S::read_primitive_advance(iter, $size) };
                byteorder::$wrapper::<S::Order>::from_bytes(unsafe { *p.cast() }).get()
            }
            #[inline(always)]
            unsafe fn seek_list_elem<'a, S: Source>(
                cursor: S::ListCursor<'a>,
                index: usize,
            ) -> S::IterCursor {
                unsafe { S::list_seek_primitive(cursor, index, $size) }
            }
            #[inline(always)]
            unsafe fn read_elem_mut<'a, O: ByteOrder>(slot: *mut u8) -> ScalarMut<'a, $prim> {
                unsafe { ScalarMut::from_endian::<O>(slot) }
            }
            #[inline(always)]
            unsafe fn write_own_to_slot<O: ByteOrder>(dst: *mut u8, val: $prim) {
                unsafe {
                    ptr::write_unaligned(
                        dst.cast::<[u8; $size]>(),
                        byteorder::$wrapper::<O>::new(val).to_bytes(),
                    );
                }
            }
            #[inline(always)]
            unsafe fn read_own_from_slot<O: ByteOrder>(src: *const u8) -> $prim {
                byteorder::$wrapper::<O>::from_bytes(unsafe { *src.cast() }).get()
            }
        }
    };
}

impl_scalar_tag!(i16, Short, I16, 2);
impl_scalar_tag!(i32, Int, I32, 4);
impl_scalar_tag!(i64, Long, I64, 8);
impl_scalar_tag!(f32, Float, F32, 4);
impl_scalar_tag!(f64, Double, F64, 8);

macro_rules! impl_dyn_tag {
    (
        [$($generics:tt)*]
        $tag_ty:ty,
        $variant:ident,
        ref($lt:lifetime, $src:ident): $ref_ty:ty,
        own($ord:ident): $own_ty:ty,
        read_ref($iter:ident): $read_ref_expr:expr,
        seek_list($cur:ident, $idx:ident): $seek_expr:expr $(,)?
    ) => {
        impl<$($generics)*> private::SealedTag for $tag_ty {}
        impl<$($generics)*> Tag for $tag_ty {
            const TAG_ID: TagID = TagID::$variant;
            type Ref<$lt, $src: Source> = $ref_ty;
            type Mut<'a, $ord: ByteOrder> = Mut<'a, $own_ty>;
            type Own<$ord: ByteOrder> = $own_ty;

            #[inline(always)]
            unsafe fn read_ref<$lt, $src: Source>($iter: &mut $src::IterCursor) -> Option<$ref_ty> {
                Some(unsafe { $read_ref_expr })
            }
            #[inline(always)]
            fn from_value_ref<$lt, $src: Source>(val: ValueRef<$lt, $src>) -> Option<$ref_ty> {
                match val {
                    ValueRef::$variant(v) => Some(v),
                    _ => None,
                }
            }
            #[inline(always)]
            unsafe fn read_mut<'a, $ord: ByteOrder>(slot: *mut u8) -> Option<Mut<'a, $own_ty>> {
                Some(unsafe { Mut::from_slot(slot) })
            }
            #[inline(always)]
            fn into_value_mut<'a, $ord: ByteOrder>(
                val: ValueMut<'a, $ord>,
            ) -> Option<Mut<'a, $own_ty>> {
                match val {
                    ValueMut::$variant(v) => Some(v),
                    _ => None,
                }
            }
            #[inline(always)]
            fn into_value_own<$ord: ByteOrder>(val: Value<$ord>) -> Option<$own_ty> {
                match val {
                    Value::$variant(v) => Some(v),
                    _ => None,
                }
            }
            #[inline(always)]
            unsafe fn can_take_from_slot<$ord: ByteOrder>(_slot: *const u8) -> bool {
                true
            }
            #[inline(always)]
            unsafe fn take_own_from_slot<$ord: ByteOrder>(slot: *mut u8) -> $own_ty {
                unsafe { slot_read(slot) }
            }
        }

        impl<$($generics)*> ElementTag for $tag_ty {
            const INLINE_SIZE: usize = SIZE_DYN;

            #[inline(always)]
            unsafe fn read_elem_ref<$lt, $src: Source>($iter: &mut $src::IterCursor) -> $ref_ty {
                unsafe { $read_ref_expr }
            }
            #[inline(always)]
            unsafe fn seek_list_elem<'a, $src: Source>(
                $cur: $src::ListCursor<'a>,
                $idx: usize,
            ) -> $src::IterCursor {
                unsafe { $seek_expr }
            }
            #[inline(always)]
            unsafe fn read_elem_mut<'a, $ord: ByteOrder>(slot: *mut u8) -> Mut<'a, $own_ty> {
                unsafe { Mut::from_slot(slot) }
            }
            #[inline(always)]
            unsafe fn write_own_to_slot<$ord: ByteOrder>(dst: *mut u8, val: $own_ty) {
                unsafe { slot_write(dst, val) };
            }
            #[inline(always)]
            unsafe fn read_own_from_slot<$ord: ByteOrder>(src: *const u8) -> $own_ty {
                unsafe { slot_read(src) }
            }
        }
    };
}

impl_dyn_tag!(
    []
    [i8],
    ByteArray,
    ref('a, S): &'a [i8],
    own(O): Vec<i8>,
    read_ref(iter): S::read_byte_array_advance(iter),
    seek_list(cur, idx): S::list_seek_array(cur, idx, 1),
);

impl_dyn_tag!(
    []
    str,
    String,
    ref('a, S): &'a MUTF8Str,
    own(O): MUTF8String,
    read_ref(iter): S::read_string_advance(iter),
    seek_list(cur, idx): S::list_seek_string(cur, idx),
);

impl_dyn_tag!(
    [AnyO: ByteOrder]
    List<AnyO>,
    List,
    ref('a, S): ListRef<'a, S>,
    own(O): List<O>,
    read_ref(iter): S::read_list_advance(iter),
    seek_list(cur, idx): S::list_seek_composite(cur, idx),
);

impl_dyn_tag!(
    [AnyO: ByteOrder]
    Compound<AnyO>,
    Compound,
    ref('a, S): CompoundRef<'a, S>,
    own(O): Compound<O>,
    read_ref(iter): S::read_compound_advance(iter),
    seek_list(cur, idx): S::list_seek_composite(cur, idx),
);

impl_dyn_tag!(
    []
    [i32],
    IntArray,
    ref('a, S): &'a [byteorder::I32<S::Order>],
    own(O): Vec<byteorder::I32<O>>,
    read_ref(iter): S::read_int_array_advance(iter),
    seek_list(cur, idx): S::list_seek_array(cur, idx, 4),
);

impl_dyn_tag!(
    []
    [i64],
    LongArray,
    ref('a, S): &'a [byteorder::I64<S::Order>],
    own(O): Vec<byteorder::I64<O>>,
    read_ref(iter): S::read_long_array_advance(iter),
    seek_list(cur, idx): S::list_seek_array(cur, idx, 8),
);

// Convenience `Tag` aliases (`MUTF8Str`, `MUTF8String`, `String`, `Vec<i8>`, endian array slices/vecs)
// delegating to the canonical `ElementTag` without affecting `IntoNBT` type inference.
macro_rules! impl_tag_alias {
    ([$($generics:tt)*] $alias:ty => $canonical:ty) => {
        impl<$($generics)*> private::SealedTag for $alias {}
        impl<$($generics)*> Tag for $alias {
            const TAG_ID: TagID = <$canonical as Tag>::TAG_ID;
            type Ref<'a, S: Source> = <$canonical as Tag>::Ref<'a, S>;
            type Mut<'a, O: ByteOrder> = <$canonical as Tag>::Mut<'a, O>;
            type Own<O: ByteOrder> = <$canonical as Tag>::Own<O>;

            #[inline(always)]
            unsafe fn read_ref<'a, S: Source>(
                iter: &mut S::IterCursor,
            ) -> Option<Self::Ref<'a, S>> {
                unsafe { <$canonical as Tag>::read_ref::<S>(iter) }
            }
            #[inline(always)]
            fn from_value_ref<'a, S: Source>(val: ValueRef<'a, S>) -> Option<Self::Ref<'a, S>> {
                <$canonical as Tag>::from_value_ref::<S>(val)
            }
            #[inline(always)]
            unsafe fn read_mut<'a, O: ByteOrder>(slot: *mut u8) -> Option<Self::Mut<'a, O>> {
                unsafe { <$canonical as Tag>::read_mut::<O>(slot) }
            }
            #[inline(always)]
            fn into_value_mut<'a, O: ByteOrder>(val: ValueMut<'a, O>) -> Option<Self::Mut<'a, O>> {
                <$canonical as Tag>::into_value_mut::<O>(val)
            }
            #[inline(always)]
            fn into_value_own<O: ByteOrder>(val: Value<O>) -> Option<Self::Own<O>> {
                <$canonical as Tag>::into_value_own::<O>(val)
            }
            #[inline(always)]
            unsafe fn can_take_from_slot<O: ByteOrder>(slot: *const u8) -> bool {
                unsafe { <$canonical as Tag>::can_take_from_slot::<O>(slot) }
            }
            #[inline(always)]
            unsafe fn take_own_from_slot<O: ByteOrder>(slot: *mut u8) -> Self::Own<O> {
                unsafe { <$canonical as Tag>::take_own_from_slot::<O>(slot) }
            }
        }
    };
}

impl_tag_alias!([] MUTF8Str => str);
impl_tag_alias!([] MUTF8String => str);
impl_tag_alias!([] String => str);
impl_tag_alias!([] Vec<i8> => [i8]);
impl_tag_alias!([AnyO: ByteOrder] [byteorder::I32<AnyO>] => [i32]);
impl_tag_alias!([AnyO: ByteOrder] Vec<byteorder::I32<AnyO>> => [i32]);
impl_tag_alias!([] Vec<i32> => [i32]);
impl_tag_alias!([AnyO: ByteOrder] [byteorder::I64<AnyO>] => [i64]);
impl_tag_alias!([AnyO: ByteOrder] Vec<byteorder::I64<AnyO>> => [i64]);
impl_tag_alias!([] Vec<i64> => [i64]);

#[inline(always)]
unsafe fn slot_list_matches_elem<O: ByteOrder, T: ?Sized + ElementTag>(slot: *const u8) -> bool {
    let hdr = unsafe { slot_list_ptr(slot) };
    let elem_tag = unsafe { *hdr.cast::<TagID>() };
    if elem_tag == T::TAG_ID {
        return true;
    }
    let len = byteorder::U32::<O>::from_bytes(unsafe { *hdr.add(1).cast() }).get();
    len == 0
}

impl<T: ?Sized + ElementTag, AnyO: ByteOrder> private::SealedTag for TypedList<T, AnyO> {}
impl<T: ?Sized + ElementTag, AnyO: ByteOrder> Tag for TypedList<T, AnyO> {
    const TAG_ID: TagID = TagID::List;
    type Ref<'a, S: Source> = TypedListRef<'a, T, S>;
    type Mut<'a, O: ByteOrder> = Mut<'a, TypedList<T, O>>;
    type Own<O: ByteOrder> = TypedList<T, O>;

    #[inline(always)]
    unsafe fn read_ref<'a, S: Source>(iter: &mut S::IterCursor) -> Option<Self::Ref<'a, S>> {
        unsafe { S::read_list_advance(iter) }.typed_as::<T>()
    }

    #[inline(always)]
    fn from_value_ref<'a, S: Source>(val: ValueRef<'a, S>) -> Option<Self::Ref<'a, S>> {
        match val {
            ValueRef::List(list) => list.typed_as::<T>(),
            _ => None,
        }
    }

    #[inline(always)]
    unsafe fn read_mut<'a, O: ByteOrder>(slot: *mut u8) -> Option<Self::Mut<'a, O>> {
        if unsafe { slot_list_matches_elem::<O, T>(slot) } {
            let mut m: Mut<'a, TypedList<T, O>> = unsafe { Mut::from_slot(slot) };
            m.fix_header_tag();
            Some(m)
        } else {
            cold_path();
            None
        }
    }

    #[inline(always)]
    fn into_value_mut<'a, O: ByteOrder>(val: ValueMut<'a, O>) -> Option<Self::Mut<'a, O>> {
        match val {
            ValueMut::List(list_mut) => list_mut.into_typed::<T>().ok(),
            _ => None,
        }
    }

    #[inline(always)]
    fn into_value_own<O: ByteOrder>(val: Value<O>) -> Option<Self::Own<O>> {
        match val {
            Value::List(list) => list.into_typed::<T>(),
            _ => None,
        }
    }

    #[inline(always)]
    unsafe fn can_take_from_slot<O: ByteOrder>(slot: *const u8) -> bool {
        unsafe { slot_list_matches_elem::<O, T>(slot) }
    }

    #[inline(always)]
    unsafe fn take_own_from_slot<O: ByteOrder>(slot: *mut u8) -> Self::Own<O> {
        let mut tl: TypedList<T, O> = unsafe { slot_read(slot) };
        tl.fix_header_tag();
        tl
    }
}
