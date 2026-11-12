use std::{marker::PhantomData, ptr, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, CompoundBase, CompoundRef, ConfigRef, EMPTY_COMPOUND, MUTF8Str, TagID,
    immutable::{
        config::ImmutableConfig, document::Document, list::read_and_advance, mark::Mark,
        string::ReadonlyString, value::ReadonlyValue,
    },
};

#[derive(Clone)]
pub struct ReadonlyCompound<'doc, O: ByteOrder, D: Document> {
    pub(crate) data: &'doc [u8],
    pub(crate) mark: *const Mark,
    pub(crate) doc: D,
    pub(crate) _marker: PhantomData<O>,
}

impl<'doc, O: ByteOrder, D: Document> Default for ReadonlyCompound<'doc, O, D> {
    #[inline]
    fn default() -> Self {
        Self {
            data: &EMPTY_COMPOUND,
            mark: ptr::null(),
            doc: D::empty(),
            _marker: PhantomData,
        }
    }
}

unsafe impl<'doc, O: ByteOrder, D: Document> Send for ReadonlyCompound<'doc, O, D> {}
unsafe impl<'doc, O: ByteOrder, D: Document> Sync for ReadonlyCompound<'doc, O, D> {}

impl<'doc, O: ByteOrder, D: Document> IntoIterator for ReadonlyCompound<'doc, O, D> {
    type Item = (ReadonlyString<'doc, D>, ReadonlyValue<'doc, O, D>);
    type IntoIter = ReadonlyCompoundIter<'doc, O, D>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        ReadonlyCompoundIter {
            data: self.data.as_ptr(),
            mark: self.mark,
            doc: self.doc,
            _marker: PhantomData,
        }
    }
}

impl<'doc, O: ByteOrder, D: Document> CompoundBase for ReadonlyCompound<'doc, O, D> {}

impl<'doc, O: ByteOrder, D: Document> CompoundRef<'doc> for ReadonlyCompound<'doc, O, D> {
    type Config = ImmutableConfig<O, D>;

    #[inline]
    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        'doc: 'a,
    {
        (self.data.as_ptr(), self.mark, &self.doc)
    }

    #[inline]
    fn iter(&self) -> <Self::Config as ConfigRef>::CompoundIter<'doc> {
        ReadonlyCompoundIter {
            data: self.data.as_ptr(),
            mark: self.mark,
            doc: self.doc.clone(),
            _marker: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct ReadonlyCompoundIter<'doc, O: ByteOrder, D: Document> {
    data: *const u8,
    mark: *const Mark,
    doc: D,
    _marker: PhantomData<(&'doc (), O)>,
}

impl<'doc, O: ByteOrder, D: Document> Default for ReadonlyCompoundIter<'doc, O, D> {
    #[inline]
    fn default() -> Self {
        Self {
            data: EMPTY_COMPOUND.as_ptr(),
            mark: ptr::null(),
            doc: D::empty(),
            _marker: PhantomData,
        }
    }
}

unsafe impl<'doc, O: ByteOrder, D: Document> Send for ReadonlyCompoundIter<'doc, O, D> {}
unsafe impl<'doc, O: ByteOrder, D: Document> Sync for ReadonlyCompoundIter<'doc, O, D> {}

impl<'doc, O: ByteOrder, D: Document> Iterator for ReadonlyCompoundIter<'doc, O, D> {
    type Item = (ReadonlyString<'doc, D>, ReadonlyValue<'doc, O, D>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let ptr = self.data;
            let tag_id = *ptr.cast::<TagID>();

            if tag_id == TagID::End {
                return None;
            }

            let name_len = byteorder::U16::<O>::from_bytes(*ptr.add(1).cast()).get() as usize;
            let name = ReadonlyString {
                data: MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(ptr.add(3), name_len)),
                _doc: self.doc.clone(),
            };

            self.data = ptr.add(3 + name_len);
            let value = read_and_advance::<O, D>(tag_id, &mut self.data, &mut self.mark, &self.doc);

            Some((name, value))
        }
    }
}
