//! Mutable value traits for NBT values.
//!
//! This module defines traits for accessing and modifying NBT values.
//! These traits provide methods for:
//! - Reading values through shared or unique mutable references
//! - Modifying list and compound contents
//! - Pattern matching through visitor patterns
//!
//! # Key Traits
//!
//! - [`ValueMut`] - Mutable access to any NBT value
//! - [`ListMut`] - Mutable access to NBT lists
//! - [`TypedListMut`] - Mutable access to typed NBT lists
//! - [`CompoundMut`] - Mutable access to NBT compounds

use crate::{
    CompoundBase, ConfigMut, ConfigRef, GenericNBT, IntoNBT, ListBase, MapMut, NBT, NBTBase, TagID,
    TypedListBase, ValueBase, ValueOwn, VisitMut, VisitMutShared, cold_path,
    index::Index,
    tag::{Compound, List},
    with_mutf8_key,
};

/// Trait for mutable access to NBT values.
///
/// This trait provides methods for accessing and modifying NBT values.
/// It combines read-only access through shared references with full
/// mutation capabilities through unique mutable references.
///
/// # Type Parameters
///
/// * `'s` - Lifetime of the source data
pub trait ValueMut<'s>: ValueBase {
    type Config: ConfigMut;

    #[inline]
    fn ref_<'a, T: NBT>(&'a self) -> Option<&'a T::TypeMut<'s, Self::Config>>
    where
        's: 'a,
    {
        T::mut_shared_(self)
    }

    #[inline]
    fn mut_<'a, T: NBT>(&'a mut self) -> Option<&'a mut T::TypeMut<'s, Self::Config>>
    where
        's: 'a,
    {
        T::mut_(self)
    }

    #[inline]
    fn into_<T: GenericNBT>(self) -> Option<T::TypeMut<'s, Self::Config>> {
        T::mut_into_(self)
    }

    #[inline]
    fn get<'a>(&'a self, index: impl Index) -> Option<<Self::Config as ConfigRef>::Value<'a>>
    where
        's: 'a,
    {
        index.index_dispatch(
            self,
            |value, index| value.ref_::<List>()?.get(index),
            |value, key| value.ref_::<Compound>()?.get(key),
        )
    }

    #[inline]
    fn get_<'a, T: GenericNBT>(&'a self, index: impl Index) -> Option<T::TypeRef<'a, Self::Config>>
    where
        's: 'a,
    {
        index.index_dispatch(
            self,
            |value, index| value.ref_::<List>()?.get_::<T>(index),
            |value, key| value.ref_::<Compound>()?.get_::<T>(key),
        )
    }

    #[inline]
    fn get_mut<'a>(
        &'a mut self,
        index: impl Index,
    ) -> Option<<Self::Config as ConfigMut>::ValueMut<'a>>
    where
        's: 'a,
    {
        index.index_dispatch_mut(
            self,
            |value, index| value.mut_::<List>()?.get_mut(index),
            |value, key| value.mut_::<Compound>()?.get_mut(key),
        )
    }

    #[inline]
    fn get_mut_<'a, T: GenericNBT>(
        &'a mut self,
        index: impl Index,
    ) -> Option<T::TypeMut<'a, Self::Config>>
    where
        's: 'a,
    {
        index.index_dispatch_mut(
            self,
            |value, index| value.mut_::<List>()?.get_mut_::<T>(index),
            |value, key| value.mut_::<Compound>()?.get_mut_::<T>(key),
        )
    }

    fn visit_shared<'a, R>(
        &'a self,
        match_fn: impl FnOnce(VisitMutShared<'a, 's, Self::Config>) -> R,
    ) -> R
    where
        's: 'a;

    fn visit<'a, R>(&'a mut self, match_fn: impl FnOnce(VisitMut<'a, 's, Self::Config>) -> R) -> R
    where
        's: 'a;

    fn map<R>(self, match_fn: impl FnOnce(MapMut<'s, Self::Config>) -> R) -> R;

    fn to_ref<'a>(&'a self) -> <Self::Config as ConfigRef>::Value<'a>;
}

