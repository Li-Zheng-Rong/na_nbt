use std::{marker::PhantomData, ptr, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigRef, EMPTY_LIST, ListBase, ListRef, MUTF8Str, NBT, TagID,
    immutable::{
        array::ReadonlyArray, compound::ReadonlyCompound, config::ImmutableConfig,
        document::Document, mark::Mark, string::ReadonlyString, typed_list::ReadonlyTypedList,
        value::ReadonlyValue,
    },
};

#[inline(always)]
pub(crate) unsafe fn read_and_advance<'doc, O: ByteOrder, D: Document>(
    tag_id: TagID,
    data: &mut *const u8,
    mark: &mut *const Mark,
    doc: &D,
) -> ReadonlyValue<'doc, O, D> {
    unsafe {
        let ptr = *data;
        match tag_id {
            TagID::End => ReadonlyValue::End(()),
            TagID::Byte => {
                *data = ptr.add(1);
                ReadonlyValue::Byte(*ptr.cast())
            }
            TagID::Short => {
                *data = ptr.add(2);
                ReadonlyValue::Short(byteorder::I16::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::Int => {
                *data = ptr.add(4);
                ReadonlyValue::Int(byteorder::I32::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::Long => {
                *data = ptr.add(8);
                ReadonlyValue::Long(byteorder::I64::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::Float => {
                *data = ptr.add(4);
                ReadonlyValue::Float(byteorder::F32::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::Double => {
                *data = ptr.add(8);
                ReadonlyValue::Double(byteorder::F64::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::ByteArray => {
                let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
                *data = ptr.add(4 + len);
                ReadonlyValue::ByteArray(ReadonlyArray {
                    data: slice::from_raw_parts(ptr.add(4).cast(), len),
                    _doc: doc.clone(),
                })
            }
            TagID::String => {
                let len = byteorder::U16::<O>::from_bytes(*ptr.cast()).get() as usize;
                *data = ptr.add(2 + len);
                ReadonlyValue::String(ReadonlyString {
                    data: MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(ptr.add(2), len)),
                    _doc: doc.clone(),
                })
            }
            TagID::List => {
                let cur_mark = *mark;
                let m = *cur_mark;
                let byte_len = m.byte_len as usize;
                *data = ptr.add(byte_len);
                *mark = cur_mark.add(m.flat_next_mark as usize);
                ReadonlyValue::List(ReadonlyList {
                    data: slice::from_raw_parts(ptr, byte_len),
                    mark: cur_mark.add(1),
                    doc: doc.clone(),
                    _marker: PhantomData,
                })
            }
            TagID::Compound => {
                let cur_mark = *mark;
                let m = *cur_mark;
                let byte_len = m.byte_len as usize;
                *data = ptr.add(byte_len);
                *mark = cur_mark.add(m.flat_next_mark as usize);
                ReadonlyValue::Compound(ReadonlyCompound {
                    data: slice::from_raw_parts(ptr, byte_len),
                    mark: cur_mark.add(1),
                    doc: doc.clone(),
                    _marker: PhantomData,
                })
            }
            TagID::IntArray => {
                let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
                *data = ptr.add(4 + len * 4);
                ReadonlyValue::IntArray(ReadonlyArray {
                    data: slice::from_raw_parts(ptr.add(4).cast(), len),
                    _doc: doc.clone(),
                })
            }
            TagID::LongArray => {
                let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
                *data = ptr.add(4 + len * 8);
                ReadonlyValue::LongArray(ReadonlyArray {
                    data: slice::from_raw_parts(ptr.add(4).cast(), len),
                    _doc: doc.clone(),
                })
            }
        }
    }
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
        let mut ptr = self.data;
        let mut mark = self.mark;
        let doc = &self.doc;
        let n = self.remaining;
        macro_rules! fold_by_tag {
            ($($tag:ident),* $(,)?) => {
                match self.tag_id {
                    $(TagID::$tag => {
                        for _ in 0..n {
                            let item = unsafe {
                                read_and_advance::<O, D>(TagID::$tag, &mut ptr, &mut mark, doc)
                            };
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
