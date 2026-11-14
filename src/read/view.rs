use std::{fmt, marker::PhantomData, slice};

use zerocopy::{byteorder, BigEndian};

use crate::{
    cold_path,
    index::{AsKey, Index},
    owned::into_nbt::IntoNBT,
    write::impl_inherent_write_methods,
    Borrowed, ByteOrder, Compound, Document, ElementTag, List, MUTF8Str, Mut, Owned, Source, Tag,
    TagID, TypedList, Value, ValueMut,
};

/// A lightweight `Copy` read-only view of any NBT value (borrowed or owned).
///
/// Inside [`CompoundRef`] and [`ListRef`], `TagID::End` cannot appear as an entry, so
/// [`ValueRef::End`] doubles as a zero-cost null sentinel for chainable [`ValueRef::at`] lookups.
pub enum ValueRef<'a, S: Source = Borrowed<BigEndian>> {
    /// `TAG_End` (`0`) or a missing path lookup result from [`ValueRef::at`].
    End,
    /// `TAG_Byte` (`1`).
    Byte(i8),
    /// `TAG_Short` (`2`).
    Short(i16),
    /// `TAG_Int` (`3`).
    Int(i32),
    /// `TAG_Long` (`4`).
    Long(i64),
    /// `TAG_Float` (`5`).
    Float(f32),
    /// `TAG_Double` (`6`).
    Double(f64),
    /// `TAG_Byte_Array` (`7`).
    ByteArray(&'a [i8]),
    /// `TAG_String` (`8`).
    String(&'a MUTF8Str),
    /// `TAG_List` (`9`).
    List(ListRef<'a, S>),
    /// `TAG_Compound` (`10`).
    Compound(CompoundRef<'a, S>),
    /// `TAG_Int_Array` (`11`).
    IntArray(&'a [byteorder::I32<S::Order>]),
    /// `TAG_Long_Array` (`12`).
    LongArray(&'a [byteorder::I64<S::Order>]),
}

impl<S: Source> Copy for ValueRef<'_, S> {}
impl<S: Source> Clone for ValueRef<'_, S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Source> Default for ValueRef<'_, S> {
    #[inline]
    fn default() -> Self {
        Self::End
    }
}

#[inline(always)]
pub(crate) unsafe fn read_value_advance<'a, S: Source>(
    tag_id: TagID,
    iter: &mut S::IterCursor,
) -> ValueRef<'a, S> {
    macro_rules! dispatch {
        ($($variant:ident => $ty:ty),* $(,)?) => {
            match tag_id {
                TagID::End => ValueRef::End,
                $(TagID::$variant => ValueRef::$variant(unsafe {
                    <$ty as ElementTag>::read_elem_ref::<S>(iter)
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

impl<'a, S: Source> ValueRef<'a, S> {
    /// Returns the [`TagID`] of this value.
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

    /// Returns `true` if this value is [`ValueRef::End`].
    #[inline]
    pub const fn is_end(&self) -> bool {
        matches!(self, Self::End)
    }

    /// Returns `true` if this value matches Rust tag type `T` (including [`TypedList<E>`]).
    #[inline]
    pub fn is<T: ?Sized + Tag>(&self) -> bool {
        T::from_value_ref(*self).is_some()
    }

    /// Downcasts this `Copy` value view to Rust tag type `T` (including [`TypedList<E>`]).
    #[inline]
    pub fn as_<T: ?Sized + Tag>(self) -> Option<T::Ref<'a, S>> {
        T::from_value_ref(self)
    }

    /// Extracts a boolean (`TAG_Byte != 0`) if this value is `ValueRef::Byte`.
    #[inline]
    pub const fn as_bool(self) -> Option<bool> {
        match self {
            Self::Byte(v) => Some(v != 0),
            _ => None,
        }
    }

    /// Extracts `i8` if this value is `ValueRef::Byte`.
    #[inline]
    pub const fn as_byte(self) -> Option<i8> {
        match self {
            Self::Byte(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts `i16` if this value is `ValueRef::Short`.
    #[inline]
    pub const fn as_short(self) -> Option<i16> {
        match self {
            Self::Short(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts `i32` if this value is `ValueRef::Int`.
    #[inline]
    pub const fn as_int(self) -> Option<i32> {
        match self {
            Self::Int(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts `i64` if this value is `ValueRef::Long`.
    #[inline]
    pub const fn as_long(self) -> Option<i64> {
        match self {
            Self::Long(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts `f32` if this value is `ValueRef::Float`.
    #[inline]
    pub const fn as_float(self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts `f64` if this value is `ValueRef::Double`.
    #[inline]
    pub const fn as_double(self) -> Option<f64> {
        match self {
            Self::Double(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts `&[i8]` if this value is `ValueRef::ByteArray`.
    #[inline]
    pub const fn as_byte_array(self) -> Option<&'a [i8]> {
        match self {
            Self::ByteArray(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts zero-copy `&MUTF8Str` if this value is `ValueRef::String`.
    #[inline]
    pub const fn as_string(self) -> Option<&'a MUTF8Str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    /// Decodes a UTF-8 `Cow<str>` if this value is `ValueRef::String`.
    #[inline]
    pub fn as_str(self) -> Option<std::borrow::Cow<'a, str>> {
        self.as_string().map(MUTF8Str::decode_lossy)
    }

    /// Extracts `&[I32<S::Order>]` if this value is `ValueRef::IntArray`.
    #[inline]
    pub const fn as_int_array(self) -> Option<&'a [byteorder::I32<S::Order>]> {
        match self {
            Self::IntArray(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts `&[I64<S::Order>]` if this value is `ValueRef::LongArray`.
    #[inline]
    pub const fn as_long_array(self) -> Option<&'a [byteorder::I64<S::Order>]> {
        match self {
            Self::LongArray(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts [`ListRef`] if this value is `ValueRef::List`.
    #[inline]
    pub const fn as_list(self) -> Option<ListRef<'a, S>> {
        match self {
            Self::List(v) => Some(v),
            _ => None,
        }
    }

    /// Extracts [`CompoundRef`] if this value is `ValueRef::Compound`.
    #[inline]
    pub const fn as_compound(self) -> Option<CompoundRef<'a, S>> {
        match self {
            Self::Compound(v) => Some(v),
            _ => None,
        }
    }

    /// Chainable child navigation by compound key or list index, returning [`ValueRef::End`] when
    /// missing or when `self` is not a matching container.
    #[inline]
    pub fn at(&self, index: impl Index) -> ValueRef<'a, S> {
        self.get(index).unwrap_or(ValueRef::End)
    }

    /// Looks up a child by compound key or list index.
    #[inline]
    pub fn get(&self, index: impl Index) -> Option<ValueRef<'a, S>> {
        index.dispatch(
            *self,
            |v, idx| match v {
                Self::List(list) => list.get(idx),
                _ => None,
            },
            |v, key| match v {
                Self::Compound(comp) => comp.get_mutf8(key),
                _ => None,
            },
        )
    }

    /// Looks up and downcasts a child by compound key or list index to Rust tag type `T`.
    #[inline]
    pub fn get_as<T: ?Sized + Tag>(&self, index: impl Index) -> Option<T::Ref<'a, S>> {
        index.dispatch(
            *self,
            |v, idx| match v {
                Self::List(list) => list.get_as::<T>(idx),
                _ => None,
            },
            |v, key| match v {
                Self::Compound(comp) => comp.get_as_mutf8::<T>(key),
                _ => None,
            },
        )
    }

    /// Looks up a `TAG_Byte` child by key or index and interprets it as `bool` (`!= 0`).
    #[inline]
    pub fn get_bool(&self, index: impl Index) -> Option<bool> {
        self.get_as::<i8>(index).map(|v| v != 0)
    }

    /// Looks up a `TAG_Byte` (`i8`) child by key or index.
    #[inline]
    pub fn get_byte(&self, index: impl Index) -> Option<i8> {
        self.get_as::<i8>(index)
    }

    /// Looks up a `TAG_Short` (`i16`) child by key or index.
    #[inline]
    pub fn get_short(&self, index: impl Index) -> Option<i16> {
        self.get_as::<i16>(index)
    }

    /// Looks up a `TAG_Int` (`i32`) child by key or index.
    #[inline]
    pub fn get_int(&self, index: impl Index) -> Option<i32> {
        self.get_as::<i32>(index)
    }

    /// Looks up a `TAG_Long` (`i64`) child by key or index.
    #[inline]
    pub fn get_long(&self, index: impl Index) -> Option<i64> {
        self.get_as::<i64>(index)
    }

    /// Looks up a `TAG_Float` (`f32`) child by key or index.
    #[inline]
    pub fn get_float(&self, index: impl Index) -> Option<f32> {
        self.get_as::<f32>(index)
    }

    /// Looks up a `TAG_Double` (`f64`) child by key or index.
    #[inline]
    pub fn get_double(&self, index: impl Index) -> Option<f64> {
        self.get_as::<f64>(index)
    }

    /// Looks up a zero-copy `TAG_String` (`&MUTF8Str`) child by key or index.
    #[inline]
    pub fn get_string(&self, index: impl Index) -> Option<&'a MUTF8Str> {
        self.get_as::<str>(index)
    }

    /// Looks up a `TAG_String` child by key or index and decodes it to UTF-8 (`Cow<str>`).
    #[inline]
    pub fn get_str(&self, index: impl Index) -> Option<std::borrow::Cow<'a, str>> {
        self.get_string(index).map(MUTF8Str::decode_lossy)
    }

    /// Looks up a `TAG_Byte_Array` (`&[i8]`) child by key or index.
    #[inline]
    pub fn get_byte_array(&self, index: impl Index) -> Option<&'a [i8]> {
        self.get_as::<[i8]>(index)
    }

    /// Looks up a `TAG_Int_Array` child by key or index.
    #[inline]
    pub fn get_int_array(&self, index: impl Index) -> Option<&'a [byteorder::I32<S::Order>]> {
        self.get_as::<[i32]>(index)
    }

    /// Looks up a `TAG_Long_Array` child by key or index.
    #[inline]
    pub fn get_long_array(&self, index: impl Index) -> Option<&'a [byteorder::I64<S::Order>]> {
        self.get_as::<[i64]>(index)
    }

    /// Looks up a `TAG_List` child by key or index.
    #[inline]
    pub fn get_list(&self, index: impl Index) -> Option<ListRef<'a, S>> {
        self.get_as::<List<BigEndian>>(index)
    }

    /// Looks up a `TAG_Compound` child by key or index.
    #[inline]
    pub fn get_compound(&self, index: impl Index) -> Option<CompoundRef<'a, S>> {
        self.get_as::<Compound<BigEndian>>(index)
    }

    /// Clones this read-only view into an owned [`Value<TARGET>`].
    pub fn to_owned_value<TARGET: ByteOrder>(&self) -> Value<TARGET> {
        match *self {
            Self::End => Value::End,
            Self::Byte(v) => Value::Byte(v),
            Self::Short(v) => Value::Short(v),
            Self::Int(v) => Value::Int(v),
            Self::Long(v) => Value::Long(v),
            Self::Float(v) => Value::Float(v),
            Self::Double(v) => Value::Double(v),
            Self::ByteArray(v) => Value::ByteArray(v.to_vec()),
            Self::String(v) => Value::String(v.to_owned()),
            Self::IntArray(v) => Value::IntArray(IntoNBT::<TARGET>::into_nbt(v)),
            Self::LongArray(v) => Value::LongArray(IntoNBT::<TARGET>::into_nbt(v)),
            Self::List(v) => Value::List(v.to_owned_list::<TARGET>()),
            Self::Compound(v) => Value::Compound(v.to_owned_compound::<TARGET>()),
        }
    }

    impl_inherent_write_methods!();
}

impl<S: Source> fmt::Debug for ValueRef<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::End => f.write_str("End"),
            Self::Byte(v) => f.debug_tuple("Byte").field(v).finish(),
            Self::Short(v) => f.debug_tuple("Short").field(v).finish(),
            Self::Int(v) => f.debug_tuple("Int").field(v).finish(),
            Self::Long(v) => f.debug_tuple("Long").field(v).finish(),
            Self::Float(v) => f.debug_tuple("Float").field(v).finish(),
            Self::Double(v) => f.debug_tuple("Double").field(v).finish(),
            Self::ByteArray(v) => f.debug_tuple("ByteArray").field(v).finish(),
            Self::String(v) => f.debug_tuple("String").field(v).finish(),
            Self::List(v) => f.debug_tuple("List").field(v).finish(),
            Self::Compound(v) => f.debug_tuple("Compound").field(v).finish(),
            Self::IntArray(v) => f.debug_tuple("IntArray").field(&v.len()).finish(),
            Self::LongArray(v) => f.debug_tuple("LongArray").field(&v.len()).finish(),
        }
    }
}

/// A lightweight `Copy` read-only view of an NBT compound.
pub struct CompoundRef<'a, S: Source = Borrowed<BigEndian>> {
    pub(crate) cursor: S::CompoundCursor<'a>,
}

impl<S: Source> Copy for CompoundRef<'_, S> {}
impl<S: Source> Clone for CompoundRef<'_, S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Source> Default for CompoundRef<'_, S> {
    #[inline]
    fn default() -> Self {
        Self {
            cursor: S::empty_compound(),
        }
    }
}

impl<'a, S: Source> CompoundRef<'a, S> {
    #[inline(always)]
    pub(crate) const fn from_cursor(cursor: S::CompoundCursor<'a>) -> Self {
        Self { cursor }
    }

    #[inline]
    pub(crate) fn get_mutf8(&self, key: &MUTF8Str) -> Option<ValueRef<'a, S>> {
        unsafe {
            let (tag_id, mut iter) = S::compound_get(self.cursor, key)?;
            Some(read_value_advance::<S>(tag_id, &mut iter))
        }
    }

    #[inline]
    pub(crate) fn get_as_mutf8<T: ?Sized + Tag>(&self, key: &MUTF8Str) -> Option<T::Ref<'a, S>> {
        unsafe {
            let (tag_id, mut iter) = S::compound_get(self.cursor, key)?;
            if tag_id != T::TAG_ID {
                cold_path();
                return None;
            }
            T::read_ref::<S>(&mut iter)
        }
    }

    /// Returns `true` if this compound contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        unsafe { S::compound_is_empty(self.cursor) }
    }

    /// Returns `true` if an entry with `key` exists.
    #[inline]
    pub fn contains_key(&self, key: impl AsKey) -> bool {
        key.with_mutf8(|k| unsafe { S::compound_get(self.cursor, k).is_some() })
    }

    /// Looks up `key`, returning [`ValueRef::End`] if absent.
    #[inline]
    pub fn at(&self, key: impl AsKey) -> ValueRef<'a, S> {
        self.get(key).unwrap_or(ValueRef::End)
    }

    /// Looks up `key` and returns its [`ValueRef`].
    #[inline]
    pub fn get(&self, key: impl AsKey) -> Option<ValueRef<'a, S>> {
        key.with_mutf8(|k| self.get_mutf8(k))
    }

    /// Looks up `key` and downcasts it to Rust tag type `T` (including [`TypedList<E>`]).
    #[inline]
    pub fn get_as<T: ?Sized + Tag>(&self, key: impl AsKey) -> Option<T::Ref<'a, S>> {
        key.with_mutf8(|k| self.get_as_mutf8::<T>(k))
    }

    /// Looks up a boolean (`TAG_Byte != 0`) entry by `key`.
    #[inline]
    pub fn get_bool(&self, key: impl AsKey) -> Option<bool> {
        self.get_as::<i8>(key).map(|v| v != 0)
    }

    /// Looks up a `TAG_Byte` (`i8`) entry by `key`.
    #[inline]
    pub fn get_byte(&self, key: impl AsKey) -> Option<i8> {
        self.get_as::<i8>(key)
    }

    /// Looks up a `TAG_Short` (`i16`) entry by `key`.
    #[inline]
    pub fn get_short(&self, key: impl AsKey) -> Option<i16> {
        self.get_as::<i16>(key)
    }

    /// Looks up a `TAG_Int` (`i32`) entry by `key`.
    #[inline]
    pub fn get_int(&self, key: impl AsKey) -> Option<i32> {
        self.get_as::<i32>(key)
    }

    /// Looks up a `TAG_Long` (`i64`) entry by `key`.
    #[inline]
    pub fn get_long(&self, key: impl AsKey) -> Option<i64> {
        self.get_as::<i64>(key)
    }

    /// Looks up a `TAG_Float` (`f32`) entry by `key`.
    #[inline]
    pub fn get_float(&self, key: impl AsKey) -> Option<f32> {
        self.get_as::<f32>(key)
    }

    /// Looks up a `TAG_Double` (`f64`) entry by `key`.
    #[inline]
    pub fn get_double(&self, key: impl AsKey) -> Option<f64> {
        self.get_as::<f64>(key)
    }

    /// Looks up a zero-copy `TAG_String` (`&MUTF8Str`) entry by `key`.
    #[inline]
    pub fn get_string(&self, key: impl AsKey) -> Option<&'a MUTF8Str> {
        self.get_as::<str>(key)
    }

    /// Looks up a `TAG_String` entry by `key` and decodes it to UTF-8 (`Cow<str>`).
    #[inline]
    pub fn get_str(&self, key: impl AsKey) -> Option<std::borrow::Cow<'a, str>> {
        self.get_string(key).map(MUTF8Str::decode_lossy)
    }

    /// Looks up a `TAG_Byte_Array` (`&[i8]`) entry by `key`.
    #[inline]
    pub fn get_byte_array(&self, key: impl AsKey) -> Option<&'a [i8]> {
        self.get_as::<[i8]>(key)
    }

    /// Looks up a `TAG_Int_Array` entry by `key`.
    #[inline]
    pub fn get_int_array(&self, key: impl AsKey) -> Option<&'a [byteorder::I32<S::Order>]> {
        self.get_as::<[i32]>(key)
    }

    /// Looks up a `TAG_Long_Array` entry by `key`.
    #[inline]
    pub fn get_long_array(&self, key: impl AsKey) -> Option<&'a [byteorder::I64<S::Order>]> {
        self.get_as::<[i64]>(key)
    }

    /// Looks up a `TAG_List` entry by `key`.
    #[inline]
    pub fn get_list(&self, key: impl AsKey) -> Option<ListRef<'a, S>> {
        self.get_as::<List>(key)
    }

    /// Looks up a `TAG_Compound` entry by `key`.
    #[inline]
    pub fn get_compound(&self, key: impl AsKey) -> Option<CompoundRef<'a, S>> {
        self.get_as::<Compound>(key)
    }

    /// Returns an iterator over `(&MUTF8Str, ValueRef)` entries in wire order.
    #[inline]
    pub fn iter(&self) -> CompoundIter<'a, S> {
        CompoundIter {
            iter: unsafe { S::compound_to_iter(self.cursor) },
            _marker: PhantomData,
        }
    }

    /// Clones this compound view into an owned [`Compound<TARGET>`].
    #[inline]
    pub fn to_owned_compound<TARGET: ByteOrder>(&self) -> Compound<TARGET> {
        unsafe { S::compound_to_owned::<TARGET>(self.cursor) }
    }

    /// Entry-by-entry conversion, used where a [`Source`] has no bulk path.
    pub(crate) fn to_owned_compound_slow<TARGET: ByteOrder>(self) -> Compound<TARGET> {
        let mut out = Compound::<TARGET>::new();
        for (k, v) in self.iter() {
            out.append_raw_value(k, v.to_owned_value::<TARGET>());
        }
        out
    }

    impl_inherent_write_methods!();
}

