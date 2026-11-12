use std::marker::PhantomData;

use crate::{
    ByteOrder, CompoundMut, CompoundRef, ConfigMut, ConfigRef, GenericNBT, IntoNBT, NBT, ValueOwn,
    VecOwn, cold_path,
    mutable::{
        compound_mut::MutCompound, compound_ref::RefCompound, config::MutableConfig,
        size::drop_compound_entries, value_mut::MutValue, value_ref::RefValue,
    },
    with_mutf8_key,
};

#[repr(transparent)]
pub struct CompoundOwn<O: ByteOrder> {
    pub(crate) data: VecOwn<u8>,
    pub(crate) _marker: PhantomData<O>,
}

impl<O: ByteOrder> Default for CompoundOwn<O> {
    fn default() -> Self {
        Self {
            data: vec![0].into(),
            _marker: PhantomData,
        }
    }
}

impl<O: ByteOrder> CompoundOwn<O> {
    #[inline]
    pub(crate) fn as_ref(&self) -> RefCompound<'_, O> {
        RefCompound {
            data: self.data.as_ptr(),
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn as_mut(&mut self) -> MutCompound<'_, O> {
        MutCompound {
            data: self.data.to_mut(),
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn get<'a>(&'a self, key: &str) -> Option<RefValue<'a, O>> {
        self.as_ref().get(key)
    }

    #[inline]
    pub fn get_<'a, T: GenericNBT>(
        &'a self,
        key: &str,
    ) -> Option<T::TypeRef<'a, MutableConfig<O>>> {
        self.as_ref().get_::<T>(key)
    }

    #[inline]
    pub fn get_mut<'a>(&'a mut self, key: &str) -> Option<MutValue<'a, O>> {
        with_mutf8_key(key, |key| unsafe {
            let (tag_id, params) = MutableConfig::<O>::compound_get(self.data.as_ptr(), key)?;
            Some(MutableConfig::<O>::read_value_mut(tag_id, params))
        })
    }

    #[inline]
    pub fn get_mut_<'a, T: GenericNBT>(
        &'a mut self,
        key: &str,
    ) -> Option<T::TypeMut<'a, MutableConfig<O>>> {
        with_mutf8_key(key, |key| unsafe {
            let (tag_id, params) = MutableConfig::<O>::compound_get(self.data.as_ptr(), key)?;
            if tag_id != T::TAG_ID {
                cold_path();
                return None;
            }
            MutableConfig::<O>::read_mut::<T>(params)
        })
    }

    #[inline]
    pub fn insert<T: NBT>(
        &mut self,
        key: &str,
        value: impl IntoNBT<O, Tag = T>,
    ) -> Option<ValueOwn<O>> {
        self.as_mut().insert(key, value)
    }

    #[inline]
    pub fn remove(&mut self, key: &str) -> Option<ValueOwn<O>> {
        self.as_mut().remove(key)
    }
}

impl<O: ByteOrder> Drop for CompoundOwn<O> {
    fn drop(&mut self) {
        unsafe {
            let ptr = self.data.as_mut_ptr();
            drop_compound_entries::<O>(ptr, ptr.add(self.data.len()));
        }
    }
}
