//! Base traits for NBT values.
//!
//! This module defines foundational traits that are used throughout
//! the value system:
//!
//! - [`Writable`] - Trait for serializing NBT values to bytes
//! - [`ValueBase`] - Base trait for all NBT values
//! - [`ListBase`] - Base trait for NBT lists
//! - [`TypedListBase`] - Base trait for typed NBT lists
//! - [`CompoundBase`] - Base trait for NBT compounds

use std::io::Write;

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigMut, ConfigRef, ListMut, ListRef, MapMut, MapRef, NBT, NBTBase, Result,
    StringMut, TagID, ValueMut, ValueRef, VecMut, VisitMut, VisitMutShared, VisitRef, cold_path,
    tag::{
        Byte, ByteArray, Compound, Double, End, Float, Int, IntArray, List, Long, LongArray, Short,
        String, TypedList,
    },
};

/// Trait for writing NBT values to bytes.
///
/// This trait provides methods for serializing NBT values to different
/// output targets with configurable endianness.
pub trait Writable {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8>;

    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()>;
}

/// Base trait for all NBT values.
///
/// This trait provides the minimal interface shared by all NBT value types,
/// including methods for getting the tag ID and type checking.
pub trait ValueBase: Writable + Send + Sync + Sized {
    fn tag_id(&self) -> TagID;

    #[inline]
    fn is_<T: NBT>(&self) -> bool {
        self.tag_id() == T::TAG_ID
    }
}

/// Base trait for NBT lists.
///
/// This trait provides the minimal interface shared by all NBT list types,
/// including methods for getting the element tag ID, list length, and checking emptiness.
pub trait ListBase: Writable + Send + Sync + Sized {
    fn element_tag_id(&self) -> TagID;

    #[inline]
    fn element_is_<T: NBT>(&self) -> bool {
        self.element_tag_id() == T::TAG_ID
            || (self.element_tag_id() == TagID::End && self.is_empty())
    }

    fn len(&self) -> usize;

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Base trait for typed NBT lists.
///
/// This trait provides the minimal interface for homogeneous NBT lists
/// where all elements have the same type `T`. The element tag ID is
/// available as a constant for compile-time type checking.
///
/// # Type Parameters
///
/// * `T` - The NBT tag type of all elements
pub trait TypedListBase<T: NBT>: Writable + Send + Sync + Sized {
    const ELEMENT_TAG_ID: TagID = T::TAG_ID;

    fn len(&self) -> usize;

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Base trait for NBT compounds.
///
/// This trait provides the minimal interface for NBT compound values,
/// which are key-value maps where keys are MUTF-8 strings and values
/// are any NBT type.
pub trait CompoundBase: Writable + Send + Sync + Sized {}

pub trait NBTInto: NBTBase {
    fn ref_into_<'s, V: ValueRef<'s>>(value: V) -> Option<Self::TypeRef<'s, V::Config>>;

    fn mut_into_<'s, V: ValueMut<'s>>(value: V) -> Option<Self::TypeMut<'s, V::Config>>;

    unsafe fn read_static<'a, 'doc, C: ConfigRef>(
        params: C::ReadParams<'a>,
    ) -> Option<Self::TypeRef<'doc, C>>;

    unsafe fn read_mut_static<'a, 'doc, C: ConfigMut>(
        params: C::ReadParams<'a>,
    ) -> Option<Self::TypeMut<'doc, C>>;

    unsafe fn list_get_static<'a, C: ConfigRef>(
        params: C::ReadParams<'a>,
        index: usize,
    ) -> C::ReadParams<'a>;

    unsafe fn list_pop_static<'s, L: ListMut<'s>>(
        list: &mut L,
        len: usize,
    ) -> Option<Self::Type<<L::Config as ConfigRef>::ByteOrder>>;

