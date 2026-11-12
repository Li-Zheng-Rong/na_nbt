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

    type ReadParams<'a>: Sized;

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
        T::ref_into_(unsafe { Self::read_value(T::TAG_ID, params) })
    }
}
