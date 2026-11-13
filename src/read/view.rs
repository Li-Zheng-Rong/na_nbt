use std::{fmt, io::Write, marker::PhantomData};

use zerocopy::byteorder;

use crate::{
    Borrowed, ByteOrder, CompoundOwn, Document, ElementTag, ListOwn, MUTF8Str, Mut, Owned, Result,
    Source, Tag, TagID, ValueMut, ValueOwn, Writable, cold_path, index::Index, tag,
    with_mutf8_key,
};

/// A lightweight `Copy` read-only view of any NBT value (borrowed or owned).
pub enum ValueRef<'a, S: Source> {
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(&'a [i8]),
    String(&'a MUTF8Str),
    List(ListRef<'a, S>),
    Compound(CompoundRef<'a, S>),
    IntArray(&'a [byteorder::I32<S::Order>]),
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
        ($($t:ident),* $(,)?) => {
            match tag_id {
                TagID::End => ValueRef::End,
                $(TagID::$t => ValueRef::$t(unsafe {
                    <tag::$t as ElementTag>::read_elem_ref::<S>(iter)
                }),)*
            }
        };
    }
    dispatch!(
        Byte, Short, Int, Long, Float, Double, ByteArray, String, List, Compound, IntArray,
        LongArray
    )
}

impl<'a, S: Source> ValueRef<'a, S> {
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
        T::from_value_ref(*self).is_some()
    }

    /// Downcasts this `Copy` value view to tag `T` (including [`tag::TypedList<E>`]).
    #[inline]
    pub fn as_<T: Tag>(self) -> Option<T::Ref<'a, S>> {
        T::from_value_ref(self)
    }

    #[inline]
    pub fn get(&self, index: impl Index) -> Option<ValueRef<'a, S>> {
        index.dispatch(
            *self,
            |v, idx| match v {
                Self::List(list) => list.get(idx),
                _ => None,
            },
            |v, key| match v {
                Self::Compound(comp) => comp.get(key),
                _ => None,
            },
        )
    }

    #[inline]
    pub fn get_as<T: Tag>(&self, index: impl Index) -> Option<T::Ref<'a, S>> {
        index.dispatch(
            *self,
            |v, idx| match v {
                Self::List(list) => list.get_as::<T>(idx),
                _ => None,
            },
            |v, key| match v {
                Self::Compound(comp) => comp.get_as::<T>(key),
                _ => None,
            },
        )
    }
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

impl<S: Source> Writable for ValueRef<'_, S> {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        match self {
            Self::End => ().write_to_vec::<TARGET>(),
            Self::Byte(v) => v.write_to_vec::<TARGET>(),
            Self::Short(v) => v.write_to_vec::<TARGET>(),
            Self::Int(v) => v.write_to_vec::<TARGET>(),
            Self::Long(v) => v.write_to_vec::<TARGET>(),
            Self::Float(v) => v.write_to_vec::<TARGET>(),
            Self::Double(v) => v.write_to_vec::<TARGET>(),
            Self::ByteArray(v) => v.write_to_vec::<TARGET>(),
            Self::String(v) => v.write_to_vec::<TARGET>(),
            Self::List(v) => v.write_to_vec::<TARGET>(),
            Self::Compound(v) => v.write_to_vec::<TARGET>(),
            Self::IntArray(v) => v.write_to_vec::<TARGET>(),
            Self::LongArray(v) => v.write_to_vec::<TARGET>(),
        }
    }

    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        match self {
            Self::End => ().write_to_writer::<TARGET>(writer),
            Self::Byte(v) => v.write_to_writer::<TARGET>(writer),
            Self::Short(v) => v.write_to_writer::<TARGET>(writer),
            Self::Int(v) => v.write_to_writer::<TARGET>(writer),
            Self::Long(v) => v.write_to_writer::<TARGET>(writer),
            Self::Float(v) => v.write_to_writer::<TARGET>(writer),
            Self::Double(v) => v.write_to_writer::<TARGET>(writer),
            Self::ByteArray(v) => v.write_to_writer::<TARGET>(writer),
            Self::String(v) => v.write_to_writer::<TARGET>(writer),
            Self::List(v) => v.write_to_writer::<TARGET>(writer),
            Self::Compound(v) => v.write_to_writer::<TARGET>(writer),
            Self::IntArray(v) => v.write_to_writer::<TARGET>(writer),
            Self::LongArray(v) => v.write_to_writer::<TARGET>(writer),
        }
    }
}

