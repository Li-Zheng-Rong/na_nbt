use std::{borrow::Cow, fmt};

use zerocopy::{byteorder, BigEndian};

use crate::{
    index::Index, write::impl_inherent_write_methods, ByteOrder, Compound, CompoundRef, ElementTag,
    IntoNBT, List, ListRef, MUTF8Str, MUTF8String, Mut, Owned, ScalarMut, Tag, TagID, TypedList,
    ValueRef,
};

/// An owned, mutable NBT value.
///
/// Multi-byte scalar variants (`Short`, `Int`, `Long`, `Float`, `Double`) store native Rust
/// primitives directly, while composite and array containers keep target-endian buffers for
/// zero-copy serialization.
#[derive(Clone, Default)]
pub enum Value<O: ByteOrder = BigEndian> {
    /// `TAG_End` (`0`): empty / missing sentinel.
    #[default]
    End,
    /// `TAG_Byte` (`1`): signed 8-bit integer.
    Byte(i8),
    /// `TAG_Short` (`2`): signed 16-bit integer.
    Short(i16),
    /// `TAG_Int` (`3`): signed 32-bit integer.
    Int(i32),
    /// `TAG_Long` (`4`): signed 64-bit integer.
    Long(i64),
    /// `TAG_Float` (`5`): 32-bit IEEE-754 float.
    Float(f32),
    /// `TAG_Double` (`6`): 64-bit IEEE-754 float.
    Double(f64),
    /// `TAG_Byte_Array` (`7`): owned byte array.
    ByteArray(Vec<i8>),
    /// `TAG_String` (`8`): owned MUTF-8 string.
    String(MUTF8String),
    /// `TAG_List` (`9`): owned homogeneous list.
    List(List<O>),
    /// `TAG_Compound` (`10`): owned compound map.
    Compound(Compound<O>),
    /// `TAG_Int_Array` (`11`): owned `i32` array in byte order `O`.
    IntArray(Vec<byteorder::I32<O>>),
    /// `TAG_Long_Array` (`12`): owned `i64` array in byte order `O`.
    LongArray(Vec<byteorder::I64<O>>),
}

macro_rules! impl_value_mut_index_getters {
    ($($fn_name:ident => ($tag:ty, $ret:ty)),* $(,)?) => {
        $(
            #[doc = concat!("Mutably borrows the child at `index` if its tag matches `", stringify!($tag), "`.")]
            #[inline]
            pub fn $fn_name(&mut self, index: impl Index) -> Option<$ret> {
                self.get_mut_as::<$tag>(index)
            }
        )*
    };
}

impl<O: ByteOrder> Value<O> {
    #[inline]
    pub(crate) unsafe fn read_from_slot(tag_id: TagID, src: *const u8) -> Self {
        macro_rules! dispatch {
            ($($variant:ident => $ty:ty),* $(,)?) => {
                match tag_id {
                    TagID::End => Self::End,
                    $(TagID::$variant => Self::$variant(unsafe {
                        <$ty as ElementTag>::read_own_from_slot::<O>(src)
                    }),)*
                }
            };
        }
        dispatch!(
            Byte => i8,
            Short => i16,
            Int => i32,
            Long => i64,
            Float => f32,
            Double => f64,
            ByteArray => [i8],
            String => str,
            List => List<BigEndian>,
            Compound => Compound<BigEndian>,
            IntArray => [i32],
            LongArray => [i64],
        )
    }

    /// Returns the wire [`TagID`] of this value.
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

    /// Returns `true` if this value is [`Value::End`].
    #[inline]
    pub const fn is_end(&self) -> bool {
        matches!(self, Self::End)
    }

    /// Returns `true` if this value matches Rust tag type `T`.
    #[inline]
    pub fn is<T: ?Sized + Tag>(&self) -> bool {
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

    /// Downcasts a shared borrow of this value to `T::Ref`.
    #[inline]
    pub fn as_<T: ?Sized + Tag>(&self) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().as_::<T>()
    }