impl<'a, S: Source> IntoIterator for CompoundRef<'a, S> {
    type Item = (&'a MUTF8Str, ValueRef<'a, S>);
    type IntoIter = CompoundIter<'a, S>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<S: Source> fmt::Debug for CompoundRef<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// Iterator over key-value entries of a [`CompoundRef`].
pub struct CompoundIter<'a, S: Source = Borrowed<BigEndian>> {
    iter: S::IterCursor,
    _marker: PhantomData<&'a ()>,
}

impl<S: Source> Copy for CompoundIter<'_, S> {}
impl<S: Source> Clone for CompoundIter<'_, S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Source> Default for CompoundIter<'_, S> {
    #[inline]
    fn default() -> Self {
        Self {
            iter: S::empty_iter(),
            _marker: PhantomData,
        }
    }
}

impl<'a, S: Source> Iterator for CompoundIter<'a, S> {
    type Item = (&'a MUTF8Str, ValueRef<'a, S>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let (name, tag_id) = S::compound_next(&mut self.iter)?;
            let val = read_value_advance::<S>(tag_id, &mut self.iter);
            Some((name, val))
        }
    }
}

/// Single-dispatch homogeneous view of a [`ListRef`], matching `element_tag_id` once outside
/// element loops and exposing contiguous slices directly for numeric primitives.
pub enum ListKind<'a, S: Source = Borrowed<BigEndian>> {
    /// `TAG_End` list (typically empty; check [`ListRef::len`] when non-zero `TAG_End` counts matter).
    Empty,
    /// Homogeneous list of `TAG_Byte` (`&[i8]`).
    Byte(&'a [i8]),
    /// Homogeneous list of `TAG_Short` (`&[I16<S::Order>]`).
    Short(&'a [byteorder::I16<S::Order>]),
    /// Homogeneous list of `TAG_Int` (`&[I32<S::Order>]`).
    Int(&'a [byteorder::I32<S::Order>]),
    /// Homogeneous list of `TAG_Long` (`&[I64<S::Order>]`).
    Long(&'a [byteorder::I64<S::Order>]),
    /// Homogeneous list of `TAG_Float` (`&[F32<S::Order>]`).
    Float(&'a [byteorder::F32<S::Order>]),
    /// Homogeneous list of `TAG_Double` (`&[F64<S::Order>]`).
    Double(&'a [byteorder::F64<S::Order>]),
    /// Homogeneous list of `TAG_Byte_Array`.
    ByteArray(TypedListRef<'a, [i8], S>),
    /// Homogeneous list of `TAG_String`.
    String(TypedListRef<'a, str, S>),
    /// Homogeneous list of nested `TAG_List`.
    List(TypedListRef<'a, List<BigEndian>, S>),
    /// Homogeneous list of `TAG_Compound`.
    Compound(TypedListRef<'a, Compound<BigEndian>, S>),
    /// Homogeneous list of `TAG_Int_Array`.
    IntArray(TypedListRef<'a, [i32], S>),
    /// Homogeneous list of `TAG_Long_Array`.
    LongArray(TypedListRef<'a, [i64], S>),
}

impl<S: Source> Copy for ListKind<'_, S> {}
impl<S: Source> Clone for ListKind<'_, S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Source> fmt::Debug for ListKind<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Empty"),
            Self::Byte(s) => f.debug_tuple("Byte").field(s).finish(),
            Self::Short(s) => f.debug_tuple("Short").field(&s.len()).finish(),
            Self::Int(s) => f.debug_tuple("Int").field(&s.len()).finish(),
            Self::Long(s) => f.debug_tuple("Long").field(&s.len()).finish(),
            Self::Float(s) => f.debug_tuple("Float").field(&s.len()).finish(),
            Self::Double(s) => f.debug_tuple("Double").field(&s.len()).finish(),
            Self::ByteArray(l) => f.debug_tuple("ByteArray").field(l).finish(),
            Self::String(l) => f.debug_tuple("String").field(l).finish(),
            Self::List(l) => f.debug_tuple("List").field(l).finish(),
            Self::Compound(l) => f.debug_tuple("Compound").field(l).finish(),
            Self::IntArray(l) => f.debug_tuple("IntArray").field(l).finish(),
            Self::LongArray(l) => f.debug_tuple("LongArray").field(l).finish(),
        }
    }
}

/// A lightweight `Copy` read-only view of an NBT list.
pub struct ListRef<'a, S: Source = Borrowed<BigEndian>> {
    pub(crate) cursor: S::ListCursor<'a>,
}

impl<S: Source> Copy for ListRef<'_, S> {}
impl<S: Source> Clone for ListRef<'_, S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Source> Default for ListRef<'_, S> {
    #[inline]
    fn default() -> Self {
        Self {
            cursor: S::empty_list(),
        }
    }
}

macro_rules! impl_list_ref_prim_slice {
    ($($fn_name:ident => ($variant:ident, $elem_ty:ty)),* $(,)?) => {
        $(
            #[doc = concat!("Returns the underlying slice if this is a list of `TagID::", stringify!($variant), "` (or empty).")]
            #[inline]
            pub fn $fn_name(&self) -> Option<&'a [$elem_ty]> {
                let (tag_id, len) = unsafe { S::list_header(self.cursor) };
                if len == 0 && (tag_id == TagID::$variant || tag_id == TagID::End) {
                    return Some(&[]);
                }
                if tag_id != TagID::$variant {
                    cold_path();
                    return None;
                }
                let ptr = unsafe { S::list_primitive_ptr(self.cursor) };
                Some(unsafe { slice::from_raw_parts(ptr.cast::<$elem_ty>(), len) })
            }
        )*
    };
}

impl<'a, S: Source> ListRef<'a, S> {
    #[inline(always)]
    pub(crate) const fn from_cursor(cursor: S::ListCursor<'a>) -> Self {
        Self { cursor }
    }

    /// Returns the homogeneous [`TagID`] stored in the list header.
    #[inline]
    pub fn element_tag_id(&self) -> TagID {
        unsafe { S::list_header(self.cursor).0 }
    }

    /// Returns `true` if this list has element type `T` or is empty.
    #[inline]
    pub fn element_is<T: ?Sized + ElementTag>(&self) -> bool {
        let (tag_id, len) = unsafe { S::list_header(self.cursor) };
        tag_id == T::TAG_ID || len == 0
    }

    /// Returns the number of elements in the list.
    #[inline]
    pub fn len(&self) -> usize {
        unsafe { S::list_header(self.cursor).1 }
    }

    /// Returns `true` if the list contains zero elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    impl_list_ref_prim_slice!(
        bytes => (Byte, i8),
        shorts => (Short, byteorder::I16<S::Order>),
        ints => (Int, byteorder::I32<S::Order>),
        longs => (Long, byteorder::I64<S::Order>),
        floats => (Float, byteorder::F32<S::Order>),
        doubles => (Double, byteorder::F64<S::Order>),
    );

    /// Returns a statically typed view if this is a list of `TAG_Byte_Array` (or empty).
    #[inline]
    pub fn byte_arrays(&self) -> Option<TypedListRef<'a, [i8], S>> {
        self.typed_as::<[i8]>()
    }

    /// Returns a statically typed view if this is a list of `TAG_String` (or empty).
    #[inline]
    pub fn strings(&self) -> Option<TypedListRef<'a, str, S>> {
        self.typed_as::<str>()
    }

    /// Returns a statically typed view if this is a list of `TAG_List` (or empty).
    #[inline]
    pub fn lists(&self) -> Option<TypedListRef<'a, List<BigEndian>, S>> {
        self.typed_as::<List<BigEndian>>()
    }

    /// Returns a statically typed view if this is a list of `TAG_Compound` (or empty).
    #[inline]
    pub fn compounds(&self) -> Option<TypedListRef<'a, Compound<BigEndian>, S>> {
        self.typed_as::<Compound<BigEndian>>()
    }

    /// Returns a statically typed view if this is a list of `TAG_Int_Array` (or empty).
    #[inline]
    pub fn int_arrays(&self) -> Option<TypedListRef<'a, [i32], S>> {
        self.typed_as::<[i32]>()
    }

    /// Returns a statically typed view if this is a list of `TAG_Long_Array` (or empty).
    #[inline]
    pub fn long_arrays(&self) -> Option<TypedListRef<'a, [i64], S>> {
        self.typed_as::<[i64]>()
    }

    /// Dispatches `element_tag_id` once and returns a homogeneous [`ListKind`] enum (with direct
    /// native slices for all 6 numeric primitive types).
    #[inline]
    pub fn typed(&self) -> ListKind<'a, S> {
        let (tag_id, len) = unsafe { S::list_header(self.cursor) };
        let prim_slice = |ptr: *const u8| {
            if len == 0 {
                std::ptr::NonNull::<u8>::dangling().as_ptr().cast_const()
            } else {
                ptr
            }
        };
        unsafe {
            match tag_id {
                TagID::End => ListKind::Empty,
                TagID::Byte => ListKind::Byte(slice::from_raw_parts(
                    prim_slice(S::list_primitive_ptr(self.cursor)).cast(),
                    len,
                )),
                TagID::Short => ListKind::Short(slice::from_raw_parts(
                    prim_slice(S::list_primitive_ptr(self.cursor)).cast(),
                    len,
                )),
                TagID::Int => ListKind::Int(slice::from_raw_parts(
                    prim_slice(S::list_primitive_ptr(self.cursor)).cast(),
                    len,
                )),
                TagID::Long => ListKind::Long(slice::from_raw_parts(
                    prim_slice(S::list_primitive_ptr(self.cursor)).cast(),
                    len,
                )),
                TagID::Float => ListKind::Float(slice::from_raw_parts(
                    prim_slice(S::list_primitive_ptr(self.cursor)).cast(),
                    len,
                )),
                TagID::Double => ListKind::Double(slice::from_raw_parts(
                    prim_slice(S::list_primitive_ptr(self.cursor)).cast(),
                    len,
                )),
                TagID::ByteArray => ListKind::ByteArray(TypedListRef::from_cursor(self.cursor)),
                TagID::String => ListKind::String(TypedListRef::from_cursor(self.cursor)),
                TagID::List => ListKind::List(TypedListRef::from_cursor(self.cursor)),
                TagID::Compound => ListKind::Compound(TypedListRef::from_cursor(self.cursor)),
                TagID::IntArray => ListKind::IntArray(TypedListRef::from_cursor(self.cursor)),
                TagID::LongArray => ListKind::LongArray(TypedListRef::from_cursor(self.cursor)),
            }
        }
    }

    /// Downcasts this list to a statically typed [`TypedListRef<T, S>`] if its element type
    /// matches `T` (or if the list is empty).
    #[inline]
    pub fn typed_as<T: ?Sized + ElementTag>(&self) -> Option<TypedListRef<'a, T, S>> {
        self.element_is::<T>()
            .then_some(TypedListRef::from_cursor(self.cursor))
    }

    /// Looks up the element at `index`, returning [`ValueRef::End`] if out of bounds.
    #[inline]
    pub fn at(&self, index: usize) -> ValueRef<'a, S> {
        self.get(index).unwrap_or(ValueRef::End)
    }

    /// Looks up the element at `index`.
    #[inline]
    pub fn get(&self, index: usize) -> Option<ValueRef<'a, S>> {
        let (tag_id, len) = unsafe { S::list_header(self.cursor) };
        if index >= len {
            cold_path();
            return None;
        }
        unsafe {
            let mut iter = S::list_seek_tag(self.cursor, tag_id, index);
            Some(read_value_advance::<S>(tag_id, &mut iter))
        }
    }

    /// Looks up and downcasts the element at `index` to Rust tag type `T`.
    #[inline]
    pub fn get_as<T: ?Sized + Tag>(&self, index: usize) -> Option<T::Ref<'a, S>> {
        let (tag_id, len) = unsafe { S::list_header(self.cursor) };
        if index >= len || tag_id != T::TAG_ID {
            cold_path();
            return None;
        }
        unsafe {
            let mut iter = S::list_seek_tag(self.cursor, tag_id, index);
            T::read_ref::<S>(&mut iter)
        }
    }

    /// Returns an iterator yielding [`ValueRef`] elements.
    #[inline]
    pub fn iter(&self) -> ListIter<'a, S> {
        let (tag_id, len) = unsafe { S::list_header(self.cursor) };
        ListIter {
            tag_id,
            remaining: len as u32,
            iter: unsafe { S::list_to_iter(self.cursor) },
            _marker: PhantomData,
        }
    }

    /// Clones this list view into an owned [`List<TARGET>`].
    pub fn to_owned_list<TARGET: ByteOrder>(&self) -> List<TARGET> {
        let (tag_id, len) = unsafe { S::list_header(self.cursor) };
        if len == 0 {
            return List::with_tag_id(tag_id);
        }
        unsafe { S::list_to_owned::<TARGET>(self.cursor) }
    }

    /// Element-by-element conversion, used where a [`Source`] has no bulk path.
    pub(crate) fn to_owned_list_slow<TARGET: ByteOrder>(self) -> List<TARGET> {
        let tag_id = self.element_tag_id();
        let mut out = List::<TARGET>::with_tag_id(tag_id);
        for item in self.iter() {
            let _ = out.push_value(item.to_owned_value::<TARGET>());
        }
        out
    }

    impl_inherent_write_methods!();
}

