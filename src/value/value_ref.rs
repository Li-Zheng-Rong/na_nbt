//! Read-only value traits for NBT values.
//!
//! This module defines traits for accessing NBT values without mutation.
//! These traits provide methods for:
//! - Type-safe access to typed values
//! - Indexing into compounds and lists
//! - Pattern matching through visitor patterns
//!
//! # Key Traits
//!
//! - [`ValueRef`] - Read-only access to any NBT value
//! - [`ListRef`] - Read-only access to NBT lists
//! - [`TypedListRef`] - Read-only access to typed NBT lists
//! - [`CompoundRef`] - Read-only access to NBT compounds

use crate::{
    CompoundBase, ConfigRef, GenericNBT, ListBase, MapRef, NBT, TypedListBase, ValueBase, VisitRef,
    cold_path,
    index::Index,
    tag::{Compound, List},
    with_mutf8_key,
};

/// Trait for read-only access to NBT values.
///
/// This trait provides methods for accessing and inspecting NBT values
/// without mutation. It's implemented by all value types returned by
/// zero-copy reading operations.
///
/// # Type Parameters
///
/// * `'s` - Lifetime of the source data
pub trait ValueRef<'s>: ValueBase + Clone + Default {
    type Config: ConfigRef;

    #[inline]
    fn ref_<'a, T: NBT>(&'a self) -> Option<&'a T::TypeRef<'s, Self::Config>>
    where
        's: 'a,
    {
        T::ref_(self)
    }

    #[inline]
    fn into_<T: GenericNBT>(self) -> Option<T::TypeRef<'s, Self::Config>> {
        T::ref_into_(self)
    }

    #[inline]
    fn get(&self, index: impl Index) -> Option<<Self::Config as ConfigRef>::Value<'s>> {
        index.index_dispatch(
            self,
            |value, index| value.ref_::<List>()?.get(index),
            |value, key| value.ref_::<Compound>()?.get(key),
        )
    }

    #[inline]
    fn get_<T: GenericNBT>(&self, index: impl Index) -> Option<T::TypeRef<'s, Self::Config>> {
        index.index_dispatch(
            self,
            |value, index| value.ref_::<List>()?.get_::<T>(index),
            |value, key| value.ref_::<Compound>()?.get_::<T>(key),
        )
    }

    fn visit<'a, R>(&'a self, match_fn: impl FnOnce(VisitRef<'a, 's, Self::Config>) -> R) -> R
    where
        's: 'a;

    fn map<R>(self, match_fn: impl FnOnce(MapRef<'s, Self::Config>) -> R) -> R;
}

/// Trait for read-only access to NBT lists.
///
/// This trait provides methods for accessing NBT lists,
/// Also see [`TypedListRef`].
///
/// # Type Parameters
///
/// * `'s` - Lifetime of the source data
pub trait ListRef<'s>:
    ListBase
    + IntoIterator<
        IntoIter = <Self::Config as ConfigRef>::ListIter<'s>,
        Item = <Self::Config as ConfigRef>::Value<'s>,
    > + Clone
    + Default
{
    type Config: ConfigRef;

    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        's: 'a;

    #[inline]
    fn get(&self, index: usize) -> Option<<Self::Config as ConfigRef>::Value<'s>> {
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
    fn get_<T: GenericNBT>(&self, index: usize) -> Option<T::TypeRef<'s, Self::Config>> {
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

    fn typed_<T: NBT>(self) -> Option<<Self::Config as ConfigRef>::TypedList<'s, T>>;

    fn iter(&self) -> <Self::Config as ConfigRef>::ListIter<'s>;
}

/// Trait for read-only access to typed NBT lists.
///
/// This trait provides methods for accessing NBT lists,
/// where all elements have the same type `T`. This provides type-safe
/// access to list elements.
///
/// # Type Parameters
///
/// * `'s` - Lifetime of the source data
/// * `T` - The NBT tag type of all elements
pub trait TypedListRef<'s, T: NBT>:
    TypedListBase<T>
    + IntoIterator<
        IntoIter = <Self::Config as ConfigRef>::TypedListIter<'s, T>,
        Item = T::TypeRef<'s, Self::Config>,
    > + Clone
    + Default
{
    type Config: ConfigRef;

    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        's: 'a;

    #[inline]
    fn get(&self, index: usize) -> Option<T::TypeRef<'s, Self::Config>> {
        if index >= self.len() {
            cold_path();
            return None;
        }

        unsafe {
            let mut params = Self::Config::list_get_typed::<T>(self._to_read_params(), index);
            Some(T::read_advance_nbt::<Self::Config>(&mut params))
        }
    }

    fn iter(&self) -> <Self::Config as ConfigRef>::TypedListIter<'s, T>;
}

/// Trait for read-only access to NBT compounds.
///
/// This trait provides methods for accessing NBT compound values,
/// which are key-value maps where keys are MUTF-8 strings and values
/// are any NBT type.
///
/// # Type Parameters
///
/// * `'s` - Lifetime of the source data
pub trait CompoundRef<'s>:
    CompoundBase
    + IntoIterator<
        IntoIter = <Self::Config as ConfigRef>::CompoundIter<'s>,
        Item = (
            <Self::Config as ConfigRef>::String<'s>,
            <Self::Config as ConfigRef>::Value<'s>,
        ),
    > + Clone
    + Default
{
    type Config: ConfigRef;

    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        's: 'a;

    #[inline]
    fn get(&self, key: &str) -> Option<<Self::Config as ConfigRef>::Value<'s>> {
        with_mutf8_key(key, |key| unsafe {
            let (tag_id, params) = Self::Config::compound_get(self._to_read_params(), key)?;
            Some(Self::Config::read_value(tag_id, params))
        })
    }

    #[inline]
    fn get_<T: GenericNBT>(&self, key: &str) -> Option<T::TypeRef<'s, Self::Config>> {
        with_mutf8_key(key, |key| unsafe {
            let (tag_id, params) = Self::Config::compound_get(self._to_read_params(), key)?;
            if tag_id != T::TAG_ID {
                cold_path();
                return None;
            }
            Self::Config::read::<T>(params)
        })
    }

    fn iter(&self) -> <Self::Config as ConfigRef>::CompoundIter<'s>;
}
