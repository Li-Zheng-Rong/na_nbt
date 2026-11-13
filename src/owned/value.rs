use std::{fmt, io::Write};

use zerocopy::byteorder;

use crate::{
    index::Index, tag, ByteOrder, CompoundOwn, ElementTag, ListOwn, MUTF8Str, MUTF8String, Mut,
    Owned, Result, ScalarMut, Tag, TagID, TypedListOwn, ValueRef, Writable,
};

/// An owned, mutable NBT value.
///
/// Multi-byte scalar variants (`Short`, `Int`, `Long`, `Float`, `Double`) store native Rust
/// primitives directly, while composite and array containers keep target-endian buffers for
/// zero-copy serialization.
#[derive(Clone, Default)]
pub enum ValueOwn<O: ByteOrder> {
    #[default]
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(MUTF8String),
    List(ListOwn<O>),
    Compound(CompoundOwn<O>),
    IntArray(Vec<byteorder::I32<O>>),
    LongArray(Vec<byteorder::I64<O>>),
}

impl<O: ByteOrder> ValueOwn<O> {
    #[inline]
    pub(crate) unsafe fn read_from_slot(tag_id: TagID, src: *const u8) -> Self {
        macro_rules! dispatch {
            ($($t:ident),* $(,)?) => {
                match tag_id {
                    TagID::End => Self::End,
                    $(TagID::$t => Self::$t(unsafe {
                        <tag::$t as ElementTag>::read_own_from_slot::<O>(src)
                    }),)*
                }
            };
        }
        dispatch!(
            Byte, Short, Int, Long, Float, Double, ByteArray, String, List, Compound, IntArray,
            LongArray
        )
    }

    #[inline]
    pub const fn tag_id(&self) -> TagID {
        match self {
            Self::End => TagID::End,
            Self::Byte(_) => TagID::Byte,
            Self::Short(_) => TagID::Short,
            Self::Int(_) => TagID::Int,
            Self::Long(_) => TagID::Long,
            Self::Float(_) => TagID::Float,
            Self::Double(_) => TagID::Double,
            Self::ByteArray(_) => TagID::ByteArray,
            Self::String(_) => TagID::String,
            Self::List(_) => TagID::List,
            Self::Compound(_) => TagID::Compound,
            Self::IntArray(_) => TagID::IntArray,
            Self::LongArray(_) => TagID::LongArray,
        }
    }

    #[inline]
    pub fn is<T: Tag>(&self) -> bool {
        self.as_ref().is::<T>()
    }

    /// Returns a lightweight `Copy` read-only view of this value.
    #[inline]
    pub fn as_ref(&self) -> ValueRef<'_, Owned<O>> {
        match self {
            Self::End => ValueRef::End,
            Self::Byte(v) => ValueRef::Byte(*v),
            Self::Short(v) => ValueRef::Short(*v),
            Self::Int(v) => ValueRef::Int(*v),
            Self::Long(v) => ValueRef::Long(*v),
            Self::Float(v) => ValueRef::Float(*v),
            Self::Double(v) => ValueRef::Double(*v),
            Self::ByteArray(v) => ValueRef::ByteArray(v),
            Self::String(v) => ValueRef::String(v),
            Self::List(v) => ValueRef::List(v.as_ref()),
            Self::Compound(v) => ValueRef::Compound(v.as_ref()),
            Self::IntArray(v) => ValueRef::IntArray(v),
            Self::LongArray(v) => ValueRef::LongArray(v),
        }
    }

    /// Returns a mutable view enum over this value.
    #[inline]
    pub fn as_mut(&mut self) -> ValueMut<'_, O> {
        ValueMut::from(self)
    }

    #[inline]
    pub fn as_<T: Tag>(&self) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().as_::<T>()
    }

    #[inline]
    pub fn as_mut_<T: Tag>(&mut self) -> Option<T::Mut<'_, O>> {
        self.as_mut().into_::<T>()
    }

    #[inline]
    pub fn into_<T: Tag>(self) -> Option<T::Own<O>> {
        T::into_value_own(self)
    }

    #[inline]
    pub fn get(&self, index: impl Index) -> Option<ValueRef<'_, Owned<O>>> {
        self.as_ref().get(index)
    }

    #[inline]
    pub fn get_as<T: Tag>(&self, index: impl Index) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().get_as::<T>(index)
    }

    #[inline]
    pub fn get_mut(&mut self, index: impl Index) -> Option<ValueMut<'_, O>> {
        index.dispatch(
            self,
            |this, idx| match this {
                Self::List(list) => list.get_mut(idx),
                _ => None,
            },
            |this, key| match this {
                Self::Compound(comp) => comp.get_mut(key),
                _ => None,
            },
        )
    }

    #[inline]
    pub fn get_mut_as<T: Tag>(&mut self, index: impl Index) -> Option<T::Mut<'_, O>> {
        index.dispatch(
            self,
            |this, idx| match this {
                Self::List(list) => list.get_mut_as::<T>(idx),
                _ => None,
            },
            |this, key| match this {
                Self::Compound(comp) => comp.get_mut_as::<T>(key),
                _ => None,
            },
        )
    }
}