/// Trait for mutable access to NBT lists.
///
/// This trait provides methods for accessing and modifying NBT lists.
///
/// Also see [`TypedListMut`].
///
/// # Type Parameters
///
/// * `'s` - Lifetime of the source data
pub trait ListMut<'s>:
    ListBase
    + IntoIterator<
        IntoIter = <Self::Config as ConfigMut>::ListIterMut<'s>,
        Item = <Self::Config as ConfigMut>::ValueMut<'s>,
    >
{
    type Config: ConfigMut;

    fn _set_element_tag_id<T: NBT>(&mut self);

    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        's: 'a;

    fn _to_write_params<'a>(&'a mut self) -> <Self::Config as ConfigMut>::WriteParams<'a>
    where
        's: 'a;

    #[inline]
    fn get<'a>(&'a self, index: usize) -> Option<<Self::Config as ConfigRef>::Value<'a>>
    where
        's: 'a,
    {
        if index >= self.len() {
            cold_path();
            return None;
        }

        let tag_id = self.element_tag_id();
        unsafe {
            Some(Self::Config::read_value(
                tag_id,
                Self::Config::list_get(tag_id, self._to_read_params(), index),
            ))
        }
    }

    #[inline]
    fn get_<'a, T: GenericNBT>(&'a self, index: usize) -> Option<T::TypeRef<'a, Self::Config>>
    where
        's: 'a,
    {
        if index >= self.len() || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }

        unsafe {
            Self::Config::read::<T>(Self::Config::list_get_typed::<T>(
                self._to_read_params(),
                index,
            ))
        }
    }

    #[inline]
    fn get_mut<'a>(&'a mut self, index: usize) -> Option<<Self::Config as ConfigMut>::ValueMut<'a>>
    where
        's: 'a,
    {
        if index >= self.len() {
            cold_path();
            return None;
        }

        let tag_id = self.element_tag_id();
        unsafe {
            Some(Self::Config::read_value_mut(
                tag_id,
                Self::Config::list_get(tag_id, self._to_read_params(), index),
            ))
        }
    }

    #[inline]
    fn get_mut_<'a, T: GenericNBT>(
        &'a mut self,
        index: usize,
    ) -> Option<T::TypeMut<'a, Self::Config>>
    where
        's: 'a,
    {
        if index >= self.len() || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }

        unsafe {
            Self::Config::read_mut::<T>(Self::Config::list_get_typed::<T>(
                self._to_read_params(),
                index,
            ))
        }
    }

    #[inline]
    fn push<V: IntoNBT<<Self::Config as ConfigRef>::ByteOrder>>(&mut self, value: V) {
        if self.element_tag_id() != <V::Tag>::TAG_ID {
            cold_path();
            if self.element_tag_id() == TagID::End && self.is_empty() {
                self._set_element_tag_id::<V::Tag>();
            } else {
                cold_path();
                return;
            }
        }

        unsafe { Self::Config::list_push::<V::Tag>(self._to_write_params(), value.into_nbt()) }
    }

    #[inline]
    fn pop(&mut self) -> Option<ValueOwn<<Self::Config as ConfigRef>::ByteOrder>> {
        if self.is_empty() {
            cold_path();
            return None;
        }

        unsafe {
            Some(Self::Config::list_pop_value(
                self.element_tag_id(),
                self._to_write_params(),
            ))
        }
    }

    #[inline]
    fn pop_<T: GenericNBT>(&mut self) -> Option<T::Type<<Self::Config as ConfigRef>::ByteOrder>> {
        let len = self.len();
        if len == 0 || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }

        unsafe { T::list_pop_static(self, len) }
    }

    #[inline]
    fn insert<V: IntoNBT<<Self::Config as ConfigRef>::ByteOrder>>(
        &mut self,
        index: usize,
        value: V,
    ) {
        if index > self.len() {
            cold_path();
            return;
        }

        if self.element_tag_id() != <V::Tag>::TAG_ID {
            cold_path();
            if self.element_tag_id() == TagID::End && self.is_empty() {
                self._set_element_tag_id::<V::Tag>();
            } else {
                cold_path();
                return;
            }
        }

        unsafe {
            Self::Config::list_insert::<V::Tag>(self._to_write_params(), index, value.into_nbt())
        }
    }

    #[inline]
    fn remove(&mut self, index: usize) -> Option<ValueOwn<<Self::Config as ConfigRef>::ByteOrder>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        unsafe {
            Some(Self::Config::list_remove_value(
                self.element_tag_id(),
                self._to_write_params(),
                index,
            ))
        }
    }

    #[inline]
    fn remove_<T: GenericNBT>(
        &mut self,
        index: usize,
    ) -> Option<T::Type<<Self::Config as ConfigRef>::ByteOrder>> {
        if index >= self.len() || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }

        unsafe { T::list_remove_static(self, index) }
    }

    fn typed_<T: NBT>(self) -> Option<<Self::Config as ConfigMut>::TypedListMut<'s, T>>;

    fn iter<'a>(&'a self) -> <Self::Config as ConfigRef>::ListIter<'a>
    where
        's: 'a;

    fn iter_mut<'a>(&'a mut self) -> <Self::Config as ConfigMut>::ListIterMut<'a>
    where
        's: 'a;
}