    unsafe fn list_remove_static<'s, L: ListMut<'s>>(
        list: &mut L,
        index: usize,
    ) -> Option<Self::Type<<L::Config as ConfigRef>::ByteOrder>>;
}

pub trait NBTRef: NBTBase {
    fn ref_<'a, 's: 'a, V: ValueRef<'s>>(value: &'a V) -> Option<&'a Self::TypeRef<'s, V::Config>>;

    fn mut_<'a, 's: 'a, V: ValueMut<'s>>(
        value: &'a mut V,
    ) -> Option<&'a mut Self::TypeMut<'s, V::Config>>;

    fn mut_shared_<'a, 's: 'a, V: ValueMut<'s>>(
        value: &'a V,
    ) -> Option<&'a Self::TypeMut<'s, V::Config>>;

    unsafe fn read_advance_nbt<'a, 'doc, C: ConfigRef>(
        params: &mut C::ReadParams<'a>,
    ) -> Self::TypeRef<'doc, C>;

    unsafe fn read_mut_nbt<'a, 'doc, C: ConfigMut>(
        params: C::ReadParams<'a>,
    ) -> Self::TypeMut<'doc, C>;
}

macro_rules! impl_value_ref_dispatch {
    ($(
        $t:ident => {
            read($c_read:ident, $p_read:ident): $read_expr:expr,
            read_mut($c_mut:ident, $p_mut:ident): $read_mut_expr:expr,
            list_get($c_get:ident, $p_get:ident, $idx:ident): $list_get_expr:expr,
        }
    ),* $(,)?) => {
        $(
            impl NBTInto for $t {
                #[inline]
                fn ref_into_<'s, V: ValueRef<'s>>(value: V) -> Option<Self::TypeRef<'s, V::Config>> {
                    value.map(|value| match value {
                        MapRef::$t(v) => Some(v),
                        _ => None,
                    })
                }

                #[inline]
                fn mut_into_<'s, V: ValueMut<'s>>(value: V) -> Option<Self::TypeMut<'s, V::Config>> {
                    value.map(|value| match value {
                        MapMut::$t(v) => Some(v),
                        _ => None,
                    })
                }

                #[inline]
                unsafe fn read_static<'a, 'doc, C: ConfigRef>(
                    mut params: C::ReadParams<'a>,
                ) -> Option<Self::TypeRef<'doc, C>> {
                    Some(unsafe { Self::read_advance_nbt::<C>(&mut params) })
                }

                #[inline]
                unsafe fn read_mut_static<'a, 'doc, C: ConfigMut>(
                    params: C::ReadParams<'a>,
                ) -> Option<Self::TypeMut<'doc, C>> {
                    Some(unsafe { Self::read_mut_nbt::<C>(params) })
                }

                #[inline]
                unsafe fn list_get_static<'a, $c_get: ConfigRef>(
                    $p_get: $c_get::ReadParams<'a>,
                    $idx: usize,
                ) -> $c_get::ReadParams<'a> {
                    #[allow(unused_unsafe)]
                    unsafe { $list_get_expr }
                }

                #[inline]
                unsafe fn list_pop_static<'s, L: ListMut<'s>>(
                    list: &mut L,
                    _len: usize,
                ) -> Option<Self::Type<<L::Config as ConfigRef>::ByteOrder>> {
                    Some(unsafe { L::Config::list_pop::<Self>(list._to_write_params()) })
                }

                #[inline]
                unsafe fn list_remove_static<'s, L: ListMut<'s>>(
                    list: &mut L,
                    index: usize,
                ) -> Option<Self::Type<<L::Config as ConfigRef>::ByteOrder>> {
                    Some(unsafe { L::Config::list_remove::<Self>(list._to_write_params(), index) })
                }
            }

            impl NBTRef for $t {
                #[inline]
                fn ref_<'a, 's: 'a, V: ValueRef<'s>>(value: &'a V) -> Option<&'a Self::TypeRef<'s, V::Config>> {
                    value.visit(|value| match value {
                        VisitRef::$t(v) => Some(v),
                        _ => None,
                    })
                }

                #[inline]
                fn mut_<'a, 's: 'a, V: ValueMut<'s>>(value: &'a mut V) -> Option<&'a mut Self::TypeMut<'s, V::Config>> {
                    value.visit(|value| match value {
                        VisitMut::$t(v) => Some(v),
                        _ => None,
                    })
                }

                #[inline]
                fn mut_shared_<'a, 's: 'a, V: ValueMut<'s>>(value: &'a V) -> Option<&'a Self::TypeMut<'s, V::Config>> {
                    value.visit_shared(|value| match value {
                        VisitMutShared::$t(v) => Some(v),
                        _ => None,
                    })
                }

                #[inline]
                unsafe fn read_advance_nbt<'a, 'doc, $c_read: ConfigRef>(
                    $p_read: &mut $c_read::ReadParams<'a>,
                ) -> Self::TypeRef<'doc, $c_read> {
                    #[allow(unused_unsafe)]
                    unsafe { $read_expr }
                }

                #[inline]
                unsafe fn read_mut_nbt<'a, 'doc, $c_mut: ConfigMut>(
                    $p_mut: $c_mut::ReadParams<'a>,
                ) -> Self::TypeMut<'doc, $c_mut> {
                    #[allow(unused_unsafe)]
                    unsafe { $read_mut_expr }
                }
            }
        )*
    };
}