impl<'a, S: Source> IntoIterator for ListRef<'a, S> {
    type Item = ValueRef<'a, S>;
    type IntoIter = ListIter<'a, S>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<S: Source> fmt::Debug for ListRef<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Iterator over elements of a [`ListRef`].
pub struct ListIter<'a, S: Source = Borrowed<BigEndian>> {
    tag_id: TagID,
    remaining: u32,
    iter: S::IterCursor,
    _marker: PhantomData<&'a ()>,
}

impl<S: Source> Copy for ListIter<'_, S> {}
impl<S: Source> Clone for ListIter<'_, S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Source> Default for ListIter<'_, S> {
    #[inline]
    fn default() -> Self {
        Self {
            tag_id: TagID::End,
            remaining: 0,
            iter: S::empty_iter(),
            _marker: PhantomData,
        }
    }
}

impl<'a, S: Source> Iterator for ListIter<'a, S> {
    type Item = ValueRef<'a, S>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(unsafe { read_value_advance::<S>(self.tag_id, &mut self.iter) })
    }

    #[inline]
    fn fold<B, F>(mut self, mut accum: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        let n = self.remaining;
        macro_rules! fold_by_tag {
            ($($variant:ident => $ty:ty),* $(,)?) => {
                match self.tag_id {
                    TagID::End => {
                        for _ in 0..n {
                            accum = f(accum, ValueRef::End);
                        }
                    }
                    $(TagID::$variant => {
                        for _ in 0..n {
                            let item = ValueRef::$variant(unsafe {
                                <$ty as ElementTag>::read_elem_ref::<S>(&mut self.iter)
                            });
                            accum = f(accum, item);
                        }
                    })*
                }
            };
        }
        fold_by_tag!(
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
        );
        accum
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.remaining as usize;
        (rem, Some(rem))
    }
}

