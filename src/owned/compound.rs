use std::{fmt, io::Write, marker::PhantomData, mem::ManuallyDrop, ptr, slice};

use zerocopy::byteorder;

use crate::{
    cold_path,
    owned::{
        slot::{drop_compound_entries, mutable_tag_size, slot_write},
        write_compound_sink, OwnedPtr,
    },
    with_mutf8_key,
    write::{sink_to_vec, ByteSink, IoSink},
    ByteOrder, CompoundIter, CompoundRef, ElementTag, IntoNBT, ListOwn, MUTF8Str, MUTF8String,
    Owned, Result, Tag, TagID, ValueMut, ValueOwn, ValueRef, Writable, EMPTY_COMPOUND,
};

/// An owned, flat-buffer NBT compound map.
#[repr(transparent)]
pub struct CompoundOwn<O: ByteOrder> {
    pub(crate) data: Vec<u8>,
    pub(crate) _marker: PhantomData<fn() -> O>,
}

impl<O: ByteOrder> Default for CompoundOwn<O> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<O: ByteOrder> CompoundOwn<O> {
    /// Creates an empty compound without heap allocation.
    #[inline]
    pub const fn new() -> Self {
        Self {
            data: Vec::new(),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn raw_ptr(&self) -> *const u8 {
        if self.data.is_empty() {
            EMPTY_COMPOUND.as_ptr()
        } else {
            self.data.as_ptr()
        }
    }

    /// Returns a lightweight `Copy` read-only view of this compound.
    #[inline]
    pub fn as_ref(&self) -> CompoundRef<'_, Owned<O>> {
        CompoundRef::from_cursor(OwnedPtr {
            ptr: self.raw_ptr(),
            _marker: PhantomData,
        })
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        let empty = self.data.len() <= 1;
        debug_assert_eq!(empty, unsafe { *self.raw_ptr() == 0 });
        empty
    }

    #[inline]
    pub fn contains_key(&self, key: &str) -> bool {
        if self.is_empty() {
            return false;
        }
        self.as_ref().contains_key(key)
    }

    #[inline]
    pub fn get(&self, key: &str) -> Option<ValueRef<'_, Owned<O>>> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get(key)
    }