impl<O: ByteOrder> fmt::Debug for ValueOwn<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

impl<O: ByteOrder> Writable for ValueOwn<O> {
    #[inline]
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        self.as_ref().write_to_vec::<TARGET>()
    }

    #[inline]
    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        self.as_ref().write_to_writer::<TARGET>(writer)
    }
}

impl<O: ByteOrder> From<()> for ValueOwn<O> {
    #[inline]
    fn from((): ()) -> Self {
        Self::End
    }
}

macro_rules! impl_from_value_own {
    ($($ty:ty => $variant:ident),* $(,)?) => {
        $(
            impl<O: ByteOrder> From<$ty> for ValueOwn<O> {
                #[inline]
                fn from(v: $ty) -> Self {
                    Self::$variant(v)
                }
            }
        )*
    };
}

impl_from_value_own!(
    i8 => Byte,
    i16 => Short,
    i32 => Int,
    i64 => Long,
    f32 => Float,
    f64 => Double,
    Vec<i8> => ByteArray,
    MUTF8String => String,
    ListOwn<O> => List,
    CompoundOwn<O> => Compound,
    Vec<byteorder::I32<O>> => IntArray,
    Vec<byteorder::I64<O>> => LongArray,
);

macro_rules! impl_from_endian_scalar {
    ($($wrapper:ident => $variant:ident),* $(,)?) => {
        $(
            impl<O: ByteOrder> From<byteorder::$wrapper<O>> for ValueOwn<O> {
                #[inline]
                fn from(v: byteorder::$wrapper<O>) -> Self {
                    Self::$variant(v.get())
                }
            }
        )*
    };
}

impl_from_endian_scalar!(
    I16 => Short,
    I32 => Int,
    I64 => Long,
    F32 => Float,
    F64 => Double,
);

impl<O: ByteOrder> From<&[i8]> for ValueOwn<O> {
    #[inline]
    fn from(v: &[i8]) -> Self {
        Self::ByteArray(v.to_vec())
    }
}

impl<O: ByteOrder> From<&MUTF8Str> for ValueOwn<O> {
    #[inline]
    fn from(v: &MUTF8Str) -> Self {
        Self::String(MUTF8String::from(v))
    }
}

impl<O: ByteOrder> From<&str> for ValueOwn<O> {
    #[inline]
    fn from(v: &str) -> Self {
        Self::String(MUTF8String::from(v))
    }
}

impl<O: ByteOrder> From<String> for ValueOwn<O> {
    #[inline]
    fn from(v: String) -> Self {
        Self::String(MUTF8String::from(v))
    }
}

impl<O: ByteOrder, T: ElementTag> From<TypedListOwn<O, T>> for ValueOwn<O> {
    #[inline]
    fn from(v: TypedListOwn<O, T>) -> Self {
        Self::List(ListOwn::from(v))
    }
}

