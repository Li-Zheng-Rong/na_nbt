use std::{marker::PhantomData, ptr};

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigRef, EMPTY_LIST, ListBase, ListRef, NBT, NBTRef, TagID,
    immutable::{
        config::ImmutableConfig, document::Document, mark::Mark, typed_list::ReadonlyTypedList,
        value::ReadonlyValue,
    },
    tag,
};

#[inline(always)]
pub(crate) unsafe fn read_and_advance<'doc, O: ByteOrder, D: Document>(
    tag_id: TagID,
    data: &mut *const u8,
    mark: &mut *const Mark,
    doc: &D,
) -> ReadonlyValue<'doc, O, D> {
    let mut params = (*data, *mark, doc);
    macro_rules! dispatch {
        ($($t:ident),* $(,)?) => {
            match tag_id {
                $(TagID::$t => ReadonlyValue::$t(unsafe {
                    <tag::$t as NBTRef>::read_advance_nbt::<ImmutableConfig<O, D>>(&mut params)
                }),)*
            }
        };
    }
    let val = dispatch!(
        End, Byte, Short, Int, Long, Float, Double, ByteArray, String, List, Compound, IntArray,
        LongArray
    );
    *data = params.0;
    *mark = params.1;
    val
}

#[derive(Clone)]
pub struct ReadonlyList<'doc, O: ByteOrder, D: Document> {
    pub(crate) data: &'doc [u8],
    pub(crate) mark: *const Mark,
    pub(crate) doc: D,
    pub(crate) _marker: PhantomData<O>,
}

impl<'doc, O: ByteOrder, D: Document> Default for ReadonlyList<'doc, O, D> {
    #[inline]
    fn default() -> Self {
        Self {
            data: &EMPTY_LIST,
            mark: ptr::null(),
            doc: D::empty(),
            _marker: PhantomData,
        }
    }
}

unsafe impl<'doc, O: ByteOrder, D: Document> Send for ReadonlyList<'doc, O, D> {}
unsafe impl<'doc, O: ByteOrder, D: Document> Sync for ReadonlyList<'doc, O, D> {}

impl<'doc, O: ByteOrder, D: Document> IntoIterator for ReadonlyList<'doc, O, D> {
    type Item = ReadonlyValue<'doc, O, D>;
    type IntoIter = ReadonlyListIter<'doc, O, D>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        ReadonlyListIter {
            tag_id: self.element_tag_id(),
            remaining: self.len() as u32,
            data: unsafe { self.data.as_ptr().add(1 + 4) },
            mark: self.mark,
            doc: self.doc,
            _marker: PhantomData,
        }
    }
}

impl<'doc, O: ByteOrder, D: Document> ListBase for ReadonlyList<'doc, O, D> {
    #[inline]
    fn element_tag_id(&self) -> TagID {
        unsafe { *self.data.as_ptr().cast() }
    }

    #[inline]
    fn len(&self) -> usize {
        unsafe { byteorder::U32::<O>::from_bytes(*self.data.as_ptr().add(1).cast()).get() as usize }
    }
}

impl<'doc, O: ByteOrder, D: Document> ListRef<'doc> for ReadonlyList<'doc, O, D> {
    type Config = ImmutableConfig<O, D>;

    #[inline]
    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        'doc: 'a,
    {
        (
            unsafe { self.data.as_ptr().add(1 + 4) },
            self.mark,
            &self.doc,
        )
    }

    #[inline]
    fn typed_<T: NBT>(self) -> Option<<Self::Config as ConfigRef>::TypedList<'doc, T>> {
        self.element_is_::<T>().then_some(ReadonlyTypedList {
            data: self.data,
            mark: self.mark,
            doc: self.doc,
            _marker: PhantomData,
        })
    }

    #[inline]
    fn iter(&self) -> <Self::Config as ConfigRef>::ListIter<'doc> {
        ReadonlyListIter {
            tag_id: self.element_tag_id(),
            remaining: self.len() as u32,
            data: unsafe { self.data.as_ptr().add(1 + 4) },
            mark: self.mark,
            doc: self.doc.clone(),
            _marker: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct ReadonlyListIter<'doc, O: ByteOrder, D: Document> {
    tag_id: TagID,
    remaining: u32,
    data: *const u8,
    mark: *const Mark,
    doc: D,
    _marker: PhantomData<(&'doc (), O)>,
}

impl<'doc, O: ByteOrder, D: Document> Default for ReadonlyListIter<'doc, O, D> {
    #[inline]
    fn default() -> Self {
        Self {
            tag_id: TagID::End,
            remaining: 0,
            data: ptr::null(),
            mark: ptr::null(),
            doc: D::empty(),
            _marker: PhantomData,
        }
    }
}

unsafe impl<'doc, O: ByteOrder, D: Document> Send for ReadonlyListIter<'doc, O, D> {}
unsafe impl<'doc, O: ByteOrder, D: Document> Sync for ReadonlyListIter<'doc, O, D> {}

impl<'doc, O: ByteOrder, D: Document> Iterator for ReadonlyListIter<'doc, O, D> {
    type Item = ReadonlyValue<'doc, O, D>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(unsafe {
            read_and_advance::<O, D>(self.tag_id, &mut self.data, &mut self.mark, &self.doc)
        })
    }

    #[inline]
    fn fold<B, F>(self, mut accum: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        let mut params = (self.data, self.mark, &self.doc);
        let n = self.remaining;
        macro_rules! fold_by_tag {
            ($($t:ident),* $(,)?) => {
                match self.tag_id {
                    $(TagID::$t => {
                        for _ in 0..n {
                            let item = ReadonlyValue::$t(unsafe {
                                <tag::$t as NBTRef>::read_advance_nbt::<ImmutableConfig<O, D>>(&mut params)
                            });
                            accum = f(accum, item);
                        }
                    })*
                }
            };
        }
        fold_by_tag!(
            End, Byte, Short, Int, Long, Float, Double, ByteArray, String, List, Compound,
            IntArray, LongArray
        );
        accum
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining as usize;
        (remaining, Some(remaining))
    }
}

impl<'doc, O: ByteOrder, D: Document> ExactSizeIterator for ReadonlyListIter<'doc, O, D> {}
