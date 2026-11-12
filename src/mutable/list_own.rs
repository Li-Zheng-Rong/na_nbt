use std::{marker::PhantomData, mem::ManuallyDrop, ptr};

use crate::{
    ByteOrder, ConfigMut, ConfigRef, GenericNBT, IntoNBT, ListBase, ListMut, ListRef, NBT, TagID,
    TypedListOwn, ValueOwn, VecOwn, cold_path,
    mutable::{
        config::MutableConfig,
        list_mut::MutList,
        list_ref::RefList,
        size::{SIZE_DYN, drop_list_elements},
        value_mut::MutValue,
        value_ref::RefValue,
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
    pub(crate) fn as_ref(&self) -> RefList<'_, O> {
        RefList {
            data: self.data.as_ptr(),
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn as_mut(&mut self) -> MutList<'_, O> {
        MutList {
            data: self.data.to_mut(),
            _marker: PhantomData,
        }
    }

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
        self.as_ref().element_tag_id()
    }

    #[inline]
    pub fn element_is_<T: NBT>(&self) -> bool {
        self.as_ref().element_is_::<T>()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.as_ref().len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }

    #[inline]
    pub fn get<'a>(&'a self, index: usize) -> Option<RefValue<'a, O>> {
        self.as_ref().get(index)
    }

    #[inline]
    pub fn get_<'a, T: GenericNBT>(
        &'a self,
        index: usize,
    ) -> Option<T::TypeRef<'a, MutableConfig<O>>> {
        self.as_ref().get_::<T>(index)
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

        let tag_id = self.element_tag_id();
        if !(tag_id == T::TAG_ID || (tag_id == TagID::End && self.is_empty())) {
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
        self.as_mut().push(value);
    }

    #[inline]
    pub fn pop(&mut self) -> Option<ValueOwn<O>> {
        self.as_mut().pop()
    }

    #[inline]
    pub fn pop_<T: GenericNBT>(
        &mut self,
    ) -> Option<T::Type<<MutableConfig<O> as ConfigRef>::ByteOrder>> {
        self.as_mut().pop_::<T>()
    }

    #[inline]
    pub fn insert<V: IntoNBT<O>>(&mut self, index: usize, value: V) {
        self.as_mut().insert(index, value);
    }

    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<ValueOwn<O>> {
        self.as_mut().remove(index)
    }

    #[inline]
    pub fn remove_<T: GenericNBT>(&mut self, index: usize) -> Option<T::Type<O>> {
        self.as_mut().remove_::<T>(index)
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
        let tag_id = self.element_tag_id();
        if tag_id.is_primitive() {
            return;
        }
        let len = self.len();
        unsafe {
            debug_assert_eq!(1 + 4 + len * SIZE_DYN, self.data.len());
            drop_list_elements::<O>(tag_id, self.data.as_mut_ptr().add(5), len);
        }
    }
}