/// A mutable view of an NBT value inside a [`CompoundOwn`], [`ListOwn`], or [`ValueOwn`].
#[derive(Default)]
pub enum ValueMut<'a, O: ByteOrder> {
    #[default]
    End,
    Byte(&'a mut i8),
    Short(ScalarMut<'a, i16>),
    Int(ScalarMut<'a, i32>),
    Long(ScalarMut<'a, i64>),
    Float(ScalarMut<'a, f32>),
    Double(ScalarMut<'a, f64>),
    ByteArray(Mut<'a, Vec<i8>>),
    String(Mut<'a, MUTF8String>),
    List(Mut<'a, ListOwn<O>>),
    Compound(Mut<'a, CompoundOwn<O>>),
    IntArray(Mut<'a, Vec<byteorder::I32<O>>>),
    LongArray(Mut<'a, Vec<byteorder::I64<O>>>),
}

impl<'a, O: ByteOrder> ValueMut<'a, O> {
    #[inline]
    pub(crate) unsafe fn from_slot(tag_id: TagID, slot: *mut u8) -> Self {
        macro_rules! dispatch {
            ($($t:ident),* $(,)?) => {
                match tag_id {
                    TagID::End => Self::End,
                    $(TagID::$t => Self::$t(unsafe {
                        <tag::$t as ElementTag>::read_elem_mut::<O>(slot)
                    }),)*
                }
            };
        }
        dispatch!(
            Byte, Short, Int, Long, Float, Double, ByteArray, String, List, Compound, IntArray,
            LongArray
        )
    }

    #[inline]
    pub const fn tag_id(&self) -> TagID {
        match self {
            Self::End => TagID::End,
            Self::Byte(_) => TagID::Byte,
            Self::Short(_) => TagID::Short,
            Self::Int(_) => TagID::Int,
            Self::Long(_) => TagID::Long,
            Self::Float(_) => TagID::Float,
            Self::Double(_) => TagID::Double,
            Self::ByteArray(_) => TagID::ByteArray,
            Self::String(_) => TagID::String,
            Self::List(_) => TagID::List,
            Self::Compound(_) => TagID::Compound,
            Self::IntArray(_) => TagID::IntArray,
            Self::LongArray(_) => TagID::LongArray,
        }
    }

    #[inline]
    pub fn is<T: Tag>(&self) -> bool {
        self.as_ref().is::<T>()
    }

    #[inline]
    pub fn as_ref(&self) -> ValueRef<'_, Owned<O>> {
        match self {
            Self::End => ValueRef::End,
            Self::Byte(v) => ValueRef::Byte(**v),
            Self::Short(v) => ValueRef::Short(v.get()),
            Self::Int(v) => ValueRef::Int(v.get()),
            Self::Long(v) => ValueRef::Long(v.get()),
            Self::Float(v) => ValueRef::Float(v.get()),
            Self::Double(v) => ValueRef::Double(v.get()),
            Self::ByteArray(v) => ValueRef::ByteArray(v),
            Self::String(v) => ValueRef::String(v),
            Self::List(v) => ValueRef::List(v.as_ref()),
            Self::Compound(v) => ValueRef::Compound(v.as_ref()),
            Self::IntArray(v) => ValueRef::IntArray(v),
            Self::LongArray(v) => ValueRef::LongArray(v),
        }
    }

    /// Reborrows this mutable value view with a shorter lifetime without consuming `self`.
    #[inline]
    pub fn reborrow(&mut self) -> ValueMut<'_, O> {
        match self {
            Self::End => ValueMut::End,
            Self::Byte(v) => ValueMut::Byte(v),
            Self::Short(v) => ValueMut::Short(ScalarMut::from_native(&mut **v)),
            Self::Int(v) => ValueMut::Int(ScalarMut::from_native(&mut **v)),
            Self::Long(v) => ValueMut::Long(ScalarMut::from_native(&mut **v)),
            Self::Float(v) => ValueMut::Float(ScalarMut::from_native(&mut **v)),
            Self::Double(v) => ValueMut::Double(ScalarMut::from_native(&mut **v)),
            Self::ByteArray(v) => ValueMut::ByteArray(Mut::from_mut(&mut **v)),
            Self::String(v) => ValueMut::String(Mut::from_mut(&mut **v)),
            Self::List(v) => ValueMut::List(Mut::from_mut(&mut **v)),
            Self::Compound(v) => ValueMut::Compound(Mut::from_mut(&mut **v)),
            Self::IntArray(v) => ValueMut::IntArray(Mut::from_mut(&mut **v)),
            Self::LongArray(v) => ValueMut::LongArray(Mut::from_mut(&mut **v)),
        }
    }

    #[inline]
    pub fn as_<T: Tag>(&self) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().as_::<T>()
    }

    #[inline]
    pub fn as_mut_<T: Tag>(&mut self) -> Option<T::Mut<'_, O>> {
        self.reborrow().into_::<T>()
    }

    #[inline]
    pub fn into_<T: Tag>(self) -> Option<T::Mut<'a, O>> {
        T::into_value_mut(self)
    }

    #[inline]
    pub fn get(&self, index: impl Index) -> Option<ValueRef<'_, Owned<O>>> {
        self.as_ref().get(index)
    }

    #[inline]
    pub fn get_as<T: Tag>(&self, index: impl Index) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().get_as::<T>(index)
    }

    #[inline]
    pub fn get_mut(&mut self, index: impl Index) -> Option<ValueMut<'_, O>> {
        index.dispatch(
            self,
            |this, idx| match this {
                Self::List(list) => list.get_mut(idx),
                _ => None,
            },
            |this, key| match this {
                Self::Compound(comp) => comp.get_mut(key),
                _ => None,
            },
        )
    }

    #[inline]
    pub fn get_mut_as<T: Tag>(&mut self, index: impl Index) -> Option<T::Mut<'_, O>> {
        index.dispatch(
            self,
            |this, idx| match this {
                Self::List(list) => list.get_mut_as::<T>(idx),
                _ => None,
            },
            |this, key| match this {
                Self::Compound(comp) => comp.get_mut_as::<T>(key),
                _ => None,
            },
        )
    }
}

impl<'a, O: ByteOrder> From<&'a mut ValueOwn<O>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: &'a mut ValueOwn<O>) -> Self {
        match value {
            ValueOwn::End => Self::End,
            ValueOwn::Byte(v) => Self::Byte(v),
            ValueOwn::Short(v) => Self::Short(ScalarMut::from_native(v)),
            ValueOwn::Int(v) => Self::Int(ScalarMut::from_native(v)),
            ValueOwn::Long(v) => Self::Long(ScalarMut::from_native(v)),
            ValueOwn::Float(v) => Self::Float(ScalarMut::from_native(v)),
            ValueOwn::Double(v) => Self::Double(ScalarMut::from_native(v)),
            ValueOwn::ByteArray(v) => Self::ByteArray(Mut::from_mut(v)),
            ValueOwn::String(v) => Self::String(Mut::from_mut(v)),
            ValueOwn::List(v) => Self::List(Mut::from_mut(v)),
            ValueOwn::Compound(v) => Self::Compound(Mut::from_mut(v)),
            ValueOwn::IntArray(v) => Self::IntArray(Mut::from_mut(v)),
            ValueOwn::LongArray(v) => Self::LongArray(Mut::from_mut(v)),
        }
    }
}

impl<'a, O: ByteOrder> From<&'a mut CompoundOwn<O>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: &'a mut CompoundOwn<O>) -> Self {
        Self::Compound(Mut::from_mut(value))
    }
}

impl<'a, O: ByteOrder> From<Mut<'a, CompoundOwn<O>>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: Mut<'a, CompoundOwn<O>>) -> Self {
        Self::Compound(value)
    }
}

impl<'a, O: ByteOrder> From<&'a mut ListOwn<O>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: &'a mut ListOwn<O>) -> Self {
        Self::List(Mut::from_mut(value))
    }
}

impl<'a, O: ByteOrder> From<Mut<'a, ListOwn<O>>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: Mut<'a, ListOwn<O>>) -> Self {
        Self::List(value)
    }
}

impl<O: ByteOrder> fmt::Debug for ValueMut<'_, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

impl<O: ByteOrder> Writable for ValueMut<'_, O> {
    #[inline]
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        self.as_ref().write_to_vec::<TARGET>()
    }

    #[inline]
    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        self.as_ref().write_to_writer::<TARGET>(writer)
    }
}