impl<S: Source> ExactSizeIterator for ListIter<'_, S> {}

/// A lightweight `Copy` read-only view of a homogeneous NBT list with statically known element
/// type `T` (`i32`, `str`, `[i8]`, [`Compound`], [`List`], etc.).
pub struct TypedListRef<'a, T: ?Sized + ElementTag, S: Source = Borrowed<BigEndian>> {
    pub(crate) cursor: S::ListCursor<'a>,
    pub(crate) _marker: PhantomData<fn() -> *const T>,
}

impl<T: ?Sized + ElementTag, S: Source> Copy for TypedListRef<'_, T, S> {}
impl<T: ?Sized + ElementTag, S: Source> Clone for TypedListRef<'_, T, S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized + ElementTag, S: Source> Default for TypedListRef<'_, T, S> {
    #[inline]
    fn default() -> Self {
        Self {
            cursor: S::empty_list(),
            _marker: PhantomData,
        }
    }
}

impl<'a, T: ?Sized + ElementTag, S: Source> TypedListRef<'a, T, S> {
    #[inline(always)]
    pub(crate) const fn from_cursor(cursor: S::ListCursor<'a>) -> Self {
        Self {
            cursor,
            _marker: PhantomData,
        }
    }

    /// Returns this typed list as a dynamic [`ListRef`].
    #[inline]
    pub const fn as_list(&self) -> ListRef<'a, S> {
        ListRef {
            cursor: self.cursor,
        }
    }

    /// Returns the number of elements in the list.
    #[inline]
    pub fn len(&self) -> usize {
        unsafe { S::list_header(self.cursor).1 }
    }

    /// Returns `true` if the list contains zero elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Looks up the typed element at `index` without per-element tag dispatch.
    #[inline]
    pub fn get(&self, index: usize) -> Option<T::Ref<'a, S>> {
        if index >= self.len() {
            cold_path();
            return None;
        }
        unsafe {
            let mut iter = T::seek_list_elem::<S>(self.cursor, index);
            Some(T::read_elem_ref::<S>(&mut iter))
        }
    }

    /// Returns a branchless typed iterator over the elements of this list.
    #[inline]
    pub fn iter(&self) -> TypedListIter<'a, T, S> {
        TypedListIter {
            remaining: self.len() as u32,
            iter: unsafe { S::list_to_iter(self.cursor) },
            _marker: PhantomData,
        }
    }

    impl_inherent_write_methods!();
}

