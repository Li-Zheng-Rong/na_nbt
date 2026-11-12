use std::{marker::PhantomData, ptr, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigRef, EMPTY_LIST, ListBase, ListRef, MUTF8Str, NBT, TagID,
    mutable::{
        compound_ref::RefCompound,
        config::MutableConfig,
        size::{SIZE_DYN, SIZE_USIZE},
        string_ref::RefString,
        typed_list_ref::RefTypedList,
        value_ref::RefValue,
    },
};

#[inline(always)]
pub(crate) unsafe fn read_ref_and_advance<'s, O: ByteOrder>(
    tag_id: TagID,
    data: &mut *const u8,
) -> RefValue<'s, O> {
    unsafe {
        let ptr = *data;
        match tag_id {
            TagID::End => RefValue::End(()),
            TagID::Byte => {
                *data = ptr.add(1);
                RefValue::Byte(*ptr.cast())
            }
            TagID::Short => {
                *data = ptr.add(2);
                RefValue::Short(byteorder::I16::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::Int => {
                *data = ptr.add(4);
                RefValue::Int(byteorder::I32::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::Long => {
                *data = ptr.add(8);
                RefValue::Long(byteorder::I64::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::Float => {
                *data = ptr.add(4);
                RefValue::Float(byteorder::F32::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::Double => {
                *data = ptr.add(8);
                RefValue::Double(byteorder::F64::<O>::from_bytes(*ptr.cast()).get())
            }
            TagID::ByteArray => {
                let p: *const i8 = ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
                let len = usize::from_ne_bytes(*ptr.add(SIZE_USIZE).cast());
                *data = ptr.add(SIZE_DYN);
                RefValue::ByteArray(slice::from_raw_parts(p, len))
            }
            TagID::String => {
                let p: *const u8 = ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
                let len = usize::from_ne_bytes(*ptr.add(SIZE_USIZE).cast());
                *data = ptr.add(SIZE_DYN);
                RefValue::String(RefString {
                    data: MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(p, len)),
                })
            }
            TagID::List => {
                let p: *const u8 = ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
                *data = ptr.add(SIZE_DYN);
                RefValue::List(RefList {
                    data: p,
                    _marker: PhantomData,
                })
            }
            TagID::Compound => {
                let p: *const u8 = ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
                *data = ptr.add(SIZE_DYN);
                RefValue::Compound(RefCompound {
                    data: p,
                    _marker: PhantomData,
                })
            }
            TagID::IntArray => {
                let p: *const byteorder::I32<O> =
                    ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
                let len = usize::from_ne_bytes(*ptr.add(SIZE_USIZE).cast());
                *data = ptr.add(SIZE_DYN);
                RefValue::IntArray(slice::from_raw_parts(p, len))
            }
            TagID::LongArray => {
                let p: *const byteorder::I64<O> =
                    ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
                let len = usize::from_ne_bytes(*ptr.add(SIZE_USIZE).cast());
                *data = ptr.add(SIZE_DYN);
                RefValue::LongArray(slice::from_raw_parts(p, len))
            }
        }
    }
}

#[derive(Clone)]
#[repr(transparent)]
pub struct RefList<'s, O: ByteOrder> {
    pub(crate) data: *const u8,
    pub(crate) _marker: PhantomData<(&'s (), O)>,
}

impl<'s, O: ByteOrder> Default for RefList<'s, O> {
    #[inline]
    fn default() -> Self {
        Self {
            data: EMPTY_LIST.as_ptr(),
            _marker: PhantomData,
        }
    }
}

unsafe impl<'s, O: ByteOrder> Send for RefList<'s, O> {}
unsafe impl<'s, O: ByteOrder> Sync for RefList<'s, O> {}

impl<'s, O: ByteOrder> IntoIterator for RefList<'s, O> {
    type Item = RefValue<'s, O>;
    type IntoIter = RefListIter<'s, O>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        RefListIter {
            tag_id: self.element_tag_id(),
            remaining: self.len() as u32,
            data: unsafe { self.data.add(1 + 4) },
            _marker: PhantomData,
        }
    }
}

impl<'s, O: ByteOrder> ListBase for RefList<'s, O> {
    #[inline]
    fn element_tag_id(&self) -> TagID {
        unsafe { *self.data.cast() }
    }

    #[inline]
    fn len(&self) -> usize {
        unsafe { byteorder::U32::<O>::from_bytes(*self.data.add(1).cast()).get() as usize }
    }
}

impl<'s, O: ByteOrder> ListRef<'s> for RefList<'s, O> {
    type Config = MutableConfig<O>;

    #[inline]
    fn _to_read_params<'a>(&'a self) -> <Self::Config as ConfigRef>::ReadParams<'a>
    where
        's: 'a,
    {
        unsafe { self.data.add(1 + 4) }
    }

    #[inline]
    fn typed_<T: NBT>(self) -> Option<<Self::Config as ConfigRef>::TypedList<'s, T>> {
        self.element_is_::<T>().then_some(RefTypedList {
            data: self.data,
            _marker: PhantomData,
        })
    }

    #[inline]
    fn iter(&self) -> <Self::Config as ConfigRef>::ListIter<'s> {
        RefListIter {
            tag_id: self.element_tag_id(),
            remaining: self.len() as u32,
            data: unsafe { self.data.add(1 + 4) },
            _marker: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct RefListIter<'s, O: ByteOrder> {
    pub(crate) tag_id: TagID,
    pub(crate) remaining: u32,
    pub(crate) data: *const u8,
    pub(crate) _marker: PhantomData<(&'s (), O)>,
}

impl<'s, O: ByteOrder> Default for RefListIter<'s, O> {
    #[inline]
    fn default() -> Self {
        Self {
            tag_id: TagID::End,
            remaining: 0,
            data: ptr::null(),
            _marker: PhantomData,
        }
    }
}

unsafe impl<'s, O: ByteOrder> Send for RefListIter<'s, O> {}
unsafe impl<'s, O: ByteOrder> Sync for RefListIter<'s, O> {}

impl<'s, O: ByteOrder> Iterator for RefListIter<'s, O> {
    type Item = RefValue<'s, O>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(unsafe { read_ref_and_advance::<O>(self.tag_id, &mut self.data) })
    }

    #[inline]
    fn fold<B, F>(self, mut accum: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        let mut ptr = self.data;
        let n = self.remaining;
        macro_rules! fold_by_tag {
            ($($tag:ident),* $(,)?) => {
                match self.tag_id {
                    $(TagID::$tag => {
                        for _ in 0..n {
                            let item = unsafe { read_ref_and_advance::<O>(TagID::$tag, &mut ptr) };
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

impl<'s, O: ByteOrder> ExactSizeIterator for RefListIter<'s, O> {}