impl_value_ref_dispatch! {
    End => {
        read(C, _params): (),
        read_mut(C, params): &mut *C::read_ptr(&params).cast_mut().cast(),
        list_get(C, params, _index): params,
    },
    Byte => {
        read(C, params): *C::read_primitive_advance(params, 1).cast(),
        read_mut(C, params): &mut *C::read_ptr(&params).cast_mut().cast(),
        list_get(C, params, index): C::list_get_primitive(params, index, 1),
    },
    Short => {
        read(C, params): byteorder::I16::<C::ByteOrder>::from_bytes(
            *C::read_primitive_advance(params, 2).cast()
        ).get(),
        read_mut(C, params): &mut *C::read_ptr(&params).cast_mut().cast(),
        list_get(C, params, index): C::list_get_primitive(params, index, 2),
    },
    Int => {
        read(C, params): byteorder::I32::<C::ByteOrder>::from_bytes(
            *C::read_primitive_advance(params, 4).cast()
        ).get(),
        read_mut(C, params): &mut *C::read_ptr(&params).cast_mut().cast(),
        list_get(C, params, index): C::list_get_primitive(params, index, 4),
    },
    Long => {
        read(C, params): byteorder::I64::<C::ByteOrder>::from_bytes(
            *C::read_primitive_advance(params, 8).cast()
        ).get(),
        read_mut(C, params): &mut *C::read_ptr(&params).cast_mut().cast(),
        list_get(C, params, index): C::list_get_primitive(params, index, 8),
    },
    Float => {
        read(C, params): byteorder::F32::<C::ByteOrder>::from_bytes(
            *C::read_primitive_advance(params, 4).cast()
        ).get(),
        read_mut(C, params): &mut *C::read_ptr(&params).cast_mut().cast(),
        list_get(C, params, index): C::list_get_primitive(params, index, 4),
    },
    Double => {
        read(C, params): byteorder::F64::<C::ByteOrder>::from_bytes(
            *C::read_primitive_advance(params, 8).cast()
        ).get(),
        read_mut(C, params): &mut *C::read_ptr(&params).cast_mut().cast(),
        list_get(C, params, index): C::list_get_primitive(params, index, 8),
    },
    ByteArray => {
        read(C, params): C::read_byte_array_advance(params),
        read_mut(C, params): VecMut::from_raw(C::read_ptr(&params).cast_mut()),
        list_get(C, params, index): C::list_get_array(params, index, 1),
    },
    String => {
        read(C, params): C::read_string_advance(params),
        read_mut(C, params): StringMut::from_raw(C::read_ptr(&params).cast_mut()),
        list_get(C, params, index): C::list_get_string(params, index),
    },
    List => {
        read(C, params): C::read_list_advance(params),
        read_mut(C, params): C::read_list_mut(params),
        list_get(C, params, index): C::list_get_composite(params, index),
    },
    Compound => {
        read(C, params): C::read_compound_advance(params),
        read_mut(C, params): C::read_compound_mut(params),
        list_get(C, params, index): C::list_get_composite(params, index),
    },
    IntArray => {
        read(C, params): C::read_int_array_advance(params),
        read_mut(C, params): VecMut::from_raw(C::read_ptr(&params).cast_mut()),
        list_get(C, params, index): C::list_get_array(params, index, 4),
    },
    LongArray => {
        read(C, params): C::read_long_array_advance(params),
        read_mut(C, params): VecMut::from_raw(C::read_ptr(&params).cast_mut()),
        list_get(C, params, index): C::list_get_array(params, index, 8),
    },
}

impl<T: NBT> NBTInto for TypedList<T> {
    #[inline]
    fn ref_into_<'s, V: ValueRef<'s>>(value: V) -> Option<Self::TypeRef<'s, V::Config>> {
        value.map(|value| match value {
            MapRef::List(v) => v.typed_::<T>(),
            _ => None,
        })
    }

    #[inline]
    fn mut_into_<'s, V: ValueMut<'s>>(value: V) -> Option<Self::TypeMut<'s, V::Config>> {
        value.map(|value| match value {
            MapMut::List(v) => v.typed_::<T>(),
            _ => None,
        })
    }

    #[inline]
    unsafe fn read_static<'a, 'doc, C: ConfigRef>(
        mut params: C::ReadParams<'a>,
    ) -> Option<Self::TypeRef<'doc, C>> {
        unsafe { C::read_list_advance(&mut params) }.typed_::<T>()
    }

    #[inline]
    unsafe fn read_mut_static<'a, 'doc, C: ConfigMut>(
        params: C::ReadParams<'a>,
    ) -> Option<Self::TypeMut<'doc, C>> {
        unsafe { C::read_list_mut(params) }.typed_::<T>()
    }

    #[inline]
    unsafe fn list_get_static<'a, C: ConfigRef>(
        params: C::ReadParams<'a>,
        index: usize,
    ) -> C::ReadParams<'a> {
        unsafe { C::list_get_composite(params, index) }
    }

    #[inline]
    unsafe fn list_pop_static<'s, L: ListMut<'s>>(
        list: &mut L,
        len: usize,
    ) -> Option<Self::Type<<L::Config as ConfigRef>::ByteOrder>> {
        unsafe {
            let elem_ok = {
                let mut elem_params =
                    L::Config::list_get_composite(list._to_read_params(), len - 1);
                L::Config::read_list_advance(&mut elem_params).element_is_::<T>()
            };
            if !elem_ok {
                cold_path();
                return None;
            }
            L::Config::list_pop::<List>(list._to_write_params()).typed_::<T>()
        }
    }

    #[inline]
    unsafe fn list_remove_static<'s, L: ListMut<'s>>(
        list: &mut L,
        index: usize,
    ) -> Option<Self::Type<<L::Config as ConfigRef>::ByteOrder>> {
        unsafe {
            let elem_ok = {
                let mut elem_params = L::Config::list_get_composite(list._to_read_params(), index);
                L::Config::read_list_advance(&mut elem_params).element_is_::<T>()
            };
            if !elem_ok {
                cold_path();
                return None;
            }
            L::Config::list_remove::<List>(list._to_write_params(), index).typed_::<T>()
        }
    }
}