impl<'a, S: Source> TypedListRef<'a, i8, S> {
    /// Returns the underlying contiguous `&[i8]` slice.
    #[inline]
    pub fn as_slice(&self) -> &'a [i8] {
        let len = self.len();
        if len == 0 {
            &[]
        } else {
            let ptr = unsafe { S::list_primitive_ptr(self.cursor) };
            unsafe { slice::from_raw_parts(ptr.cast::<i8>(), len) }
        }
    }
}

macro_rules! impl_typed_list_ref_endian_slices {
    ($($prim:ty => $wrapper:ident),* $(,)?) => {
        $(
            impl<'a, S: Source> TypedListRef<'a, $prim, S> {
                /// Returns the contiguous 1-byte-aligned endian-encoded element slice without copying.
                #[inline]
                pub fn as_slice(&self) -> &'a [byteorder::$wrapper<S::Order>] {
                    let len = self.len();
                    if len == 0 {
                        &[]
                    } else {
                        let ptr = unsafe { S::list_primitive_ptr(self.cursor) };
                        unsafe {
                            slice::from_raw_parts(ptr.cast::<byteorder::$wrapper<S::Order>>(), len)
                        }
                    }
                }
            }
        )*
    };
}

impl_typed_list_ref_endian_slices!(
    i16 => I16,
    i32 => I32,
    i64 => I64,
    f32 => F32,
    f64 => F64,
);