    #[inline]
    pub fn get_as<T: Tag>(&self, key: &str) -> Option<T::Ref<'_, Owned<O>>> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get_as::<T>(key)
    }

    #[inline]
    pub fn get_mut(&mut self, key: &str) -> Option<ValueMut<'_, O>> {
        if self.is_empty() {
            return None;
        }
        with_mutf8_key(key, |k| unsafe {
            let (_entry_ptr, tag_id, val_ptr) = find_entry_mut::<O>(&mut self.data, k)?;
            Some(ValueMut::from_slot(tag_id, val_ptr))
        })
    }

    #[inline]
    pub fn get_mut_as<T: Tag>(&mut self, key: &str) -> Option<T::Mut<'_, O>> {
        if self.is_empty() {
            return None;
        }
        with_mutf8_key(key, |k| unsafe {
            let (_entry_ptr, tag_id, val_ptr) = find_entry_mut::<O>(&mut self.data, k)?;
            if tag_id != T::TAG_ID {
                cold_path();
                return None;
            }
            T::read_mut::<O>(val_ptr)
        })
    }

    /// Inserts or replaces an entry, returning the previous value if the key existed.
    pub fn insert<V: IntoNBT<O>>(&mut self, key: &str, value: V) -> Option<ValueOwn<O>> {
        assert!(
            const { V::Tag::TAG_ID as u8 != TagID::End as u8 },
            "End cannot be inserted into a compound"
        );
        let new_val = value.into_nbt();
        let new_size = const { <V::Tag as ElementTag>::INLINE_SIZE };

        with_mutf8_key(key, |k| {
            let name_bytes = k.as_bytes();
            assert!(
                name_bytes.len() <= u16::MAX as usize,
                "compound key length too long"
            );
            let name_len = name_bytes.len();
            let name_len_raw = byteorder::U16::<O>::new(name_len as u16).to_bytes();

            unsafe {
                if self.data.is_empty() {
                    let total = 3 + name_len + new_size + 1;
                    self.data.reserve(total);
                    let dst = self.data.as_mut_ptr();
                    dst.write(V::Tag::TAG_ID as u8);
                    ptr::write_unaligned(dst.add(1).cast::<[u8; 2]>(), name_len_raw);
                    ptr::copy_nonoverlapping(name_bytes.as_ptr(), dst.add(3), name_len);
                    <V::Tag as ElementTag>::write_own_to_slot::<O>(dst.add(3 + name_len), new_val);
                    dst.add(3 + name_len + new_size).write(TagID::End as u8);
                    self.data.set_len(total);
                    return None;
                }

                if let Some((entry_ptr, old_tag, val_ptr)) = find_entry_mut::<O>(&mut self.data, k)
                {
                    let old_val = ValueOwn::<O>::read_from_slot(old_tag, val_ptr);
                    let old_size = mutable_tag_size(old_tag);
                    if old_size == new_size {
                        entry_ptr.write(V::Tag::TAG_ID as u8);
                        <V::Tag as ElementTag>::write_own_to_slot::<O>(val_ptr, new_val);
                    } else if new_size < old_size {
                        let old_total = self.data.len();
                        let val_offset = val_ptr.byte_offset_from_unsigned(self.data.as_mut_ptr());
                        entry_ptr.write(V::Tag::TAG_ID as u8);
                        <V::Tag as ElementTag>::write_own_to_slot::<O>(val_ptr, new_val);
                        let tail_len = old_total - val_offset - old_size;
                        ptr::copy(val_ptr.add(old_size), val_ptr.add(new_size), tail_len);
                        self.data.set_len(old_total - (old_size - new_size));
                    } else {
                        let grow = new_size - old_size;
                        let old_total = self.data.len();
                        let entry_off = entry_ptr.byte_offset_from_unsigned(self.data.as_mut_ptr());
                        let val_off = val_ptr.byte_offset_from_unsigned(self.data.as_mut_ptr());
                        self.data.reserve(grow);
                        let base = self.data.as_mut_ptr();
                        let tail_len = old_total - val_off - old_size;
                        ptr::copy(
                            base.add(val_off + old_size),
                            base.add(val_off + new_size),
                            tail_len,
                        );
                        base.add(entry_off).write(V::Tag::TAG_ID as u8);
                        <V::Tag as ElementTag>::write_own_to_slot::<O>(base.add(val_off), new_val);
                        self.data.set_len(old_total + grow);
                    }
                    return Some(old_val);
                }

                let old_len = self.data.len();
                let entry_bytes = 3 + name_len + new_size;
                self.data.reserve(entry_bytes);
                let dst = self.data.as_mut_ptr().add(old_len - 1);
                dst.write(V::Tag::TAG_ID as u8);
                ptr::write_unaligned(dst.add(1).cast::<[u8; 2]>(), name_len_raw);
                ptr::copy_nonoverlapping(name_bytes.as_ptr(), dst.add(3), name_len);
                <V::Tag as ElementTag>::write_own_to_slot::<O>(dst.add(3 + name_len), new_val);
                dst.add(entry_bytes).write(TagID::End as u8);
                self.data.set_len(old_len + entry_bytes);
                None
            }
        })
    }

    #[inline]
    pub fn remove(&mut self, key: &str) -> Option<ValueOwn<O>> {
        if self.is_empty() {
            return None;
        }
        with_mutf8_key(key, |k| unsafe {
            let (entry_ptr, tag_id, val_ptr) = find_entry_mut::<O>(&mut self.data, k)?;
            let tag_size = mutable_tag_size(tag_id);
            let value = ValueOwn::<O>::read_from_slot(tag_id, val_ptr);
            let base = self.data.as_mut_ptr();
            let after_off = val_ptr.add(tag_size).byte_offset_from_unsigned(base);
            let total_len = self.data.len();
            let removed_len = 3 + k.len() + tag_size;
            ptr::copy(val_ptr.add(tag_size), entry_ptr, total_len - after_off);
            self.data.set_len(total_len - removed_len);
            Some(value)
        })
    }

    /// Removes and returns an entry only if its tag matches `T` (including [`crate::tag::TypedList<E>`]);
    /// leaves the compound unchanged and returns `None` otherwise.
    #[inline]
    pub fn remove_as<T: Tag>(&mut self, key: &str) -> Option<T::Own<O>> {
        if self.is_empty() {
            return None;
        }
        with_mutf8_key(key, |k| unsafe {
            let (entry_ptr, tag_id, val_ptr) = find_entry_mut::<O>(&mut self.data, k)?;
            if tag_id != T::TAG_ID || !T::can_take_from_slot::<O>(val_ptr) {
                cold_path();
                return None;
            }
            let tag_size = mutable_tag_size(tag_id);
            let value = T::take_own_from_slot::<O>(val_ptr);
            let base = self.data.as_mut_ptr();
            let after_off = val_ptr.add(tag_size).byte_offset_from_unsigned(base);
            let total_len = self.data.len();
            let removed_len = 3 + k.len() + tag_size;
            ptr::copy(val_ptr.add(tag_size), entry_ptr, total_len - after_off);
            self.data.set_len(total_len - removed_len);
            Some(value)
        })
    }

    #[inline]
    pub fn clear(&mut self) {
        if !self.data.is_empty() {
            unsafe {
                let ptr = self.data.as_mut_ptr();
                drop_compound_entries::<O>(ptr, ptr.add(self.data.len()));
            }
            self.data.clear();
        }
    }

    #[inline]
    pub fn iter(&self) -> CompoundIter<'_, Owned<O>> {
        self.as_ref().iter()
    }

    #[inline]
    pub fn iter_mut(&mut self) -> CompoundIterMut<'_, O> {
        let data = if self.data.is_empty() {
            ptr::null_mut()
        } else {
            self.data.as_mut_ptr()
        };
        CompoundIterMut {
            data,
            _marker: PhantomData,
        }
    }
}

