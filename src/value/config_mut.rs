use crate::{
    CompoundMut, ConfigRef, GenericNBT, ListMut, MUTF8Str, NBT, TagID, TypedListMut, ValueMut,
    ValueOwn,
};

pub trait ConfigMut: ConfigRef {
    type ValueMut<'doc>: ValueMut<'doc, Config = Self>;
    type ListMut<'doc>: ListMut<'doc, Config = Self>;
    type ListIterMut<'doc>: Iterator<Item = Self::ValueMut<'doc>> + ExactSizeIterator + Default;
    type TypedListMut<'doc, T: NBT>: TypedListMut<'doc, T, Config = Self>;
    type TypedListIterMut<'doc, T: NBT>: Iterator<Item = T::TypeMut<'doc, Self>>
        + ExactSizeIterator
        + Default;
    type CompoundMut<'doc>: CompoundMut<'doc, Config = Self>;
    type CompoundIterMut<'doc>: Iterator<Item = (Self::String<'doc>, Self::ValueMut<'doc>)>
        + Default;

    type WriteParams<'a>: Sized;

    unsafe fn read_list_mut<'a, 'doc>(params: Self::ReadParams<'a>) -> Self::ListMut<'doc>;

    unsafe fn read_compound_mut<'a, 'doc>(params: Self::ReadParams<'a>) -> Self::CompoundMut<'doc>;

    unsafe fn read_value_mut<'a, 'doc>(
        tag_id: TagID,
        params: Self::ReadParams<'a>,
    ) -> Self::ValueMut<'doc>;

    #[inline]
    unsafe fn read_mut<'a, 'doc, T: GenericNBT>(
        params: Self::ReadParams<'a>,
    ) -> Option<T::TypeMut<'doc, Self>> {
        unsafe { T::read_mut_static::<Self>(params) }
    }

    unsafe fn list_push<'a, T: NBT>(params: Self::WriteParams<'a>, value: T::Type<Self::ByteOrder>);

    unsafe fn list_pop<'a, T: NBT>(params: Self::WriteParams<'a>) -> T::Type<Self::ByteOrder>;

    unsafe fn list_pop_value<'a>(
        tag_id: TagID,
        params: Self::WriteParams<'a>,
    ) -> ValueOwn<Self::ByteOrder>;

    unsafe fn list_insert<'a, T: NBT>(
        params: Self::WriteParams<'a>,
        index: usize,
        value: T::Type<Self::ByteOrder>,
    );

    unsafe fn list_remove<'a, T: NBT>(
        params: Self::WriteParams<'a>,
        index: usize,
    ) -> T::Type<Self::ByteOrder>;

    unsafe fn list_remove_value<'a>(
        tag_id: TagID,
        params: Self::WriteParams<'a>,
        index: usize,
    ) -> ValueOwn<Self::ByteOrder>;

    unsafe fn compound_insert<'a, T: NBT>(
        params: Self::WriteParams<'a>,
        key: &MUTF8Str,
        value: T::Type<Self::ByteOrder>,
    );

    unsafe fn compound_remove<'a>(
        params: Self::WriteParams<'a>,
        key: &MUTF8Str,
    ) -> Option<ValueOwn<Self::ByteOrder>>;
}