impl<'a, T: ?Sized + ElementTag, S: Source> IntoIterator for TypedListRef<'a, T, S> {
    type Item = T::Ref<'a, S>;
    type IntoIter = TypedListIter<'a, T, S>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: ?Sized + ElementTag, S: Source> fmt::Debug for TypedListRef<'_, T, S>
where
    for<'b> T::Ref<'b, S>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Iterator over elements of a [`TypedListRef`].
pub struct TypedListIter<'a, T: ?Sized + ElementTag, S: Source = Borrowed<BigEndian>> {
    remaining: u32,
    iter: S::IterCursor,
    #[allow(clippy::type_complexity)]
    _marker: PhantomData<(&'a (), fn() -> *const T)>,
}

impl<T: ?Sized + ElementTag, S: Source> Copy for TypedListIter<'_, T, S> {}
impl<T: ?Sized + ElementTag, S: Source> Clone for TypedListIter<'_, T, S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized + ElementTag, S: Source> Default for TypedListIter<'_, T, S> {
    #[inline]
    fn default() -> Self {
        Self {
            remaining: 0,
            iter: S::empty_iter(),
            _marker: PhantomData,
        }
    }
}

impl<'a, T: ?Sized + ElementTag, S: Source> Iterator for TypedListIter<'a, T, S> {
    type Item = T::Ref<'a, S>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(unsafe { T::read_elem_ref::<S>(&mut self.iter) })
    }

