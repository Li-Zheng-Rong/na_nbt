use std::{hint::unreachable_unchecked, marker::PhantomData, mem::ManuallyDrop, ptr};

use zerocopy::byteorder;

use crate::{
    ByteOrder, CompoundOwn, ConfigMut, ConfigRef, GenericNBT, IntoNBT, NBT, NBTBase, StringOwn,
    TagID, TypedListOwn, ValueOwn, VecOwn, cold_path,
    mutable::{
        config::MutableConfig, size::mutable_tag_size, value_mut::MutValue, value_ref::RefValue,
    },
};

#[repr(transparent)]
pub struct ListOwn<O: ByteOrder> {
    pub(crate) data: VecOwn<u8>,
    pub(crate) _marker: PhantomData<O>,
}

impl<O: ByteOrder> Default for ListOwn<O> {
    fn default() -> Self {
        Self {
            data: vec![0, 0, 0, 0, 0].into(),
            _marker: PhantomData,
        }
    }
}

impl<O: ByteOrder> ListOwn<O> {
    #[inline]
    fn _to_read_params<'a>(&'a self) -> <MutableConfig<O> as ConfigRef>::ReadParams<'a> {
        unsafe { self.data.as_ptr().add(1 + 4) }
    }

    #[inline]
    pub fn _to_write_params<'a>(&'a mut self) -> <MutableConfig<O> as ConfigMut>::WriteParams<'a> {
        self.data.to_mut()
    }

    #[inline]
    pub fn element_tag_id(&self) -> TagID {
        unsafe { *self.data.as_ptr().cast() }
    }

    #[inline]
    pub fn element_is_<T: NBT>(&self) -> bool {
        self.element_tag_id() == T::TAG_ID
            || (self.element_tag_id() == TagID::End && self.is_empty())
    }

    #[inline]
    pub fn len(&self) -> usize {
        unsafe { byteorder::U32::<O>::from_bytes(*self.data.as_ptr().add(1).cast()).get() as usize }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn get<'a>(&'a self, index: usize) -> Option<RefValue<'a, O>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        let tag_id = self.element_tag_id();
        unsafe {
            Some(MutableConfig::<O>::read_value(
                tag_id,
                MutableConfig::<O>::list_get(tag_id, self._to_read_params(), index),
            ))
        }
    }

    #[inline]
    pub fn get_<'a, T: GenericNBT>(
        &'a self,
        index: usize,
    ) -> Option<T::TypeRef<'a, MutableConfig<O>>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        if !(self.element_tag_id() == T::TAG_ID
            || (self.element_tag_id() == TagID::End && self.is_empty()))
        {
            cold_path();
            return None;
        }

        unsafe {
            MutableConfig::<O>::read::<T>(MutableConfig::<O>::list_get(
                T::TAG_ID,
                self._to_read_params(),
                index,
            ))
        }
    }

    #[inline]
    pub fn get_mut<'a>(&'a mut self, index: usize) -> Option<MutValue<'a, O>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        let tag_id = self.element_tag_id();
        unsafe {
            Some(MutableConfig::<O>::read_value_mut(
                tag_id,
                MutableConfig::<O>::list_get(tag_id, self._to_read_params(), index),
            ))
        }
    }

    #[inline]
    pub fn get_mut_<'a, T: GenericNBT>(
        &'a mut self,
        index: usize,
    ) -> Option<T::TypeMut<'a, MutableConfig<O>>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        if !(self.element_tag_id() == T::TAG_ID
            || (self.element_tag_id() == TagID::End && self.is_empty()))
        {
            cold_path();
            return None;
        }

        unsafe {
            MutableConfig::<O>::read_mut::<T>(MutableConfig::<O>::list_get(
                T::TAG_ID,
                self._to_read_params(),
                index,
            ))
        }
    }

    #[inline]
    pub fn push<V: IntoNBT<O>>(&mut self, value: V) {
        if self.element_tag_id() != <V::Tag>::TAG_ID {
            cold_path();
            if self.element_tag_id() == TagID::End && self.is_empty() {
                *unsafe { self.data.get_unchecked_mut(0) } = <V::Tag>::TAG_ID as u8;
            } else {
                cold_path();
                return;
            }
        }

        unsafe {
            MutableConfig::<O>::list_push::<V::Tag>(self._to_write_params(), value.into_nbt())
        }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<ValueOwn<O>> {
        if self.is_empty() {
            cold_path();
            return None;
        }

        unsafe {
            Some(MutableConfig::<O>::list_pop_value(
                self.element_tag_id(),
                self._to_write_params(),
            ))
        }
    }

    #[inline]
    pub fn pop_<T: GenericNBT>(
        &mut self,
    ) -> Option<T::Type<<MutableConfig<O> as ConfigRef>::ByteOrder>> {
        let len = self.len();
        if len == 0 || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }

        unsafe {
            if !T::can_take_list_elem::<MutableConfig<O>>(self._to_read_params(), len - 1) {
                cold_path();
                return None;
            }
            T::own_into_(MutableConfig::<O>::list_pop_value(
                T::TAG_ID,
                self._to_write_params(),
            ))
        }
    }

    #[inline]
    pub fn insert<V: IntoNBT<O>>(&mut self, index: usize, value: V) {
        if index > self.len() {
            cold_path();
            return;
        }

        if self.element_tag_id() != <V::Tag>::TAG_ID {
            cold_path();
            if self.element_tag_id() == TagID::End && self.is_empty() {
                *unsafe { self.data.get_unchecked_mut(0) } = <V::Tag>::TAG_ID as u8;
            } else {
                cold_path();
                return;
            }
        }

        unsafe {
            MutableConfig::<O>::list_insert::<V::Tag>(
                self._to_write_params(),
                index,
                value.into_nbt(),
            )
        }
    }

    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<ValueOwn<O>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        unsafe {
            Some(MutableConfig::<O>::list_remove_value(
                self.element_tag_id(),
                self._to_write_params(),
                index,
            ))
        }
    }

    #[inline]
    pub fn remove_<T: GenericNBT>(&mut self, index: usize) -> Option<T::Type<O>> {
        if index >= self.len() || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }

        unsafe {
            if !T::can_take_list_elem::<MutableConfig<O>>(self._to_read_params(), index) {
                cold_path();
                return None;
            }
            T::own_into_(MutableConfig::<O>::list_remove_value(
                T::TAG_ID,
                self._to_write_params(),
                index,
            ))
        }
    }

    #[inline]
    pub fn typed_<T: NBT>(self) -> Option<TypedListOwn<O, T>> {
        if !self.element_is_::<T>() {
            return None;
        }
        let me = ManuallyDrop::new(self);
        let mut new = TypedListOwn {
            data: unsafe { ptr::read(&me.data) },
            _marker: PhantomData,
        };
        *unsafe { new.data.get_unchecked_mut(0) } = T::TAG_ID as u8;
        Some(new)
    }
}