/// A lightweight `Copy` read-only view of an NBT compound.
pub struct CompoundRef<'a, S: Source> {
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
    pub fn is_empty(&self) -> bool {
        unsafe { S::compound_is_empty(self.cursor) }
    }

    #[inline]
    pub fn contains_key(&self, key: &str) -> bool {
        with_mutf8_key(key, |k| unsafe { S::compound_get(self.cursor, k).is_some() })
    }

    #[inline]
    pub fn get(&self, key: &str) -> Option<ValueRef<'a, S>> {
        with_mutf8_key(key, |k| unsafe {
            let (tag_id, mut iter) = S::compound_get(self.cursor, k)?;
            Some(read_value_advance::<S>(tag_id, &mut iter))
        })
    }

    #[inline]
    pub fn get_as<T: Tag>(&self, key: &str) -> Option<T::Ref<'a, S>> {
        with_mutf8_key(key, |k| unsafe {
            let (tag_id, mut iter) = S::compound_get(self.cursor, k)?;
            if tag_id != T::TAG_ID {
                cold_path();
                return None;
            }
            T::read_ref::<S>(&mut iter)
        })
    }

    #[inline]
    pub fn iter(&self) -> CompoundIter<'a, S> {
        CompoundIter {
            iter: unsafe { S::compound_to_iter(self.cursor) },
            _marker: PhantomData,
        }
    }
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

impl<S: Source> Writable for CompoundRef<'_, S> {
    #[inline]
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        S::write_compound_to_vec::<TARGET>(self.cursor)
    }

    #[inline]
    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        S::write_compound_to_writer::<TARGET>(self.cursor, writer)
    }
}

/// Iterator over key-value entries of a [`CompoundRef`].
pub struct CompoundIter<'a, S: Source> {
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

/// A lightweight `Copy` read-only view of an untyped NBT list.
pub struct ListRef<'a, S: Source> {
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

impl<'a, S: Source> ListRef<'a, S> {
    #[inline(always)]
    pub(crate) const fn from_cursor(cursor: S::ListCursor<'a>) -> Self {
        Self { cursor }
    }

    #[inline]
    pub fn element_tag_id(&self) -> TagID {
        unsafe { S::list_header(self.cursor).0 }
    }

    #[inline]
    pub fn element_is<T: ElementTag>(&self) -> bool {
        let (tag_id, len) = unsafe { S::list_header(self.cursor) };
        tag_id == T::TAG_ID || len == 0
    }

    #[inline]
    pub fn len(&self) -> usize {
        unsafe { S::list_header(self.cursor).1 }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

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

    #[inline]
    pub fn get_as<T: Tag>(&self, index: usize) -> Option<T::Ref<'a, S>> {
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

    #[inline]
    pub fn typed<T: ElementTag>(&self) -> Option<TypedListRef<'a, S, T>> {
        self.element_is::<T>().then_some(TypedListRef {
            cursor: self.cursor,
            _marker: PhantomData,
        })
    }

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

impl<S: Source> Writable for ListRef<'_, S> {
    #[inline]
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        S::write_list_to_vec::<TARGET>(self.cursor)
    }

    #[inline]
    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        S::write_list_to_writer::<TARGET>(self.cursor, writer)
    }
}

/// Iterator over elements of a [`ListRef`].
pub struct ListIter<'a, S: Source> {
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
            ($($t:ident),* $(,)?) => {
                match self.tag_id {
                    TagID::End => {
                        for _ in 0..n {
                            accum = f(accum, ValueRef::End);
                        }
                    }
                    $(TagID::$t => {
                        for _ in 0..n {
                            let item = ValueRef::$t(unsafe {
                                <tag::$t as ElementTag>::read_elem_ref::<S>(&mut self.iter)
                            });
                            accum = f(accum, item);
                        }
                    })*
                }
            };
        }
        fold_by_tag!(
            Byte, Short, Int, Long, Float, Double, ByteArray, String, List, Compound, IntArray,
            LongArray
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

/// A lightweight `Copy` read-only view of a homogeneous NBT list with statically known element tag `T`.
pub struct TypedListRef<'a, S: Source, T: ElementTag> {
    pub(crate) cursor: S::ListCursor<'a>,
    pub(crate) _marker: PhantomData<fn() -> T>,
}