    #[inline]
    fn fold<B, F>(mut self, mut accum: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        for _ in 0..self.remaining {
            let item = unsafe { T::read_elem_ref::<S>(&mut self.iter) };
            accum = f(accum, item);
        }
        accum
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.remaining as usize;
        (rem, Some(rem))
    }
}

impl<T: ?Sized + ElementTag, S: Source> ExactSizeIterator for TypedListIter<'_, T, S> {}

/// Conversion trait allowing generic read-only functions to accept [`ValueRef`],
/// [`&Document`](Document), [`&Value`](Value), [`&ValueMut`](ValueMut), [`CompoundRef`],
/// [`&Compound`](Compound), [`ListRef`], [`&List`](List), [`TypedListRef`], or
/// [`&TypedList`](TypedList).
pub trait AsValueRef<'a> {
    /// Underlying [`Source`] (`Borrowed<O>` or `Owned<O>`).
    type Source: Source;
    /// Returns a lightweight `Copy` [`ValueRef`] view.
    #[allow(clippy::wrong_self_convention)]
    fn as_value_ref(self) -> ValueRef<'a, Self::Source>;
}

impl<'a, S: Source> AsValueRef<'a> for ValueRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, S> {
        self
    }
}

impl<'a, S: Source> AsValueRef<'a> for &ValueRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, S> {
        *self
    }
}

impl<'a, S: Source> AsValueRef<'a> for CompoundRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, S> {
        ValueRef::Compound(self)
    }
}

impl<'a, S: Source> AsValueRef<'a> for &CompoundRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, S> {
        ValueRef::Compound(*self)
    }
}

impl<'a, S: Source> AsValueRef<'a> for ListRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, S> {
        ValueRef::List(self)
    }
}

impl<'a, S: Source> AsValueRef<'a> for &ListRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, S> {
        ValueRef::List(*self)
    }
}

impl<'a, T: ?Sized + ElementTag, S: Source> AsValueRef<'a> for TypedListRef<'a, T, S> {
    type Source = S;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, S> {
        ValueRef::List(self.as_list())
    }
}

impl<'a, T: ?Sized + ElementTag, S: Source> AsValueRef<'a> for &TypedListRef<'a, T, S> {
    type Source = S;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, S> {
        ValueRef::List(self.as_list())
    }
}

impl<'a, O: ByteOrder, B: crate::DocumentBuffer> AsValueRef<'a> for &'a Document<O, B> {
    type Source = Borrowed<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Borrowed<O>> {
        self.root()
    }
}

impl<'a, O: ByteOrder> AsValueRef<'a> for &'a Value<O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Owned<O>> {
        self.as_ref()
    }
}

impl<'a, O: ByteOrder> AsValueRef<'a> for &'a ValueMut<'_, O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Owned<O>> {
        self.as_ref()
    }
}

impl<'a, O: ByteOrder> AsValueRef<'a> for &'a Compound<O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Owned<O>> {
        ValueRef::Compound(self.as_ref())
    }
}

impl<'a, O: ByteOrder> AsValueRef<'a> for &'a Mut<'_, Compound<O>> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Owned<O>> {
        ValueRef::Compound((**self).as_ref())
    }
}