#[inline]
unsafe fn find_entry_mut<O: ByteOrder>(
    data: &mut [u8],
    key: &MUTF8Str,
) -> Option<(*mut u8, TagID, *mut u8)> {
    let key_bytes = key.as_bytes();
    let key_len = key_bytes.len();
    if key_len > u16::MAX as usize {
        cold_path();
        return None;
    }
    let key_len_raw = byteorder::U16::<O>::new(key_len as u16).to_bytes();

    unsafe {
        let mut ptr = data.as_mut_ptr();
        loop {
            let tag_id = *ptr.cast::<TagID>();
            if tag_id == TagID::End {
                return None;
            }
            let raw_len = *ptr.add(1).cast::<[u8; 2]>();
            if raw_len == key_len_raw && slice::from_raw_parts(ptr.add(3), key_len) == key_bytes {
                return Some((ptr, tag_id, ptr.add(3 + key_len)));
            }
            let name_len = byteorder::U16::<O>::from_bytes(raw_len).get() as usize;
            ptr = ptr.add(3 + name_len + mutable_tag_size(tag_id));
        }
    }
}

impl<O: ByteOrder> Drop for CompoundOwn<O> {
    fn drop(&mut self) {
        if !self.data.is_empty() {
            unsafe {
                let ptr = self.data.as_mut_ptr();
                drop_compound_entries::<O>(ptr, ptr.add(self.data.len()));
            }
        }
    }
}