/// Trait for mutable access to typed NBT lists.
///
/// This trait provides methods for accessing and modifying NBT lists,
/// where all elements have the same type `T`. This provides type-safe
/// mutation of list elements.
///
/// # Type Parameters
///
/// * `'s` - Lifetime of the source data
/// * `T` - The NBT tag type of all elements
pub trait TypedListMut<'s, T: NBT>:
    TypedListBase<T>
    + IntoIterator<
        IntoIter = <Self::Config as ConfigMut>::TypedListIterMut<'s, T>,
        Item = T::TypeMut<'s, Self::Config>,
    >
{
    type Config: ConfigMut;

    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        's: 'a;

    fn _to_write_params<'a>(&'a mut self) -> <Self::Config as ConfigMut>::WriteParams<'a>
    where
        's: 'a;

    #[inline]
    fn get<'a>(&'a self, index: usize) -> Option<T::TypeRef<'a, Self::Config>>
    where
        's: 'a,
    {
        if index >= self.len() {
            cold_path();
            return None;
        }

        unsafe {
            let mut params = Self::Config::list_get_typed::<T>(self._to_read_params(), index);
            Some(T::read_advance_nbt::<Self::Config>(&mut params))
        }
    }

    #[inline]
    fn get_mut<'a>(&'a mut self, index: usize) -> Option<T::TypeMut<'a, Self::Config>>
    where
        's: 'a,
    {
        if index >= self.len() {
            cold_path();
            return None;
        }

        unsafe {
            let params = Self::Config::list_get_typed::<T>(self._to_read_params(), index);
            Some(T::read_mut_nbt::<Self::Config>(params))
        }
    }

    #[inline]
    fn push(&mut self, value: impl IntoNBT<<Self::Config as ConfigRef>::ByteOrder, Tag = T>) {
        unsafe { Self::Config::list_push::<T>(self._to_write_params(), value.into_nbt()) }
    }

    #[inline]
    fn pop(&mut self) -> Option<T::Type<<Self::Config as ConfigRef>::ByteOrder>> {
        if self.is_empty() {
            cold_path();
            return None;
        }

        Some(unsafe { Self::Config::list_pop::<T>(self._to_write_params()) })
    }

    #[inline]
    fn insert(
        &mut self,
        index: usize,
        value: impl IntoNBT<<Self::Config as ConfigRef>::ByteOrder, Tag = T>,
    ) {
        if index > self.len() {
            cold_path();
            return;
        }

        unsafe { Self::Config::list_insert::<T>(self._to_write_params(), index, value.into_nbt()) }
    }

    #[inline]
    fn remove(&mut self, index: usize) -> Option<T::Type<<Self::Config as ConfigRef>::ByteOrder>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        Some(unsafe { Self::Config::list_remove::<T>(self._to_write_params(), index) })
    }

    fn iter<'a>(&'a self) -> <Self::Config as ConfigRef>::TypedListIter<'a, T>
    where
        's: 'a;

    fn iter_mut<'a>(&'a mut self) -> <Self::Config as ConfigMut>::TypedListIterMut<'a, T>
    where
        's: 'a;
}

