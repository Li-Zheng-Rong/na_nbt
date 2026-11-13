use std::{hint::assert_unchecked, io::Write, marker::PhantomData, ptr};

use zerocopy::byteorder;

use crate::{
    cold_path,
    owned::slot::{slot_list_ptr, slot_read, slot_write, SIZE_DYN},
    ByteOrder, CompoundOwn, CompoundRef, Error, ListOwn, ListRef, MUTF8Str, MUTF8String, Mut,
    Result, ScalarMut, TypedListOwn, TypedListRef, ValueMut, ValueOwn, ValueRef,
};

const TAG_ELEM_SIZES: [u8; 13] = [0, 1, 2, 4, 8, 4, 8, 1, 0, 0, 0, 4, 8];

/// NBT tag type identifier (`0..=12`).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum TagID {
    End = 0,
    Byte = 1,
    Short = 2,
    Int = 3,
    Long = 4,
    Float = 5,
    Double = 6,
    ByteArray = 7,
    String = 8,
    List = 9,
    Compound = 10,
    IntArray = 11,
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
pub struct Borrowed<O: ByteOrder>(PhantomData<fn() -> O>);

impl<O: ByteOrder> private::SealedSource for Borrowed<O> {}
impl<O: ByteOrder> Copy for Borrowed<O> {}
impl<O: ByteOrder> Clone for Borrowed<O> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

/// Owned DOM source marker for [`ValueRef`], [`CompoundRef`], and [`ListRef`].
pub struct Owned<O: ByteOrder>(PhantomData<fn() -> O>);

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
    fn write_compound_to_vec<TARGET: ByteOrder>(cursor: Self::CompoundCursor<'_>) -> Vec<u8>;
    #[doc(hidden)]
    fn write_compound_to_writer<TARGET: ByteOrder>(
        cursor: Self::CompoundCursor<'_>,
        writer: impl Write,
    ) -> Result<()>;
    #[doc(hidden)]
    fn write_list_to_vec<TARGET: ByteOrder>(cursor: Self::ListCursor<'_>) -> Vec<u8>;
    #[doc(hidden)]
    fn write_list_to_writer<TARGET: ByteOrder>(
        cursor: Self::ListCursor<'_>,
        writer: impl Write,
    ) -> Result<()>;
}

/// Compile-time marker types for NBT tags.
pub mod tag {
    use std::marker::PhantomData;

    use super::ElementTag;

    #[derive(Clone, Copy, Debug)]
    pub struct End;
    #[derive(Clone, Copy, Debug)]
    pub struct Byte;
    #[derive(Clone, Copy, Debug)]
    pub struct Short;
    #[derive(Clone, Copy, Debug)]
    pub struct Int;
    #[derive(Clone, Copy, Debug)]
    pub struct Long;
    #[derive(Clone, Copy, Debug)]
    pub struct Float;
    #[derive(Clone, Copy, Debug)]
    pub struct Double;
    #[derive(Clone, Copy, Debug)]
    pub struct ByteArray;
    #[derive(Clone, Copy, Debug)]
    pub struct String;
    #[derive(Clone, Copy, Debug)]
    pub struct List;
    #[derive(Clone, Copy, Debug)]
    pub struct Compound;
    #[derive(Clone, Copy, Debug)]
    pub struct IntArray;
    #[derive(Clone, Copy, Debug)]
    pub struct LongArray;

    #[derive(Debug)]
    pub struct TypedList<T: ElementTag>(PhantomData<fn() -> T>);

    impl<T: ElementTag> Copy for TypedList<T> {}
    impl<T: ElementTag> Clone for TypedList<T> {
        #[inline(always)]
        fn clone(&self) -> Self {
            *self
        }
    }
}

/// Compile-time NBT tag descriptor (all 13 concrete tags plus [`tag::TypedList<T>`]).
pub trait Tag: private::SealedTag + Copy + Clone + Send + Sync + 'static {
    const TAG_ID: TagID;
    type Ref<'a, S: Source>: Copy;
    type Mut<'a, O: ByteOrder>;
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
    fn into_value_own<O: ByteOrder>(val: ValueOwn<O>) -> Option<Self::Own<O>>;
    #[doc(hidden)]
    unsafe fn can_take_from_slot<O: ByteOrder>(slot: *const u8) -> bool;
    #[doc(hidden)]
    unsafe fn take_own_from_slot<O: ByteOrder>(slot: *mut u8) -> Self::Own<O>;
}

