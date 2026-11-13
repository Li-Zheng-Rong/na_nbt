use std::{borrow::Cow, fmt, marker::PhantomData, mem::ManuallyDrop, ptr, slice};

use zerocopy::{byteorder, BigEndian};

use crate::{
    cold_path,
    owned::{
        slot::{drop_compound_entries, mutable_tag_size, slot_write},
        OwnedPtr,
    },
    write::impl_inherent_write_methods,
    AsKey, ByteOrder, CompoundIter, CompoundRef, ElementTag, IntoNBT, List, ListRef, MUTF8Str,
    MUTF8String, Mut, Owned, ScalarMut, Tag, TagID, Value, ValueMut, ValueRef, EMPTY_COMPOUND,
};

/// An owned, flat-buffer NBT compound map (`TAG_Compound`).
#[repr(transparent)]
pub struct Compound<O: ByteOrder = BigEndian> {
    pub(crate) data: Vec<u8>,
    pub(crate) _marker: PhantomData<fn() -> O>,
}

impl<O: ByteOrder> Default for Compound<O> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! impl_compound_getters {
    ($($fn_name:ident -> $ret:ty),* $(,)?) => {
        $(
            #[doc = concat!("Looks up `key` and returns it if its tag matches (`", stringify!($fn_name), "`).")]
            #[inline]
            pub fn $fn_name(&self, key: impl AsKey) -> Option<$ret> {
                if self.is_empty() {
                    return None;
                }
                self.as_ref().$fn_name(key)
            }
        )*
    };
}

macro_rules! impl_compound_mut_getters {
    ($($fn_name:ident => ($tag:ty, $ret:ty)),* $(,)?) => {
        $(
            #[doc = concat!("Mutably borrows the entry at `key` if its tag matches `", stringify!($tag), "`.")]
            #[inline]
            pub fn $fn_name(&mut self, key: impl AsKey) -> Option<$ret> {
                self.get_mut_as::<$tag>(key)
            }
        )*
    };
}

impl<O: ByteOrder> Compound<O> {
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