/// Trait for mutable access to NBT compounds.
///
/// This trait provides methods for accessing and modifying NBT compound values,
/// which are key-value maps where keys are MUTF-8 strings and values
/// are any NBT type.
///
/// # Type Parameters
///
/// * `'s` - Lifetime of the source data
pub trait CompoundMut<'s>:
    CompoundBase
    + IntoIterator<
        IntoIter = <Self::Config as ConfigMut>::CompoundIterMut<'s>,
        Item = (
            <Self::Config as ConfigRef>::String<'s>,
            <Self::Config as ConfigMut>::ValueMut<'s>,
        ),
    >
{
    type Config: ConfigMut;

    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        's: 'a;

    fn _to_write_params<'a>(&'a mut self) -> <Self::Config as ConfigMut>::WriteParams<'a>
    where
        's: 'a;

    #[inline]
    fn get<'a>(&'a self, key: &str) -> Option<<Self::Config as ConfigRef>::Value<'a>>
    where
        's: 'a,
    {
        with_mutf8_key(key, |key| unsafe {
            let (tag_id, params) = Self::Config::compound_get(self._to_read_params(), key)?;
            Some(Self::Config::read_value(tag_id, params))
        })
    }

    #[inline]
    fn get_<'a, T: GenericNBT>(&'a self, key: &str) -> Option<T::TypeRef<'a, Self::Config>>
    where
        's: 'a,
    {
        with_mutf8_key(key, |key| unsafe {
            let (tag_id, params) = Self::Config::compound_get(self._to_read_params(), key)?;
            if tag_id != T::TAG_ID {
                cold_path();
                return None;
            }
            Self::Config::read::<T>(params)
        })
    }

    #[inline]
    fn get_mut<'a>(&'a mut self, key: &str) -> Option<<Self::Config as ConfigMut>::ValueMut<'a>>
    where
        's: 'a,
    {
        with_mutf8_key(key, |key| unsafe {
            let (tag_id, params) = Self::Config::compound_get(self._to_read_params(), key)?;
            Some(Self::Config::read_value_mut(tag_id, params))
        })
    }

    #[inline]
    fn get_mut_<'a, T: GenericNBT>(&'a mut self, key: &str) -> Option<T::TypeMut<'a, Self::Config>>
    where
        's: 'a,
    {
        with_mutf8_key(key, |key| unsafe {
            let (tag_id, params) = Self::Config::compound_get(self._to_read_params(), key)?;
            if tag_id != T::TAG_ID {
                cold_path();
                return None;
            }
            Self::Config::read_mut::<T>(params)
        })
    }

    #[inline]
    fn insert<T: NBT>(
        &mut self,
        key: &str,
        value: impl IntoNBT<<Self::Config as ConfigRef>::ByteOrder, Tag = T>,
    ) -> Option<ValueOwn<<Self::Config as ConfigRef>::ByteOrder>> {
        with_mutf8_key(key, |key| unsafe {
            let old = Self::Config::compound_remove(self._to_write_params(), key);
            Self::Config::compound_insert::<T>(self._to_write_params(), key, value.into_nbt());
            old
        })
    }

    #[inline]
    fn remove(&mut self, key: &str) -> Option<ValueOwn<<Self::Config as ConfigRef>::ByteOrder>> {
        with_mutf8_key(key, |key| unsafe {
            Self::Config::compound_remove(self._to_write_params(), key)
        })
    }

    fn iter<'a>(&'a self) -> <Self::Config as ConfigRef>::CompoundIter<'a>
    where
        's: 'a;

    fn iter_mut<'a>(&'a mut self) -> <Self::Config as ConfigMut>::CompoundIterMut<'a>
    where
        's: 'a;
}