    /// Downcasts a mutable borrow of this value to `T::Mut`.
    #[inline]
    pub fn as_mut_<T: ?Sized + Tag>(&mut self) -> Option<T::Mut<'_, O>> {
        self.as_mut().into_::<T>()
    }

    /// Consumes this value and extracts `T::Own` if its tag matches `T`.
    #[inline]
    pub fn into_<T: ?Sized + Tag>(self) -> Option<T::Own<O>> {
        T::into_value_own(self)
    }

    /// Interprets a `TAG_Byte` as `bool` (`!= 0`).
    #[inline]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Byte(v) => Some(*v != 0),
            _ => None,
        }
    }

    /// Extracts `i8` if `self` is [`Value::Byte`].
    #[inline]
    pub const fn as_byte(&self) -> Option<i8> {
        match self {
            Self::Byte(v) => Some(*v),
            _ => None,
        }
    }

    /// Extracts `i16` if `self` is [`Value::Short`].
    #[inline]
    pub const fn as_short(&self) -> Option<i16> {
        match self {
            Self::Short(v) => Some(*v),
            _ => None,
        }
    }

    /// Extracts `i32` if `self` is [`Value::Int`].
    #[inline]
    pub const fn as_int(&self) -> Option<i32> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// Extracts `i64` if `self` is [`Value::Long`].
    #[inline]
    pub const fn as_long(&self) -> Option<i64> {
        match self {
            Self::Long(v) => Some(*v),
            _ => None,
        }
    }

    /// Extracts `f32` if `self` is [`Value::Float`].
    #[inline]
    pub const fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Extracts `f64` if `self` is [`Value::Double`].
    #[inline]
    pub const fn as_double(&self) -> Option<f64> {
        match self {
            Self::Double(v) => Some(*v),
            _ => None,
        }
    }

    /// Extracts `&[i8]` if `self` is [`Value::ByteArray`].
    #[inline]
    pub fn as_byte_array(&self) -> Option<&[i8]> {
        match self {
            Self::ByteArray(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts zero-copy `&MUTF8Str` if `self` is [`Value::String`].
    #[inline]
    pub fn as_string(&self) -> Option<&MUTF8Str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    /// Decodes a UTF-8 `Cow<str>` if `self` is [`Value::String`].
    #[inline]
    pub fn as_str(&self) -> Option<Cow<'_, str>> {
        self.as_string().map(MUTF8Str::decode_lossy)
    }

    /// Extracts a [`ListRef`] view if `self` is [`Value::List`].
    #[inline]
    pub fn as_list(&self) -> Option<ListRef<'_, Owned<O>>> {
        match self {
            Self::List(v) => Some(v.as_ref()),
            _ => None,
        }
    }

    /// Extracts a [`CompoundRef`] view if `self` is [`Value::Compound`].
    #[inline]
    pub fn as_compound(&self) -> Option<CompoundRef<'_, Owned<O>>> {
        match self {
            Self::Compound(v) => Some(v.as_ref()),
            _ => None,
        }
    }

    /// Extracts `&[I32<O>]` if `self` is [`Value::IntArray`].
    #[inline]
    pub fn as_int_array(&self) -> Option<&[byteorder::I32<O>]> {
        match self {
            Self::IntArray(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts `&[I64<O>]` if `self` is [`Value::LongArray`].
    #[inline]
    pub fn as_long_array(&self) -> Option<&[byteorder::I64<O>]> {
        match self {
            Self::LongArray(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `i8` if `self` is [`Value::Byte`].
    #[inline]
    pub fn as_byte_mut(&mut self) -> Option<&mut i8> {
        match self {
            Self::Byte(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `i16` if `self` is [`Value::Short`].
    #[inline]
    pub fn as_short_mut(&mut self) -> Option<&mut i16> {
        match self {
            Self::Short(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `i32` if `self` is [`Value::Int`].
    #[inline]
    pub fn as_int_mut(&mut self) -> Option<&mut i32> {
        match self {
            Self::Int(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `i64` if `self` is [`Value::Long`].
    #[inline]
    pub fn as_long_mut(&mut self) -> Option<&mut i64> {
        match self {
            Self::Long(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `f32` if `self` is [`Value::Float`].
    #[inline]
    pub fn as_float_mut(&mut self) -> Option<&mut f32> {
        match self {
            Self::Float(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `f64` if `self` is [`Value::Double`].
    #[inline]
    pub fn as_double_mut(&mut self) -> Option<&mut f64> {
        match self {
            Self::Double(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `Vec<i8>` if `self` is [`Value::ByteArray`].
    #[inline]
    pub fn as_byte_array_mut(&mut self) -> Option<&mut Vec<i8>> {
        match self {
            Self::ByteArray(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner [`MUTF8String`] if `self` is [`Value::String`].
    #[inline]
    pub fn as_string_mut(&mut self) -> Option<&mut MUTF8String> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner [`List`] if `self` is [`Value::List`].
    #[inline]
    pub fn as_list_mut(&mut self) -> Option<&mut List<O>> {
        match self {
            Self::List(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner [`Compound`] if `self` is [`Value::Compound`].
    #[inline]
    pub fn as_compound_mut(&mut self) -> Option<&mut Compound<O>> {
        match self {
            Self::Compound(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `Vec<I32<O>>` if `self` is [`Value::IntArray`].
    #[inline]
    pub fn as_int_array_mut(&mut self) -> Option<&mut Vec<byteorder::I32<O>>> {
        match self {
            Self::IntArray(v) => Some(v),
            _ => None,
        }
    }

    /// Mutably borrows the inner `Vec<I64<O>>` if `self` is [`Value::LongArray`].
    #[inline]
    pub fn as_long_array_mut(&mut self) -> Option<&mut Vec<byteorder::I64<O>>> {
        match self {
            Self::LongArray(v) => Some(v),
            _ => None,
        }
    }

    /// Consumes `self` and returns `Vec<i8>` if [`Value::ByteArray`].
    #[inline]
    pub fn into_byte_array(self) -> Option<Vec<i8>> {
        match self {
            Self::ByteArray(v) => Some(v),
            _ => None,
        }
    }

    /// Consumes `self` and returns [`MUTF8String`] if [`Value::String`].
    #[inline]
    pub fn into_string(self) -> Option<MUTF8String> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    /// Consumes `self` and returns [`List<O>`] if [`Value::List`].
    #[inline]
    pub fn into_list(self) -> Option<List<O>> {
        match self {
            Self::List(v) => Some(v),
            _ => None,
        }
    }

    /// Consumes `self` and returns [`Compound<O>`] if [`Value::Compound`].
    #[inline]
    pub fn into_compound(self) -> Option<Compound<O>> {
        match self {
            Self::Compound(v) => Some(v),
            _ => None,
        }
    }

    /// Consumes `self` and returns `Vec<I32<O>>` if [`Value::IntArray`].
    #[inline]
    pub fn into_int_array(self) -> Option<Vec<byteorder::I32<O>>> {
        match self {
            Self::IntArray(v) => Some(v),
            _ => None,
        }
    }

    /// Consumes `self` and returns `Vec<I64<O>>` if [`Value::LongArray`].
    #[inline]
    pub fn into_long_array(self) -> Option<Vec<byteorder::I64<O>>> {
        match self {
            Self::LongArray(v) => Some(v),
            _ => None,
        }
    }

    /// Navigates into a child by compound key or list index, returning [`ValueRef::End`] if missing.
    #[inline]
    pub fn at(&self, index: impl Index) -> ValueRef<'_, Owned<O>> {
        self.as_ref().at(index)
    }

    /// Looks up a child by compound key or list index.
    #[inline]
    pub fn get(&self, index: impl Index) -> Option<ValueRef<'_, Owned<O>>> {
        self.as_ref().get(index)
    }

    /// Looks up and downcasts a child by compound key or list index to Rust tag type `T`.
    #[inline]
    pub fn get_as<T: ?Sized + Tag>(&self, index: impl Index) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().get_as::<T>(index)
    }

    /// Looks up a `TAG_Byte` child by key or index and interprets it as `bool` (`!= 0`).
    #[inline]
    pub fn get_bool(&self, index: impl Index) -> Option<bool> {
        self.as_ref().get_bool(index)
    }

    /// Looks up a `TAG_Byte` (`i8`) child by key or index.
    #[inline]
    pub fn get_byte(&self, index: impl Index) -> Option<i8> {
        self.as_ref().get_byte(index)
    }

    /// Looks up a `TAG_Short` (`i16`) child by key or index.
    #[inline]
    pub fn get_short(&self, index: impl Index) -> Option<i16> {
        self.as_ref().get_short(index)
    }

    /// Looks up a `TAG_Int` (`i32`) child by key or index.
    #[inline]
    pub fn get_int(&self, index: impl Index) -> Option<i32> {
        self.as_ref().get_int(index)
    }

    /// Looks up a `TAG_Long` (`i64`) child by key or index.
    #[inline]
    pub fn get_long(&self, index: impl Index) -> Option<i64> {
        self.as_ref().get_long(index)
    }

    /// Looks up a `TAG_Float` (`f32`) child by key or index.
    #[inline]
    pub fn get_float(&self, index: impl Index) -> Option<f32> {
        self.as_ref().get_float(index)
    }

    /// Looks up a `TAG_Double` (`f64`) child by key or index.
    #[inline]
    pub fn get_double(&self, index: impl Index) -> Option<f64> {
        self.as_ref().get_double(index)
    }

    /// Looks up a zero-copy `TAG_String` (`&MUTF8Str`) child by key or index.
    #[inline]
    pub fn get_string(&self, index: impl Index) -> Option<&MUTF8Str> {
        self.as_ref().get_string(index)
    }

    /// Looks up a `TAG_String` child by key or index and decodes it to UTF-8 (`Cow<str>`).
    #[inline]
    pub fn get_str(&self, index: impl Index) -> Option<Cow<'_, str>> {
        self.as_ref().get_str(index)
    }

    /// Looks up a `TAG_Byte_Array` (`&[i8]`) child by key or index.
    #[inline]
    pub fn get_byte_array(&self, index: impl Index) -> Option<&[i8]> {
        self.as_ref().get_byte_array(index)
    }

    /// Looks up a `TAG_Int_Array` child by key or index.
    #[inline]
    pub fn get_int_array(&self, index: impl Index) -> Option<&[byteorder::I32<O>]> {
        self.as_ref().get_int_array(index)
    }

    /// Looks up a `TAG_Long_Array` child by key or index.
    #[inline]
    pub fn get_long_array(&self, index: impl Index) -> Option<&[byteorder::I64<O>]> {
        self.as_ref().get_long_array(index)
    }

    /// Looks up a `TAG_List` child by key or index.
    #[inline]
    pub fn get_list(&self, index: impl Index) -> Option<ListRef<'_, Owned<O>>> {
        self.as_ref().get_list(index)
    }

    /// Looks up a `TAG_Compound` child by key or index.
    #[inline]
    pub fn get_compound(&self, index: impl Index) -> Option<CompoundRef<'_, Owned<O>>> {
        self.as_ref().get_compound(index)
    }

    /// Mutably borrows a child by compound key or list index.
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

    /// Mutably borrows and downcasts a child by compound key or list index to Rust tag type `T`.
    #[inline]
    pub fn get_mut_as<T: ?Sized + Tag>(&mut self, index: impl Index) -> Option<T::Mut<'_, O>> {
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

    impl_value_mut_index_getters!(
        get_byte_mut => (i8, &mut i8),
        get_short_mut => (i16, ScalarMut<'_, i16>),
        get_int_mut => (i32, ScalarMut<'_, i32>),
        get_long_mut => (i64, ScalarMut<'_, i64>),
        get_float_mut => (f32, ScalarMut<'_, f32>),
        get_double_mut => (f64, ScalarMut<'_, f64>),
        get_byte_array_mut => ([i8], Mut<'_, Vec<i8>>),
        get_string_mut => (str, Mut<'_, MUTF8String>),
        get_list_mut => (List<O>, Mut<'_, List<O>>),
        get_compound_mut => (Compound<O>, Mut<'_, Compound<O>>),
        get_int_array_mut => ([i32], Mut<'_, Vec<byteorder::I32<O>>>),
        get_long_array_mut => ([i64], Mut<'_, Vec<byteorder::I64<O>>>),
    );

    impl_inherent_write_methods!();
}

impl<O: ByteOrder> fmt::Debug for Value<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

impl<O: ByteOrder> From<()> for Value<O> {
    #[inline]
    fn from((): ()) -> Self {
        Self::End
    }
}

impl<O: ByteOrder> From<bool> for Value<O> {
    #[inline]
    fn from(v: bool) -> Self {
        Self::Byte(i8::from(v))
    }
}

macro_rules! impl_from_value_own {
    ($($ty:ty => $variant:ident),* $(,)?) => {
        $(
            impl<O: ByteOrder> From<$ty> for Value<O> {
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
    List<O> => List,
    Compound<O> => Compound,
    Vec<byteorder::I32<O>> => IntArray,
    Vec<byteorder::I64<O>> => LongArray,
);

macro_rules! impl_from_endian_scalar {
    ($($wrapper:ident => $variant:ident),* $(,)?) => {
        $(
            impl<O: ByteOrder, R: ByteOrder> From<byteorder::$wrapper<R>> for Value<O> {
                #[inline]
                fn from(v: byteorder::$wrapper<R>) -> Self {
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

impl<O: ByteOrder> From<&[i8]> for Value<O> {
    #[inline]
    fn from(v: &[i8]) -> Self {
        Self::ByteArray(v.to_vec())
    }
}

impl<O: ByteOrder, const N: usize> From<[i8; N]> for Value<O> {
    #[inline]
    fn from(v: [i8; N]) -> Self {
        Self::ByteArray(v.to_vec())
    }
}

impl<O: ByteOrder> From<&MUTF8Str> for Value<O> {
    #[inline]
    fn from(v: &MUTF8Str) -> Self {
        Self::String(MUTF8String::from(v))
    }
}

impl<O: ByteOrder> From<&str> for Value<O> {
    #[inline]
    fn from(v: &str) -> Self {
        Self::String(MUTF8String::from(v))
    }
}

impl<O: ByteOrder> From<String> for Value<O> {
    #[inline]
    fn from(v: String) -> Self {
        Self::String(MUTF8String::from(v))
    }
}

impl<O: ByteOrder> From<Cow<'_, str>> for Value<O> {
    #[inline]
    fn from(v: Cow<'_, str>) -> Self {
        Self::String(MUTF8String::from(v))
    }
}

impl<O: ByteOrder> From<&[i32]> for Value<O> {
    #[inline]
    fn from(v: &[i32]) -> Self {
        Self::IntArray(IntoNBT::<O>::into_nbt(v))
    }
}

impl<O: ByteOrder> From<Vec<i32>> for Value<O> {
    #[inline]
    fn from(v: Vec<i32>) -> Self {
        Self::IntArray(IntoNBT::<O>::into_nbt(v))
    }
}

impl<O: ByteOrder> From<&[i64]> for Value<O> {
    #[inline]
    fn from(v: &[i64]) -> Self {
        Self::LongArray(IntoNBT::<O>::into_nbt(v))
    }
}

impl<O: ByteOrder> From<Vec<i64>> for Value<O> {
    #[inline]
    fn from(v: Vec<i64>) -> Self {
        Self::LongArray(IntoNBT::<O>::into_nbt(v))
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> From<TypedList<T, O>> for Value<O> {
    #[inline]
    fn from(v: TypedList<T, O>) -> Self {
        Self::List(List::from(v))
    }
}

/// A mutable view of an NBT value inside a [`Compound`], [`List`], or [`Value`].
#[derive(Default)]
pub enum ValueMut<'a, O: ByteOrder = BigEndian> {
    /// `TAG_End` (`0`): empty / missing sentinel.
    #[default]
    End,
    /// `TAG_Byte` (`1`): mutable `i8` reference.
    Byte(&'a mut i8),
    /// `TAG_Short` (`2`): RAII mutable `i16` guard.
    Short(ScalarMut<'a, i16>),
    /// `TAG_Int` (`3`): RAII mutable `i32` guard.
    Int(ScalarMut<'a, i32>),
    /// `TAG_Long` (`4`): RAII mutable `i64` guard.
    Long(ScalarMut<'a, i64>),
    /// `TAG_Float` (`5`): RAII mutable `f32` guard.
    Float(ScalarMut<'a, f32>),
    /// `TAG_Double` (`6`): RAII mutable `f64` guard.
    Double(ScalarMut<'a, f64>),
    /// `TAG_Byte_Array` (`7`): RAII mutable `Vec<i8>` slot guard.
    ByteArray(Mut<'a, Vec<i8>>),
    /// `TAG_String` (`8`): RAII mutable [`MUTF8String`] slot guard.
    String(Mut<'a, MUTF8String>),
    /// `TAG_List` (`9`): RAII mutable [`List`] slot guard.
    List(Mut<'a, List<O>>),
    /// `TAG_Compound` (`10`): RAII mutable [`Compound`] slot guard.
    Compound(Mut<'a, Compound<O>>),
    /// `TAG_Int_Array` (`11`): RAII mutable `Vec<I32<O>>` slot guard.
    IntArray(Mut<'a, Vec<byteorder::I32<O>>>),
    /// `TAG_Long_Array` (`12`): RAII mutable `Vec<I64<O>>` slot guard.
    LongArray(Mut<'a, Vec<byteorder::I64<O>>>),
}

impl<'a, O: ByteOrder> ValueMut<'a, O> {
    #[inline]
    pub(crate) unsafe fn from_slot(tag_id: TagID, slot: *mut u8) -> Self {
        macro_rules! dispatch {
            ($($variant:ident => $ty:ty),* $(,)?) => {
                match tag_id {
                    TagID::End => Self::End,
                    $(TagID::$variant => Self::$variant(unsafe {
                        <$ty as ElementTag>::read_elem_mut::<O>(slot)
                    }),)*
                }
            };
        }
        dispatch!(
            Byte => i8,
            Short => i16,
            Int => i32,
            Long => i64,
            Float => f32,
            Double => f64,
            ByteArray => [i8],
            String => str,
            List => List<BigEndian>,
            Compound => Compound<BigEndian>,
            IntArray => [i32],
            LongArray => [i64],
        )
    }

    /// Returns the wire [`TagID`] of this value.
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

    /// Returns `true` if this value is [`ValueMut::End`].
    #[inline]
    pub const fn is_end(&self) -> bool {
        matches!(self, Self::End)
    }

    /// Returns `true` if this value matches Rust tag type `T`.
    #[inline]
    pub fn is<T: ?Sized + Tag>(&self) -> bool {
        self.as_ref().is::<T>()
    }

    /// Returns a lightweight `Copy` read-only view of this value.
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

    /// Downcasts a shared borrow of this value to `T::Ref`.
    #[inline]
    pub fn as_<T: ?Sized + Tag>(&self) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().as_::<T>()
    }

    /// Reborrows and downcasts this mutable view to `T::Mut`.
    #[inline]
    pub fn as_mut_<T: ?Sized + Tag>(&mut self) -> Option<T::Mut<'_, O>> {
        self.reborrow().into_::<T>()
    }

    /// Consumes this mutable view and extracts `T::Mut`.
    #[inline]
    pub fn into_<T: ?Sized + Tag>(self) -> Option<T::Mut<'a, O>> {
        T::into_value_mut(self)
    }

    /// Interprets a `TAG_Byte` as `bool` (`!= 0`).
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        self.as_ref().as_bool()
    }

    /// Extracts `i8` if `self` is [`ValueMut::Byte`].
    #[inline]
    pub fn as_byte(&self) -> Option<i8> {
        self.as_ref().as_byte()
    }

    /// Extracts `i16` if `self` is [`ValueMut::Short`].
    #[inline]
    pub fn as_short(&self) -> Option<i16> {
        self.as_ref().as_short()
    }

    /// Extracts `i32` if `self` is [`ValueMut::Int`].
    #[inline]
    pub fn as_int(&self) -> Option<i32> {
        self.as_ref().as_int()
    }

    /// Extracts `i64` if `self` is [`ValueMut::Long`].
    #[inline]
    pub fn as_long(&self) -> Option<i64> {
        self.as_ref().as_long()
    }

    /// Extracts `f32` if `self` is [`ValueMut::Float`].
    #[inline]
    pub fn as_float(&self) -> Option<f32> {
        self.as_ref().as_float()
    }

    /// Extracts `f64` if `self` is [`ValueMut::Double`].
    #[inline]
    pub fn as_double(&self) -> Option<f64> {
        self.as_ref().as_double()
    }

    /// Extracts `&[i8]` if `self` is [`ValueMut::ByteArray`].
    #[inline]
    pub fn as_byte_array(&self) -> Option<&[i8]> {
        self.as_ref().as_byte_array()
    }

    /// Extracts zero-copy `&MUTF8Str` if `self` is [`ValueMut::String`].
    #[inline]
    pub fn as_string(&self) -> Option<&MUTF8Str> {
        self.as_ref().as_string()
    }

    /// Decodes a UTF-8 `Cow<str>` if `self` is [`ValueMut::String`].
    #[inline]
    pub fn as_str(&self) -> Option<Cow<'_, str>> {
        self.as_ref().as_str()
    }

    /// Extracts a [`ListRef`] view if `self` is [`ValueMut::List`].
    #[inline]
    pub fn as_list(&self) -> Option<ListRef<'_, Owned<O>>> {
        self.as_ref().as_list()
    }

    /// Extracts a [`CompoundRef`] view if `self` is [`ValueMut::Compound`].
    #[inline]
    pub fn as_compound(&self) -> Option<CompoundRef<'_, Owned<O>>> {
        self.as_ref().as_compound()
    }

    /// Extracts `&[I32<O>]` if `self` is [`ValueMut::IntArray`].
    #[inline]
    pub fn as_int_array(&self) -> Option<&[byteorder::I32<O>]> {
        self.as_ref().as_int_array()
    }

    /// Extracts `&[I64<O>]` if `self` is [`ValueMut::LongArray`].
    #[inline]
    pub fn as_long_array(&self) -> Option<&[byteorder::I64<O>]> {
        self.as_ref().as_long_array()
    }

    /// Reborrows `&mut i8` if `self` is [`ValueMut::Byte`].
    #[inline]
    pub fn as_byte_mut(&mut self) -> Option<&mut i8> {
        self.reborrow().into_byte_mut()
    }

    /// Reborrows [`ScalarMut<i16>`] if `self` is [`ValueMut::Short`].
    #[inline]
    pub fn as_short_mut(&mut self) -> Option<ScalarMut<'_, i16>> {
        self.reborrow().into_short_mut()
    }

    /// Reborrows [`ScalarMut<i32>`] if `self` is [`ValueMut::Int`].
    #[inline]
    pub fn as_int_mut(&mut self) -> Option<ScalarMut<'_, i32>> {
        self.reborrow().into_int_mut()
    }

    /// Reborrows [`ScalarMut<i64>`] if `self` is [`ValueMut::Long`].
    #[inline]
    pub fn as_long_mut(&mut self) -> Option<ScalarMut<'_, i64>> {
        self.reborrow().into_long_mut()
    }

    /// Reborrows [`ScalarMut<f32>`] if `self` is [`ValueMut::Float`].
    #[inline]
    pub fn as_float_mut(&mut self) -> Option<ScalarMut<'_, f32>> {
        self.reborrow().into_float_mut()
    }

    /// Reborrows [`ScalarMut<f64>`] if `self` is [`ValueMut::Double`].
    #[inline]
    pub fn as_double_mut(&mut self) -> Option<ScalarMut<'_, f64>> {
        self.reborrow().into_double_mut()
    }

    /// Reborrows [`Mut<Vec<i8>>`] if `self` is [`ValueMut::ByteArray`].
    #[inline]
    pub fn as_byte_array_mut(&mut self) -> Option<Mut<'_, Vec<i8>>> {
        self.reborrow().into_byte_array_mut()
    }

    /// Reborrows [`Mut<MUTF8String>`] if `self` is [`ValueMut::String`].
    #[inline]
    pub fn as_string_mut(&mut self) -> Option<Mut<'_, MUTF8String>> {
        self.reborrow().into_string_mut()
    }

    /// Reborrows [`Mut<List<O>>`] if `self` is [`ValueMut::List`].
    #[inline]
    pub fn as_list_mut(&mut self) -> Option<Mut<'_, List<O>>> {
        self.reborrow().into_list_mut()
    }

    /// Reborrows [`Mut<Compound<O>>`] if `self` is [`ValueMut::Compound`].
    #[inline]
    pub fn as_compound_mut(&mut self) -> Option<Mut<'_, Compound<O>>> {
        self.reborrow().into_compound_mut()
    }

    /// Reborrows [`Mut<Vec<I32<O>>>`] if `self` is [`ValueMut::IntArray`].
    #[inline]
    pub fn as_int_array_mut(&mut self) -> Option<Mut<'_, Vec<byteorder::I32<O>>>> {
        self.reborrow().into_int_array_mut()
    }

    /// Reborrows [`Mut<Vec<I64<O>>>`] if `self` is [`ValueMut::LongArray`].
    #[inline]
    pub fn as_long_array_mut(&mut self) -> Option<Mut<'_, Vec<byteorder::I64<O>>>> {
        self.reborrow().into_long_array_mut()
    }

    /// Consumes this mutable view and returns `&'a mut i8` if `Byte`.
    #[inline]
    pub fn into_byte_mut(self) -> Option<&'a mut i8> {
        self.into_::<i8>()
    }

    /// Consumes this mutable view and returns [`ScalarMut<i16>`] if `Short`.
    #[inline]
    pub fn into_short_mut(self) -> Option<ScalarMut<'a, i16>> {
        self.into_::<i16>()
    }

    /// Consumes this mutable view and returns [`ScalarMut<i32>`] if `Int`.
    #[inline]
    pub fn into_int_mut(self) -> Option<ScalarMut<'a, i32>> {
        self.into_::<i32>()
    }

    /// Consumes this mutable view and returns [`ScalarMut<i64>`] if `Long`.
    #[inline]
    pub fn into_long_mut(self) -> Option<ScalarMut<'a, i64>> {
        self.into_::<i64>()
    }

    /// Consumes this mutable view and returns [`ScalarMut<f32>`] if `Float`.
    #[inline]
    pub fn into_float_mut(self) -> Option<ScalarMut<'a, f32>> {
        self.into_::<f32>()
    }

    /// Consumes this mutable view and returns [`ScalarMut<f64>`] if `Double`.
    #[inline]
    pub fn into_double_mut(self) -> Option<ScalarMut<'a, f64>> {
        self.into_::<f64>()
    }

    /// Consumes this mutable view and returns [`Mut<Vec<i8>>`] if `ByteArray`.
    #[inline]
    pub fn into_byte_array_mut(self) -> Option<Mut<'a, Vec<i8>>> {
        self.into_::<[i8]>()
    }

    /// Consumes this mutable view and returns [`Mut<MUTF8String>`] if `String`.
    #[inline]
    pub fn into_string_mut(self) -> Option<Mut<'a, MUTF8String>> {
        self.into_::<str>()
    }

    /// Consumes this mutable view and returns [`Mut<List<O>>`] if `List`.
    #[inline]
    pub fn into_list_mut(self) -> Option<Mut<'a, List<O>>> {
        self.into_::<List<O>>()
    }

    /// Consumes this mutable view and returns [`Mut<Compound<O>>`] if `Compound`.
    #[inline]
    pub fn into_compound_mut(self) -> Option<Mut<'a, Compound<O>>> {
        self.into_::<Compound<O>>()
    }

    /// Consumes this mutable view and returns [`Mut<Vec<I32<O>>>`] if `IntArray`.
    #[inline]
    pub fn into_int_array_mut(self) -> Option<Mut<'a, Vec<byteorder::I32<O>>>> {
        self.into_::<[i32]>()
    }

    /// Consumes this mutable view and returns [`Mut<Vec<I64<O>>>`] if `LongArray`.
    #[inline]
    pub fn into_long_array_mut(self) -> Option<Mut<'a, Vec<byteorder::I64<O>>>> {
        self.into_::<[i64]>()
    }

    /// Navigates into a child by compound key or list index, returning [`ValueRef::End`] if missing.
    #[inline]
    pub fn at(&self, index: impl Index) -> ValueRef<'_, Owned<O>> {
        self.as_ref().at(index)
    }

    /// Looks up a child by compound key or list index.
    #[inline]
    pub fn get(&self, index: impl Index) -> Option<ValueRef<'_, Owned<O>>> {
        self.as_ref().get(index)
    }

    /// Looks up and downcasts a child by compound key or list index to Rust tag type `T`.
    #[inline]
    pub fn get_as<T: ?Sized + Tag>(&self, index: impl Index) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().get_as::<T>(index)
    }

    /// Looks up a `TAG_Byte` child by key or index and interprets it as `bool` (`!= 0`).
    #[inline]
    pub fn get_bool(&self, index: impl Index) -> Option<bool> {
        self.as_ref().get_bool(index)
    }

    /// Looks up a `TAG_Byte` (`i8`) child by key or index.
    #[inline]
    pub fn get_byte(&self, index: impl Index) -> Option<i8> {
        self.as_ref().get_byte(index)
    }

    /// Looks up a `TAG_Short` (`i16`) child by key or index.
    #[inline]
    pub fn get_short(&self, index: impl Index) -> Option<i16> {
        self.as_ref().get_short(index)
    }

    /// Looks up a `TAG_Int` (`i32`) child by key or index.
    #[inline]
    pub fn get_int(&self, index: impl Index) -> Option<i32> {
        self.as_ref().get_int(index)
    }

    /// Looks up a `TAG_Long` (`i64`) child by key or index.
    #[inline]
    pub fn get_long(&self, index: impl Index) -> Option<i64> {
        self.as_ref().get_long(index)
    }

    /// Looks up a `TAG_Float` (`f32`) child by key or index.
    #[inline]
    pub fn get_float(&self, index: impl Index) -> Option<f32> {
        self.as_ref().get_float(index)
    }

    /// Looks up a `TAG_Double` (`f64`) child by key or index.
    #[inline]
    pub fn get_double(&self, index: impl Index) -> Option<f64> {
        self.as_ref().get_double(index)
    }

    /// Looks up a zero-copy `TAG_String` (`&MUTF8Str`) child by key or index.
    #[inline]
    pub fn get_string(&self, index: impl Index) -> Option<&MUTF8Str> {
        self.as_ref().get_string(index)
    }

    /// Looks up a `TAG_String` child by key or index and decodes it to UTF-8 (`Cow<str>`).
    #[inline]
    pub fn get_str(&self, index: impl Index) -> Option<Cow<'_, str>> {
        self.as_ref().get_str(index)
    }

    /// Looks up a `TAG_Byte_Array` (`&[i8]`) child by key or index.
    #[inline]
    pub fn get_byte_array(&self, index: impl Index) -> Option<&[i8]> {
        self.as_ref().get_byte_array(index)
    }

    /// Looks up a `TAG_Int_Array` child by key or index.
    #[inline]
    pub fn get_int_array(&self, index: impl Index) -> Option<&[byteorder::I32<O>]> {
        self.as_ref().get_int_array(index)
    }

    /// Looks up a `TAG_Long_Array` child by key or index.
    #[inline]
    pub fn get_long_array(&self, index: impl Index) -> Option<&[byteorder::I64<O>]> {
        self.as_ref().get_long_array(index)
    }

    /// Looks up a `TAG_List` child by key or index.
    #[inline]
    pub fn get_list(&self, index: impl Index) -> Option<ListRef<'_, Owned<O>>> {
        self.as_ref().get_list(index)
    }

    /// Looks up a `TAG_Compound` child by key or index.
    #[inline]
    pub fn get_compound(&self, index: impl Index) -> Option<CompoundRef<'_, Owned<O>>> {
        self.as_ref().get_compound(index)
    }

    /// Mutably borrows a child by compound key or list index.
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

    /// Mutably borrows and downcasts a child by compound key or list index to Rust tag type `T`.
    #[inline]
    pub fn get_mut_as<T: ?Sized + Tag>(&mut self, index: impl Index) -> Option<T::Mut<'_, O>> {
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

    impl_value_mut_index_getters!(
        get_byte_mut => (i8, &mut i8),
        get_short_mut => (i16, ScalarMut<'_, i16>),
        get_int_mut => (i32, ScalarMut<'_, i32>),
        get_long_mut => (i64, ScalarMut<'_, i64>),
        get_float_mut => (f32, ScalarMut<'_, f32>),
        get_double_mut => (f64, ScalarMut<'_, f64>),
        get_byte_array_mut => ([i8], Mut<'_, Vec<i8>>),
        get_string_mut => (str, Mut<'_, MUTF8String>),
        get_list_mut => (List<O>, Mut<'_, List<O>>),
        get_compound_mut => (Compound<O>, Mut<'_, Compound<O>>),
        get_int_array_mut => ([i32], Mut<'_, Vec<byteorder::I32<O>>>),
        get_long_array_mut => ([i64], Mut<'_, Vec<byteorder::I64<O>>>),
    );

    impl_inherent_write_methods!();
}

impl<'a, O: ByteOrder> From<&'a mut Value<O>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: &'a mut Value<O>) -> Self {
        match value {
            Value::End => Self::End,
            Value::Byte(v) => Self::Byte(v),
            Value::Short(v) => Self::Short(ScalarMut::from_native(v)),
            Value::Int(v) => Self::Int(ScalarMut::from_native(v)),
            Value::Long(v) => Self::Long(ScalarMut::from_native(v)),
            Value::Float(v) => Self::Float(ScalarMut::from_native(v)),
            Value::Double(v) => Self::Double(ScalarMut::from_native(v)),
            Value::ByteArray(v) => Self::ByteArray(Mut::from_mut(v)),
            Value::String(v) => Self::String(Mut::from_mut(v)),
            Value::List(v) => Self::List(Mut::from_mut(v)),
            Value::Compound(v) => Self::Compound(Mut::from_mut(v)),
            Value::IntArray(v) => Self::IntArray(Mut::from_mut(v)),
            Value::LongArray(v) => Self::LongArray(Mut::from_mut(v)),
        }
    }
}

impl<'a, O: ByteOrder> From<&'a mut Compound<O>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: &'a mut Compound<O>) -> Self {
        Self::Compound(Mut::from_mut(value))
    }
}

impl<'a, O: ByteOrder> From<Mut<'a, Compound<O>>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: Mut<'a, Compound<O>>) -> Self {
        Self::Compound(value)
    }
}

impl<'a, O: ByteOrder> From<&'a mut List<O>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: &'a mut List<O>) -> Self {
        Self::List(Mut::from_mut(value))
    }
}

impl<'a, O: ByteOrder> From<Mut<'a, List<O>>> for ValueMut<'a, O> {
    #[inline]
    fn from(value: Mut<'a, List<O>>) -> Self {
        Self::List(value)
    }
}

impl<O: ByteOrder> fmt::Debug for ValueMut<'_, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

impl<O: ByteOrder> PartialEq for Value<O> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<O: ByteOrder, S: crate::Source> PartialEq<ValueRef<'_, S>> for Value<O> {
    #[inline]
    fn eq(&self, other: &ValueRef<'_, S>) -> bool {
        self.as_ref() == *other
    }
}

impl<O: ByteOrder, B: AsRef<[u8]>> PartialEq<crate::Document<O, B>> for Value<O> {
    #[inline]
    fn eq(&self, other: &crate::Document<O, B>) -> bool {
        self.as_ref() == other.root()
    }
}

impl<O: ByteOrder> PartialEq<ValueMut<'_, O>> for Value<O> {
    #[inline]
    fn eq(&self, other: &ValueMut<'_, O>) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<O: ByteOrder> PartialEq for ValueMut<'_, O> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<O: ByteOrder> PartialEq<Value<O>> for ValueMut<'_, O> {
    #[inline]
    fn eq(&self, other: &Value<O>) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<O: ByteOrder, S: crate::Source> PartialEq<ValueRef<'_, S>> for ValueMut<'_, O> {
    #[inline]
    fn eq(&self, other: &ValueRef<'_, S>) -> bool {
        self.as_ref() == *other
    }
}