impl<O: ByteOrder> Drop for ListOwn<O> {
    fn drop(&mut self) {
        unsafe {
            let mut ptr = self.data.as_mut_ptr();

            let tag_id = *ptr.cast::<TagID>();
            ptr = ptr.add(1);

            if tag_id.is_primitive() {
                return;
            }

            let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get();
            ptr = ptr.add(4);

            match tag_id {
                TagID::ByteArray => {
                    for _ in 0..len {
                        ptr::read(ptr.cast::<VecOwn<i8>>());
                        ptr = ptr.add(mutable_tag_size(tag_id));
                    }
                }
                TagID::String => {
                    for _ in 0..len {
                        ptr::read(ptr.cast::<StringOwn>());
                        ptr = ptr.add(mutable_tag_size(tag_id));
                    }
                }
                TagID::List => {
                    for _ in 0..len {
                        ptr::read(ptr.cast::<ListOwn<O>>());
                        ptr = ptr.add(mutable_tag_size(tag_id));
                    }
                }
                TagID::Compound => {
                    for _ in 0..len {
                        ptr::read(ptr.cast::<CompoundOwn<O>>());
                        ptr = ptr.add(mutable_tag_size(tag_id));
                    }
                }
                TagID::IntArray => {
                    for _ in 0..len {
                        ptr::read(ptr.cast::<VecOwn<byteorder::I32<O>>>());
                        ptr = ptr.add(mutable_tag_size(tag_id));
                    }
                }
                TagID::LongArray => {
                    for _ in 0..len {
                        ptr::read(ptr.cast::<VecOwn<byteorder::I64<O>>>());
                        ptr = ptr.add(mutable_tag_size(tag_id));
                    }
                }
                _ => unreachable_unchecked(),
            }
            debug_assert!(ptr.byte_offset_from_unsigned(self.data.as_mut_ptr()) == self.data.len());
        }
    }
}