impl<'a, O: ByteOrder> AsValueRef<'a> for &'a List<O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Owned<O>> {
        ValueRef::List(self.as_ref())
    }
}

impl<'a, O: ByteOrder> AsValueRef<'a> for &'a Mut<'_, List<O>> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Owned<O>> {
        ValueRef::List((**self).as_ref())
    }
}

impl<'a, T: ?Sized + ElementTag, O: ByteOrder> AsValueRef<'a> for &'a TypedList<T, O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Owned<O>> {
        ValueRef::List(self.as_list_ref())
    }
}

impl<'a, T: ?Sized + ElementTag, O: ByteOrder> AsValueRef<'a> for &'a Mut<'_, TypedList<T, O>> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Owned<O>> {
        ValueRef::List((**self).as_list_ref())
    }
}

/// Conversion trait allowing generic read-only functions to accept [`CompoundRef`],
/// [`&Compound`](Compound), or [`&Mut<Compound>`](Mut).
pub trait AsCompoundRef<'a> {
    /// Underlying [`Source`] (`Borrowed<O>` or `Owned<O>`).
    type Source: Source;
    /// Returns a lightweight `Copy` [`CompoundRef`] view.
    #[allow(clippy::wrong_self_convention)]
    fn as_compound_ref(self) -> CompoundRef<'a, Self::Source>;
}

impl<'a, S: Source> AsCompoundRef<'a> for CompoundRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_compound_ref(self) -> CompoundRef<'a, S> {
        self
    }
}

impl<'a, S: Source> AsCompoundRef<'a> for &CompoundRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_compound_ref(self) -> CompoundRef<'a, S> {
        *self
    }
}

impl<'a, O: ByteOrder> AsCompoundRef<'a> for &'a Compound<O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_compound_ref(self) -> CompoundRef<'a, Owned<O>> {
        self.as_ref()
    }
}

impl<'a, O: ByteOrder> AsCompoundRef<'a> for &'a Mut<'_, Compound<O>> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_compound_ref(self) -> CompoundRef<'a, Owned<O>> {
        (**self).as_ref()
    }
}

/// Conversion trait allowing generic read-only functions to accept [`ListRef`],
/// [`&List`](List), or [`&Mut<List>`](Mut).
pub trait AsListRef<'a> {
    /// Underlying [`Source`] (`Borrowed<O>` or `Owned<O>`).
    type Source: Source;
    /// Returns a lightweight `Copy` [`ListRef`] view.
    #[allow(clippy::wrong_self_convention)]
    fn as_list_ref(self) -> ListRef<'a, Self::Source>;
}

impl<'a, S: Source> AsListRef<'a> for ListRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_list_ref(self) -> ListRef<'a, S> {
        self
    }
}

impl<'a, S: Source> AsListRef<'a> for &ListRef<'a, S> {
    type Source = S;
    #[inline(always)]
    fn as_list_ref(self) -> ListRef<'a, S> {
        *self
    }
}

impl<'a, O: ByteOrder> AsListRef<'a> for &'a List<O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_list_ref(self) -> ListRef<'a, Owned<O>> {
        self.as_ref()
    }
}

impl<'a, O: ByteOrder> AsListRef<'a> for &'a Mut<'_, List<O>> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_list_ref(self) -> ListRef<'a, Owned<O>> {
        (**self).as_ref()
    }
}

impl<S1: Source, S2: Source> PartialEq<ValueRef<'_, S2>> for ValueRef<'_, S1> {
    #[inline]
    fn eq(&self, other: &ValueRef<'_, S2>) -> bool {
        match (*self, *other) {
            (Self::End, ValueRef::End) => true,
            (Self::Byte(a), ValueRef::Byte(b)) => a == b,
            (Self::Short(a), ValueRef::Short(b)) => a == b,
            (Self::Int(a), ValueRef::Int(b)) => a == b,
            (Self::Long(a), ValueRef::Long(b)) => a == b,
            (Self::Float(a), ValueRef::Float(b)) => a == b,
            (Self::Double(a), ValueRef::Double(b)) => a == b,
            (Self::ByteArray(a), ValueRef::ByteArray(b)) => a == b,
            (Self::String(a), ValueRef::String(b)) => a == b,
            (Self::IntArray(a), ValueRef::IntArray(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.get() == y.get())
            }
            (Self::LongArray(a), ValueRef::LongArray(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.get() == y.get())
            }
            (Self::List(a), ValueRef::List(b)) => a == b,
            (Self::Compound(a), ValueRef::Compound(b)) => a == b,
            _ => false,
        }
    }
}

impl<S1: Source, S2: Source> PartialEq<ListRef<'_, S2>> for ListRef<'_, S1> {
    #[inline]
    fn eq(&self, other: &ListRef<'_, S2>) -> bool {
        let len = self.len();
        if len != other.len() {
            return false;
        }
        if len == 0 {
            return true;
        }
        self.element_tag_id() == other.element_tag_id() && self.iter().eq(other.iter())
    }
}

impl<T: ?Sized + ElementTag, S1: Source, S2: Source> PartialEq<TypedListRef<'_, T, S2>>
    for TypedListRef<'_, T, S1>
{
    #[inline]
    fn eq(&self, other: &TypedListRef<'_, T, S2>) -> bool {
        self.as_list() == other.as_list()
    }
}

impl<S1: Source, S2: Source> PartialEq<CompoundRef<'_, S2>> for CompoundRef<'_, S1> {
    fn eq(&self, other: &CompoundRef<'_, S2>) -> bool {
        let mut ia = self.iter();
        let mut ib = other.iter();
        loop {
            match (ia.next(), ib.next()) {
                (None, None) => return true,
                (Some(_), None) | (None, Some(_)) => return false,
                (Some((ka, va)), Some((kb, vb))) => {
                    if ka != kb || va != vb {
                        if ia.count() != ib.count() {
                            return false;
                        }
                        return self
                            .iter()
                            .all(|(k, v)| other.get(k).is_some_and(|ov| ov == v))
                            && other
                                .iter()
                                .all(|(k, v)| self.get(k).is_some_and(|sv| sv == v));
                    }
                }
            }
        }
    }
}
