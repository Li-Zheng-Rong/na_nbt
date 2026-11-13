use std::ops::Deref;

use zerocopy::byteorder;

use crate::{
    ByteOrder, CompoundRef, GenericNBT, ListRef, MUTF8Str, NBT, TagID, TypedListRef, ValueRef,
};

pub trait ConfigRef: Send + Sync + Sized + Clone + 'static {
    type ByteOrder: ByteOrder;
    type Value<'doc>: ValueRef<'doc, Config = Self>;
    type ByteArray<'doc>: Deref<Target = [i8]> + Clone + Default;
    type String<'doc>: Deref<Target = MUTF8Str> + Clone + Default;
    type List<'doc>: ListRef<'doc, Config = Self>;
    type ListIter<'doc>: Iterator<Item = Self::Value<'doc>> + ExactSizeIterator + Clone + Default;
    type TypedList<'doc, T: NBT>: TypedListRef<'doc, T, Config = Self>;
    type TypedListIter<'doc, T: NBT>: Iterator<Item = T::TypeRef<'doc, Self>>
        + ExactSizeIterator
        + Clone
        + Default;
    type Compound<'doc>: CompoundRef<'doc, Config = Self>;
    type CompoundIter<'doc>: Iterator<Item = (Self::String<'doc>, Self::Value<'doc>)>
        + Clone
        + Default;
    type IntArray<'doc>: Deref<Target = [byteorder::I32<Self::ByteOrder>]> + Clone + Default;
    type LongArray<'doc>: Deref<Target = [byteorder::I64<Self::ByteOrder>]> + Clone + Default;

    type ReadParams<'a>: Copy;

    fn read_ptr<'a>(params: &Self::ReadParams<'a>) -> *const u8;

    unsafe fn read_primitive_advance<'a>(
        params: &mut Self::ReadParams<'a>,
        size: usize,
    ) -> *const u8;

    unsafe fn read_byte_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::ByteArray<'doc>;

    unsafe fn read_string_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::String<'doc>;

    unsafe fn read_list_advance<'a, 'doc>(params: &mut Self::ReadParams<'a>) -> Self::List<'doc>;

    unsafe fn read_compound_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::Compound<'doc>;

    unsafe fn read_int_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::IntArray<'doc>;

    unsafe fn read_long_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::LongArray<'doc>;

    unsafe fn list_get_primitive<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
        elem_size: usize,
    ) -> Self::ReadParams<'a>;

    unsafe fn list_get_array<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
        elem_size: usize,
    ) -> Self::ReadParams<'a>;

    unsafe fn list_get_string<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a>;

    unsafe fn list_get_composite<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a>;

    #[inline]
    unsafe fn list_get_typed<'a, T: GenericNBT>(
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a> {
        unsafe { T::list_get_static::<Self>(params, index) }
    }

    unsafe fn list_get<'a, 'doc>(
        tag_id: TagID,
        value: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a>
    where
        'doc: 'a;

    unsafe fn compound_get<'a, 'doc>(
        value: Self::ReadParams<'a>,
        key: &MUTF8Str,
    ) -> Option<(TagID, Self::ReadParams<'a>)>
    where
        'doc: 'a;

    unsafe fn read_value<'a, 'doc>(
        tag_id: TagID,
        params: Self::ReadParams<'a>,
    ) -> Self::Value<'doc>;

    #[inline]
    unsafe fn read<'a, 'doc, T: GenericNBT>(
        params: Self::ReadParams<'a>,
    ) -> Option<T::TypeRef<'doc, Self>> {
        unsafe { T::read_static::<Self>(params) }
    }
}
