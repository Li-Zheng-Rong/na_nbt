use std::{marker::PhantomData, mem, ptr};

use crate::{
    ByteOrder, ConfigMut, ConfigRef, IntoNBT, NBT, TypedListBase, TypedListMut, TypedListRef,
    VecOwn, cold_path,
    mutable::{
        config::MutableConfig, size::SIZE_DYN, typed_list_mut::MutTypedList,
        typed_list_ref::RefTypedList,
    },
};

#[repr(transparent)]
pub struct TypedListOwn<O: ByteOrder, T: NBT> {
    pub(crate) data: VecOwn<u8>,
    pub(crate) _marker: PhantomData<(O, T)>,
}

impl<O: ByteOrder, T: NBT> Default for TypedListOwn<O, T> {
    fn default() -> Self {
        Self {
            data: vec![T::TAG_ID as u8, 0, 0, 0, 0].into(),
            _marker: PhantomData,
        }
    }
}

impl<O: ByteOrder, T: NBT> TypedListOwn<O, T> {
    #[inline]
    pub(crate) fn as_ref(&self) -> RefTypedList<'_, O, T> {
        RefTypedList {
            data: self.data.as_ptr(),
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn as_mut(&mut self) -> MutTypedList<'_, O, T> {
        MutTypedList {
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
    pub fn len(&self) -> usize {
        self.as_ref().len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }

    #[inline]
    pub fn get<'a>(&'a self, index: usize) -> Option<T::TypeRef<'a, MutableConfig<O>>> {
        self.as_ref().get(index)
    }

    #[inline]
    pub fn get_mut<'a>(&'a mut self, index: usize) -> Option<T::TypeMut<'a, MutableConfig<O>>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        unsafe {
            let params = MutableConfig::<O>::list_get_typed::<T>(self._to_read_params(), index);
            Some(T::read_mut_nbt::<MutableConfig<O>>(params))
        }
    }

    #[inline]
    pub fn push(&mut self, value: impl IntoNBT<O, Tag = T>) {
        self.as_mut().push(value);
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T::Type<O>> {
        self.as_mut().pop()
    }

    #[inline]
    pub fn insert(&mut self, index: usize, value: impl IntoNBT<O, Tag = T>) {
        self.as_mut().insert(index, value);
    }

    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<T::Type<O>> {
        self.as_mut().remove(index)
    }
}

impl<O: ByteOrder, T: NBT> Drop for TypedListOwn<O, T> {
    fn drop(&mut self) {
        if const { !mem::needs_drop::<T::Type<O>>() } {
            return;
        }
        let len = self.len();
        unsafe {
            debug_assert_eq!(mem::size_of::<T::Type<O>>(), SIZE_DYN);
            debug_assert_eq!(mem::align_of::<T::Type<O>>(), 1);
            debug_assert_eq!(1 + 4 + len * SIZE_DYN, self.data.len());
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(
                self.data.as_mut_ptr().add(5).cast::<T::Type<O>>(),
                len,
            ));
        }
    }
}