/// Concrete NBT element tag (`End` through `LongArray`).
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

impl private::SealedTag for tag::End {}
impl Tag for tag::End {
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
    fn into_value_own<O: ByteOrder>(val: ValueOwn<O>) -> Option<()> {
        matches!(val, ValueOwn::End).then_some(())
    }
    #[inline(always)]
    unsafe fn can_take_from_slot<O: ByteOrder>(_slot: *const u8) -> bool {
        true
    }
    #[inline(always)]
    unsafe fn take_own_from_slot<O: ByteOrder>(_slot: *mut u8) {}
}

impl ElementTag for tag::End {
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

impl private::SealedTag for tag::Byte {}
impl Tag for tag::Byte {
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
    fn into_value_own<O: ByteOrder>(val: ValueOwn<O>) -> Option<i8> {
        match val {
            ValueOwn::Byte(v) => Some(v),
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

impl ElementTag for tag::Byte {
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
    ($tag_ty:ty, $variant:ident, $prim:ty, $wrapper:ident, $size:literal) => {
        impl private::SealedTag for $tag_ty {}
        impl Tag for $tag_ty {
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
            fn into_value_own<O: ByteOrder>(val: ValueOwn<O>) -> Option<$prim> {
                match val {
                    ValueOwn::$variant(v) => Some(v),
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

        impl ElementTag for $tag_ty {
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

impl_scalar_tag!(tag::Short, Short, i16, I16, 2);
impl_scalar_tag!(tag::Int, Int, i32, I32, 4);
impl_scalar_tag!(tag::Long, Long, i64, I64, 8);
impl_scalar_tag!(tag::Float, Float, f32, F32, 4);
impl_scalar_tag!(tag::Double, Double, f64, F64, 8);

macro_rules! impl_dyn_tag {
    (
        $tag_ty:ty,
        $variant:ident,
        ref($lt:lifetime, $src:ident): $ref_ty:ty,
        own($ord:ident): $own_ty:ty,
        read_ref($iter:ident): $read_ref_expr:expr,
        seek_list($cur:ident, $idx:ident): $seek_expr:expr $(,)?
    ) => {
        impl private::SealedTag for $tag_ty {}
        impl Tag for $tag_ty {
            const TAG_ID: TagID = TagID::$variant;
            type Ref<$lt, $src: Source> = $ref_ty;
            type Mut<'a, $ord: ByteOrder> = Mut<'a, $own_ty>;
            type Own<$ord: ByteOrder> = $own_ty;

            #[inline(always)]
            unsafe fn read_ref<$lt, $src: Source>(iter: &mut $src::IterCursor) -> Option<$ref_ty> {
                Some(unsafe { Self::read_elem_ref::<$src>(iter) })
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
                Some(unsafe { Self::read_elem_mut::<$ord>(slot) })
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
            fn into_value_own<$ord: ByteOrder>(val: ValueOwn<$ord>) -> Option<$own_ty> {
                match val {
                    ValueOwn::$variant(v) => Some(v),
                    _ => None,
                }
            }
            #[inline(always)]
            unsafe fn can_take_from_slot<$ord: ByteOrder>(_slot: *const u8) -> bool {
                true
            }
            #[inline(always)]
            unsafe fn take_own_from_slot<$ord: ByteOrder>(slot: *mut u8) -> $own_ty {
                unsafe { Self::read_own_from_slot::<$ord>(slot) }
            }
        }

        impl ElementTag for $tag_ty {
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
    tag::ByteArray,
    ByteArray,
    ref('a, S): &'a [i8],
    own(O): Vec<i8>,
    read_ref(iter): S::read_byte_array_advance(iter),
    seek_list(cur, idx): S::list_seek_array(cur, idx, 1),
);

impl_dyn_tag!(
    tag::String,
    String,
    ref('a, S): &'a MUTF8Str,
    own(O): MUTF8String,
    read_ref(iter): S::read_string_advance(iter),
    seek_list(cur, idx): S::list_seek_string(cur, idx),
);

impl_dyn_tag!(
    tag::List,
    List,
    ref('a, S): ListRef<'a, S>,
    own(O): ListOwn<O>,
    read_ref(iter): S::read_list_advance(iter),
    seek_list(cur, idx): S::list_seek_composite(cur, idx),
);

impl_dyn_tag!(
    tag::Compound,
    Compound,
    ref('a, S): CompoundRef<'a, S>,
    own(O): CompoundOwn<O>,
    read_ref(iter): S::read_compound_advance(iter),
    seek_list(cur, idx): S::list_seek_composite(cur, idx),
);

impl_dyn_tag!(
    tag::IntArray,
    IntArray,
    ref('a, S): &'a [byteorder::I32<S::Order>],
    own(O): Vec<byteorder::I32<O>>,
    read_ref(iter): S::read_int_array_advance(iter),
    seek_list(cur, idx): S::list_seek_array(cur, idx, 4),
);

impl_dyn_tag!(
    tag::LongArray,
    LongArray,
    ref('a, S): &'a [byteorder::I64<S::Order>],
    own(O): Vec<byteorder::I64<O>>,
    read_ref(iter): S::read_long_array_advance(iter),
    seek_list(cur, idx): S::list_seek_array(cur, idx, 8),
);

#[inline(always)]
unsafe fn slot_list_matches_elem<O: ByteOrder, T: ElementTag>(slot: *const u8) -> bool {
    let hdr = unsafe { slot_list_ptr(slot) };
    let elem_tag = unsafe { *hdr.cast::<TagID>() };
    if elem_tag == T::TAG_ID {
        return true;
    }
    let len = byteorder::U32::<O>::from_bytes(unsafe { *hdr.add(1).cast() }).get();
    len == 0
}

impl<T: ElementTag> private::SealedTag for tag::TypedList<T> {}
impl<T: ElementTag> Tag for tag::TypedList<T> {
    const TAG_ID: TagID = TagID::List;
    type Ref<'a, S: Source> = TypedListRef<'a, S, T>;
    type Mut<'a, O: ByteOrder> = Mut<'a, TypedListOwn<O, T>>;
    type Own<O: ByteOrder> = TypedListOwn<O, T>;

    #[inline(always)]
    unsafe fn read_ref<'a, S: Source>(iter: &mut S::IterCursor) -> Option<Self::Ref<'a, S>> {
        unsafe { S::read_list_advance(iter) }.typed::<T>()
    }

    #[inline(always)]
    fn from_value_ref<'a, S: Source>(val: ValueRef<'a, S>) -> Option<Self::Ref<'a, S>> {
        match val {
            ValueRef::List(list) => list.typed::<T>(),
            _ => None,
        }
    }

    #[inline(always)]
    unsafe fn read_mut<'a, O: ByteOrder>(slot: *mut u8) -> Option<Self::Mut<'a, O>> {
        if unsafe { slot_list_matches_elem::<O, T>(slot) } {
            let mut m: Mut<'a, TypedListOwn<O, T>> = unsafe { Mut::from_slot(slot) };
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
    fn into_value_own<O: ByteOrder>(val: ValueOwn<O>) -> Option<Self::Own<O>> {
        match val {
            ValueOwn::List(list) => list.into_typed::<T>(),
            _ => None,
        }
    }

    #[inline(always)]
    unsafe fn can_take_from_slot<O: ByteOrder>(slot: *const u8) -> bool {
        unsafe { slot_list_matches_elem::<O, T>(slot) }
    }

    #[inline(always)]
    unsafe fn take_own_from_slot<O: ByteOrder>(slot: *mut u8) -> Self::Own<O> {
        let mut tl: TypedListOwn<O, T> = unsafe { slot_read(slot) };
        tl.fix_header_tag();
        tl
    }
}