impl<S: Source, T: ElementTag> Copy for TypedListRef<'_, S, T> {}
impl<S: Source, T: ElementTag> Clone for TypedListRef<'_, S, T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Source, T: ElementTag> Default for TypedListRef<'_, S, T> {
    #[inline]
    fn default() -> Self {
        Self {
            cursor: S::empty_list(),
            _marker: PhantomData,
        }
    }
}

impl<'a, S: Source, T: ElementTag> TypedListRef<'a, S, T> {
    #[inline(always)]
    pub(crate) const fn from_cursor(cursor: S::ListCursor<'a>) -> Self {
        Self {
            cursor,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        unsafe { S::list_header(self.cursor).1 }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

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

    #[inline]
    pub fn iter(&self) -> TypedListIter<'a, S, T> {
        TypedListIter {
            remaining: self.len() as u32,
            iter: unsafe { S::list_to_iter(self.cursor) },
            _marker: PhantomData,
        }
    }
}

impl<'a, S: Source, T: ElementTag> IntoIterator for TypedListRef<'a, S, T> {
    type Item = T::Ref<'a, S>;
    type IntoIter = TypedListIter<'a, S, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<S: Source, T: ElementTag> fmt::Debug for TypedListRef<'_, S, T>
where
    for<'b> T::Ref<'b, S>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<S: Source, T: ElementTag> Writable for TypedListRef<'_, S, T> {
    #[inline]
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        S::write_list_to_vec::<TARGET>(self.cursor)
    }

    #[inline]
    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        S::write_list_to_writer::<TARGET>(self.cursor, writer)
    }
}

/// Iterator over elements of a [`TypedListRef`].
pub struct TypedListIter<'a, S: Source, T: ElementTag> {
    remaining: u32,
    iter: S::IterCursor,
    _marker: PhantomData<&'a T>,
}

impl<S: Source, T: ElementTag> Copy for TypedListIter<'_, S, T> {}
impl<S: Source, T: ElementTag> Clone for TypedListIter<'_, S, T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Source, T: ElementTag> Default for TypedListIter<'_, S, T> {
    #[inline]
    fn default() -> Self {
        Self {
            remaining: 0,
            iter: S::empty_iter(),
            _marker: PhantomData,
        }
    }
}

impl<'a, S: Source, T: ElementTag> Iterator for TypedListIter<'a, S, T> {
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

impl<S: Source, T: ElementTag> ExactSizeIterator for TypedListIter<'_, S, T> {}

/// Conversion trait allowing generic read-only functions to accept [`ValueRef`], [`&Document`](Document),
/// [`&ValueOwn`](ValueOwn), or [`&ValueMut`](ValueMut) seamlessly.
pub trait AsValueRef<'a> {
    type Source: Source;
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

impl<'a, O: ByteOrder, B: AsRef<[u8]>> AsValueRef<'a> for &'a Document<O, B> {
    type Source = Borrowed<O>;
    #[inline(always)]
    fn as_value_ref(self) -> ValueRef<'a, Borrowed<O>> {
        self.root()
    }
}

impl<'a, O: ByteOrder> AsValueRef<'a> for &'a ValueOwn<O> {
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

/// Conversion trait allowing generic read-only functions to accept [`CompoundRef`],
/// [`&CompoundOwn`](CompoundOwn), or [`&Mut<CompoundOwn>`](Mut).
pub trait AsCompoundRef<'a> {
    type Source: Source;
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

impl<'a, O: ByteOrder> AsCompoundRef<'a> for &'a CompoundOwn<O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_compound_ref(self) -> CompoundRef<'a, Owned<O>> {
        self.as_ref()
    }
}

impl<'a, O: ByteOrder> AsCompoundRef<'a> for &'a Mut<'_, CompoundOwn<O>> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_compound_ref(self) -> CompoundRef<'a, Owned<O>> {
        (**self).as_ref()
    }
}

/// Conversion trait allowing generic read-only functions to accept [`ListRef`],
/// [`&ListOwn`](ListOwn), or [`&Mut<ListOwn>`](Mut).
pub trait AsListRef<'a> {
    type Source: Source;
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

impl<'a, O: ByteOrder> AsListRef<'a> for &'a ListOwn<O> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_list_ref(self) -> ListRef<'a, Owned<O>> {
        self.as_ref()
    }
}

impl<'a, O: ByteOrder> AsListRef<'a> for &'a Mut<'_, ListOwn<O>> {
    type Source = Owned<O>;
    #[inline(always)]
    fn as_list_ref(self) -> ListRef<'a, Owned<O>> {
        (**self).as_ref()
    }
}