impl<O: ByteOrder> Clone for CompoundOwn<O> {
    fn clone(&self) -> Self {
        if self.is_empty() {
            return Self::new();
        }
        let mut guard = CompoundOwn {
            data: Vec::with_capacity(self.data.len()),
            _marker: PhantomData,
        };
        unsafe {
            let src_base = self.data.as_ptr();
            let dst_base = guard.data.as_mut_ptr();
            ptr::copy_nonoverlapping(src_base, dst_base, self.data.len());
            dst_base.write(TagID::End as u8);
            guard.data.set_len(1);
            let mut off = 0usize;
            loop {
                let tag_id = *src_base.add(off).cast::<TagID>();
                if tag_id == TagID::End {
                    return guard;
                }
                let name_len =
                    byteorder::U16::<O>::from_bytes(*src_base.add(off + 1).cast()).get() as usize;
                let val_off = off + 3 + name_len;
                let size = mutable_tag_size(tag_id);
                let src_slot = src_base.add(val_off);
                let dst_slot = dst_base.add(val_off);

                macro_rules! clone_slot {
                    ($ty:ty) => {{
                        let borrowed = ptr::read_unaligned(src_slot.cast::<ManuallyDrop<$ty>>());
                        let cloned: $ty = (*borrowed).clone();
                        slot_write(dst_slot, cloned);
                    }};
                }

                match tag_id {
                    TagID::ByteArray => clone_slot!(Vec<i8>),
                    TagID::String => clone_slot!(MUTF8String),
                    TagID::List => clone_slot!(ListOwn<O>),
                    TagID::Compound => clone_slot!(CompoundOwn<O>),
                    TagID::IntArray => clone_slot!(Vec<byteorder::I32<O>>),
                    TagID::LongArray => clone_slot!(Vec<byteorder::I64<O>>),
                    _ => {}
                }
                // Commit entry `off` and advance the temporary `End` sentinel for panic safety
                dst_base.add(off).write(tag_id as u8);
                let next_off = val_off + size;
                dst_base.add(next_off).write(TagID::End as u8);
                guard.data.set_len(next_off + 1);
                off = next_off;
            }
        }
    }
}

impl<'a, O: ByteOrder> IntoIterator for &'a CompoundOwn<O> {
    type Item = (&'a MUTF8Str, ValueRef<'a, Owned<O>>);
    type IntoIter = CompoundIter<'a, Owned<O>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, O: ByteOrder> IntoIterator for &'a mut CompoundOwn<O> {
    type Item = (&'a MUTF8Str, ValueMut<'a, O>);
    type IntoIter = CompoundIterMut<'a, O>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<O: ByteOrder> fmt::Debug for CompoundOwn<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

impl<O: ByteOrder> Writable for CompoundOwn<O> {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        sink_to_vec(3 + self.data.len().max(1), |out| unsafe {
            out.write_bytes(&[TagID::Compound as u8, 0, 0])?;
            write_compound_sink::<O, TARGET, _>(self.raw_ptr(), out)
        })
    }

    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        let mut sink = IoSink(writer);
        unsafe {
            sink.write_bytes(&[TagID::Compound as u8, 0, 0])?;
            write_compound_sink::<O, TARGET, _>(self.raw_ptr(), &mut sink)
        }
    }
}

/// Mutable iterator over key-value entries of a [`CompoundOwn`].
pub struct CompoundIterMut<'a, O: ByteOrder> {
    data: *mut u8,
    _marker: PhantomData<&'a mut O>,
}

unsafe impl<O: ByteOrder> Send for CompoundIterMut<'_, O> {}
unsafe impl<O: ByteOrder> Sync for CompoundIterMut<'_, O> {}

impl<O: ByteOrder> Default for CompoundIterMut<'_, O> {
    #[inline]
    fn default() -> Self {
        Self {
            data: ptr::null_mut(),
            _marker: PhantomData,
        }
    }
}

impl<'a, O: ByteOrder> Iterator for CompoundIterMut<'a, O> {
    type Item = (&'a MUTF8Str, ValueMut<'a, O>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.data.is_null() {
            return None;
        }
        unsafe {
            let tag_id = *self.data.cast::<TagID>();
            if tag_id == TagID::End {
                return None;
            }
            let name_len = byteorder::U16::<O>::from_bytes(*self.data.add(1).cast()).get() as usize;
            let name =
                MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(self.data.add(3), name_len));
            let val_ptr = self.data.add(3 + name_len);
            let value = ValueMut::from_slot(tag_id, val_ptr);
            self.data = val_ptr.add(mutable_tag_size(tag_id));
            Some((name, value))
        }
    }
}