    /// Returns `true` if this compound contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        let empty = self.data.len() <= 1;
        debug_assert_eq!(empty, unsafe { *self.raw_ptr() == 0 });
        empty
    }

    /// Returns `true` if `key` exists in this compound.
    #[inline]
    pub fn contains_key(&self, key: impl AsKey) -> bool {
        if self.is_empty() {
            return false;
        }
        self.as_ref().contains_key(key)
    }

    /// Looks up `key` for fallible chain navigation, returning [`ValueRef::End`] if missing.
    #[inline]
    pub fn at(&self, key: impl AsKey) -> ValueRef<'_, Owned<O>> {
        if self.is_empty() {
            return ValueRef::End;
        }
        self.as_ref().at(key)
    }

    /// Looks up `key` and returns its [`ValueRef`] if present.
    #[inline]
    pub fn get(&self, key: impl AsKey) -> Option<ValueRef<'_, Owned<O>>> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get(key)
    }

    /// Looks up `key` and downcasts it to Rust tag type `T` (`i32`, `str`, `[i8]`, [`Compound`],
    /// [`TypedList<T>`](crate::TypedList), etc.).
    #[inline]
    pub fn get_as<T: ?Sized + Tag>(&self, key: impl AsKey) -> Option<T::Ref<'_, Owned<O>>> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get_as::<T>(key)
    }

    impl_compound_getters!(
        get_bool -> bool,
        get_byte -> i8,
        get_short -> i16,
        get_int -> i32,
        get_long -> i64,
        get_float -> f32,
        get_double -> f64,
    );

    /// Looks up `key` as a `TAG_Byte_Array` (`&[i8]`).
    #[inline]
    pub fn get_byte_array(&self, key: impl AsKey) -> Option<&[i8]> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get_byte_array(key)
    }

    /// Looks up `key` as a zero-copy `TAG_String` (`&MUTF8Str`).
    #[inline]
    pub fn get_string(&self, key: impl AsKey) -> Option<&MUTF8Str> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get_string(key)
    }

    /// Looks up `key` as a `TAG_String` and decodes it to UTF-8 (`Cow<str>`).
    #[inline]
    pub fn get_str(&self, key: impl AsKey) -> Option<Cow<'_, str>> {
        self.get_string(key).map(MUTF8Str::decode_lossy)
    }

    /// Looks up `key` as a `TAG_List` ([`ListRef`]).
    #[inline]
    pub fn get_list(&self, key: impl AsKey) -> Option<ListRef<'_, Owned<O>>> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get_list(key)
    }

    /// Looks up `key` as a `TAG_Compound` ([`CompoundRef`]).
    #[inline]
    pub fn get_compound(&self, key: impl AsKey) -> Option<CompoundRef<'_, Owned<O>>> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get_compound(key)
    }

    /// Looks up `key` as a `TAG_Int_Array` (`&[I32<O>]`).
    #[inline]
    pub fn get_int_array(&self, key: impl AsKey) -> Option<&[byteorder::I32<O>]> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get_int_array(key)
    }

    /// Looks up `key` as a `TAG_Long_Array` (`&[I64<O>]`).
    #[inline]
    pub fn get_long_array(&self, key: impl AsKey) -> Option<&[byteorder::I64<O>]> {
        if self.is_empty() {
            return None;
        }
        self.as_ref().get_long_array(key)
    }

    /// Mutably borrows the entry at `key` as a [`ValueMut`] if present.
    #[inline]
    pub fn get_mut(&mut self, key: impl AsKey) -> Option<ValueMut<'_, O>> {
        if self.is_empty() {
            return None;
        }
        key.with_mutf8(|k| unsafe {
            let (_entry_ptr, tag_id, val_ptr) = find_entry_mut::<O>(&mut self.data, k)?;
            Some(ValueMut::from_slot(tag_id, val_ptr))
        })
    }

    /// Mutably borrows and downcasts the entry at `key` to Rust tag type `T`.
    #[inline]
    pub fn get_mut_as<T: ?Sized + Tag>(&mut self, key: impl AsKey) -> Option<T::Mut<'_, O>> {
        if self.is_empty() {
            return None;
        }
        key.with_mutf8(|k| unsafe {
            let (_entry_ptr, tag_id, val_ptr) = find_entry_mut::<O>(&mut self.data, k)?;
            if tag_id != T::TAG_ID {
                cold_path();
                return None;
            }
            T::read_mut::<O>(val_ptr)
        })
    }

    impl_compound_mut_getters!(
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

    #[inline]
    fn insert_raw<T: ?Sized + ElementTag>(
        &mut self,
        k: &MUTF8Str,
        new_val: T::Own<O>,
    ) -> Option<Value<O>> {
        let new_size = const { T::INLINE_SIZE };
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
                dst.write(T::TAG_ID as u8);
                ptr::write_unaligned(dst.add(1).cast::<[u8; 2]>(), name_len_raw);
                ptr::copy_nonoverlapping(name_bytes.as_ptr(), dst.add(3), name_len);
                T::write_own_to_slot::<O>(dst.add(3 + name_len), new_val);
                dst.add(3 + name_len + new_size).write(TagID::End as u8);
                self.data.set_len(total);
                return None;
            }

            if let Some((entry_ptr, old_tag, val_ptr)) = find_entry_mut::<O>(&mut self.data, k) {
                let old_val = Value::<O>::read_from_slot(old_tag, val_ptr);
                let old_size = mutable_tag_size(old_tag);
                if old_size == new_size {
                    entry_ptr.write(T::TAG_ID as u8);
                    T::write_own_to_slot::<O>(val_ptr, new_val);
                } else if new_size < old_size {
                    let old_total = self.data.len();
                    let val_offset = val_ptr.byte_offset_from_unsigned(self.data.as_mut_ptr());
                    entry_ptr.write(T::TAG_ID as u8);
                    T::write_own_to_slot::<O>(val_ptr, new_val);
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
                    base.add(entry_off).write(T::TAG_ID as u8);
                    T::write_own_to_slot::<O>(base.add(val_off), new_val);
                    self.data.set_len(old_total + grow);
                }
                return Some(old_val);
            }

            let old_len = self.data.len();
            let entry_bytes = 3 + name_len + new_size;
            self.data.reserve(entry_bytes);
            let dst = self.data.as_mut_ptr().add(old_len - 1);
            dst.write(T::TAG_ID as u8);
            ptr::write_unaligned(dst.add(1).cast::<[u8; 2]>(), name_len_raw);
            ptr::copy_nonoverlapping(name_bytes.as_ptr(), dst.add(3), name_len);
            T::write_own_to_slot::<O>(dst.add(3 + name_len), new_val);
            dst.add(entry_bytes).write(TagID::End as u8);
            self.data.set_len(old_len + entry_bytes);
            None
        }
    }

    /// Inserts or replaces an entry, returning the previous [`Value`] if `key` already existed.
    ///
    /// # Panics
    ///
    /// Panics if `V::Tag` is `()` (`TagID::End`) or if the MUTF-8 encoded key exceeds `u16::MAX` bytes.
    pub fn insert<V: IntoNBT<O>>(&mut self, key: impl AsKey, value: V) -> Option<Value<O>> {
        assert!(
            const { V::Tag::TAG_ID as u8 != TagID::End as u8 },
            "End cannot be inserted into a compound"
        );
        let new_val = value.into_nbt();
        key.with_mutf8(|k| self.insert_raw::<V::Tag>(k, new_val))
    }

    /// Inserts or replaces a dynamically typed [`Value`], returning the previous value if `key` existed.
    ///
    /// # Panics
    ///
    /// Panics if `value` is [`Value::End`] or if the MUTF-8 encoded key exceeds `u16::MAX` bytes.
    pub fn insert_value(&mut self, key: impl AsKey, value: Value<O>) -> Option<Value<O>> {
        key.with_mutf8(|k| match value {
            Value::End => panic!("End cannot be inserted into a compound"),
            Value::Byte(v) => self.insert_raw::<i8>(k, v),
            Value::Short(v) => self.insert_raw::<i16>(k, v),
            Value::Int(v) => self.insert_raw::<i32>(k, v),
            Value::Long(v) => self.insert_raw::<i64>(k, v),
            Value::Float(v) => self.insert_raw::<f32>(k, v),
            Value::Double(v) => self.insert_raw::<f64>(k, v),
            Value::ByteArray(v) => self.insert_raw::<[i8]>(k, v),
            Value::String(v) => self.insert_raw::<str>(k, v),
            Value::List(v) => self.insert_raw::<List<BigEndian>>(k, v),
            Value::Compound(v) => self.insert_raw::<Compound<BigEndian>>(k, v),
            Value::IntArray(v) => self.insert_raw::<[i32]>(k, v),
            Value::LongArray(v) => self.insert_raw::<[i64]>(k, v),
        })
    }

    /// Removes and returns the entry at `key`, or `None` if absent.
    #[inline]
    pub fn remove(&mut self, key: impl AsKey) -> Option<Value<O>> {
        if self.is_empty() {
            return None;
        }
        key.with_mutf8(|k| unsafe {
            let (entry_ptr, tag_id, val_ptr) = find_entry_mut::<O>(&mut self.data, k)?;
            let tag_size = mutable_tag_size(tag_id);
            let value = Value::<O>::read_from_slot(tag_id, val_ptr);
            let base = self.data.as_mut_ptr();
            let after_off = val_ptr.add(tag_size).byte_offset_from_unsigned(base);
            let total_len = self.data.len();
            let removed_len = 3 + k.len() + tag_size;
            ptr::copy(val_ptr.add(tag_size), entry_ptr, total_len - after_off);
            self.data.set_len(total_len - removed_len);
            Some(value)
        })
    }

    /// Removes and returns an entry only if its tag matches `T` (including [`crate::TypedList<E>`]);
    /// leaves the compound unchanged and returns `None` otherwise.
    #[inline]
    pub fn remove_as<T: ?Sized + Tag>(&mut self, key: impl AsKey) -> Option<T::Own<O>> {
        if self.is_empty() {
            return None;
        }
        key.with_mutf8(|k| unsafe {
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

    /// Removes all entries from this compound, retaining the underlying buffer capacity.
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

    /// Returns an immutable iterator over `(&MUTF8Str, ValueRef)` entries.
    #[inline]
    pub fn iter(&self) -> CompoundIter<'_, Owned<O>> {
        self.as_ref().iter()
    }

    /// Returns a mutable iterator over `(&MUTF8Str, ValueMut)` entries.
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

    impl_inherent_write_methods!();
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

impl<O: ByteOrder> Drop for Compound<O> {
    fn drop(&mut self) {
        if !self.data.is_empty() {
            unsafe {
                let ptr = self.data.as_mut_ptr();
                drop_compound_entries::<O>(ptr, ptr.add(self.data.len()));
            }
        }
    }
}

impl<O: ByteOrder> Clone for Compound<O> {
    fn clone(&self) -> Self {
        if self.is_empty() {
            return Self::new();
        }
        let mut guard = Compound {
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
                    TagID::List => clone_slot!(List<O>),
                    TagID::Compound => clone_slot!(Compound<O>),
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

impl<K: AsKey, V: IntoNBT<O>, O: ByteOrder> Extend<(K, V)> for Compound<O> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

impl<K: AsKey, V: IntoNBT<O>, O: ByteOrder> FromIterator<(K, V)> for Compound<O> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut comp = Self::new();
        comp.extend(iter);
        comp
    }
}

impl<'a, O: ByteOrder> IntoIterator for &'a Compound<O> {
    type Item = (&'a MUTF8Str, ValueRef<'a, Owned<O>>);
    type IntoIter = CompoundIter<'a, Owned<O>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, O: ByteOrder> IntoIterator for &'a mut Compound<O> {
    type Item = (&'a MUTF8Str, ValueMut<'a, O>);
    type IntoIter = CompoundIterMut<'a, O>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<O: ByteOrder> fmt::Debug for Compound<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

/// Mutable iterator over key-value entries of a [`Compound`].
pub struct CompoundIterMut<'a, O: ByteOrder = BigEndian> {
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

impl<O: ByteOrder> PartialEq for Compound<O> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<O: ByteOrder, S: crate::Source> PartialEq<CompoundRef<'_, S>> for Compound<O> {
    #[inline]
    fn eq(&self, other: &CompoundRef<'_, S>) -> bool {
        self.as_ref() == *other
    }
}
