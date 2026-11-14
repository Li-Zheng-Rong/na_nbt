use std::{
    fmt,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ptr, slice,
};

use zerocopy::{byteorder, BigEndian};

use crate::{
    cold_path,
    owned::{
        compound::clone_compound_raw,
        slot::{
            drop_list_elements, mutable_tag_size, slot_read, slot_write, vec_copy_endian, SIZE_DYN,
        },
        OwnedPtr,
    },
    same_byte_order,
    write::{impl_inherent_write_methods, swap_u16, swap_u32, swap_u64},
    ByteOrder, Compound, ElementTag, IntoNBT, ListIter, ListKind, ListRef, MUTF8String, Owned, Tag,
    TagID, TypedListIter, TypedListRef, Value, ValueMut, ValueRef, EMPTY_LIST, EMPTY_TYPED_LISTS,
};

#[inline(always)]
unsafe fn write_list_header<O: ByteOrder>(ptr: *mut u8, tag_id: TagID, len: u32) {
    unsafe {
        ptr.write(tag_id as u8);
        ptr::write_unaligned(
            ptr.add(1).cast::<[u8; 4]>(),
            byteorder::U32::<O>::new(len).to_bytes(),
        );
    }
}

#[inline(always)]
fn raw_list_len<O: ByteOrder>(data: &[u8]) -> usize {
    if data.is_empty() {
        0
    } else {
        byteorder::U32::<O>::from_bytes(unsafe { *data.as_ptr().add(1).cast() }).get() as usize
    }
}

#[inline(always)]
unsafe fn raw_push_elem<T: ?Sized + ElementTag, O: ByteOrder>(
    data: &mut Vec<u8>,
    count: usize,
    val: T::Own<O>,
) {
    assert!(count < u32::MAX as usize, "list length too long");
    let elem_size = const { T::INLINE_SIZE };
    unsafe {
        if data.is_empty() {
            data.reserve(5 + elem_size);
            let base = data.as_mut_ptr();
            write_list_header::<O>(base, T::TAG_ID, 1);
            T::write_own_to_slot::<O>(base.add(5), val);
            data.set_len(5 + elem_size);
        } else {
            let old_bytes = data.len();
            data.reserve(elem_size);
            let base = data.as_mut_ptr();
            write_list_header::<O>(base, T::TAG_ID, (count + 1) as u32);
            T::write_own_to_slot::<O>(base.add(old_bytes), val);
            data.set_len(old_bytes + elem_size);
        }
    }
}

#[inline(always)]
unsafe fn raw_insert_elem<T: ?Sized + ElementTag, O: ByteOrder>(
    data: &mut Vec<u8>,
    count: usize,
    index: usize,
    val: T::Own<O>,
) {
    assert!(count < u32::MAX as usize, "list length too long");
    let elem_size = const { T::INLINE_SIZE };
    unsafe {
        if data.is_empty() {
            data.reserve(5 + elem_size);
            let base = data.as_mut_ptr();
            write_list_header::<O>(base, T::TAG_ID, 1);
            T::write_own_to_slot::<O>(base.add(5), val);
            data.set_len(5 + elem_size);
        } else {
            let old_bytes = data.len();
            let pos_bytes = 5 + index * elem_size;
            data.reserve(elem_size);
            let base = data.as_mut_ptr();
            let slot = base.add(pos_bytes);
            ptr::copy(slot, slot.add(elem_size), old_bytes - pos_bytes);
            T::write_own_to_slot::<O>(slot, val);
            write_list_header::<O>(base, T::TAG_ID, (count + 1) as u32);
            data.set_len(old_bytes + elem_size);
        }
    }
}

/// An owned, flat-buffer dynamic NBT list (`TAG_List`).
#[repr(transparent)]
pub struct List<O: ByteOrder = BigEndian> {
    pub(crate) data: Vec<u8>,
    pub(crate) _marker: PhantomData<fn() -> O>,
}

impl<O: ByteOrder> Default for List<O> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! impl_list_prim_slices {
    ($($fn_ref:ident / $fn_mut:ident => ($variant:ident, $elem_ty:ty)),* $(,)?) => {
        $(
            #[doc = concat!("Returns the contiguous slice if this is a list of `TagID::", stringify!($variant), "` (or empty).")]
            #[inline]
            pub fn $fn_ref(&self) -> Option<&[$elem_ty]> {
                let len = self.len();
                let tag_id = self.element_tag_id();
                if len == 0 && (tag_id == TagID::$variant || tag_id == TagID::End) {
                    return Some(&[]);
                }
                if tag_id != TagID::$variant {
                    cold_path();
                    return None;
                }
                Some(unsafe { slice::from_raw_parts(self.data.as_ptr().add(5).cast::<$elem_ty>(), len) })
            }

            #[doc = concat!("Returns a mutable contiguous slice if this is a list of `TagID::", stringify!($variant), "` (or empty).")]
            #[inline]
            pub fn $fn_mut(&mut self) -> Option<&mut [$elem_ty]> {
                let len = self.len();
                let tag_id = self.element_tag_id();
                if len == 0 && (tag_id == TagID::$variant || tag_id == TagID::End) {
                    return Some(&mut []);
                }
                if tag_id != TagID::$variant {
                    cold_path();
                    return None;
                }
                Some(unsafe {
                    slice::from_raw_parts_mut(self.data.as_mut_ptr().add(5).cast::<$elem_ty>(), len)
                })
            }
        )*
    };
}

impl<O: ByteOrder> List<O> {
    /// Creates an empty list (`element_tag_id == TagID::End`) without heap allocation.
    #[inline]
    pub const fn new() -> Self {
        Self {
            data: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Creates an empty list pre-tagged with element type `T`.
    #[inline]
    pub fn with_element<T: ?Sized + ElementTag>() -> Self {
        Self::with_tag_id(T::TAG_ID)
    }

    /// Creates an empty list pre-tagged with the given `tag_id`.
    #[inline]
    pub fn with_tag_id(tag_id: TagID) -> Self {
        if tag_id == TagID::End {
            Self::new()
        } else {
            Self {
                data: vec![tag_id as u8, 0, 0, 0, 0],
                _marker: PhantomData,
            }
        }
    }

    #[inline(always)]
    pub(crate) fn raw_ptr(&self) -> *const u8 {
        if self.data.is_empty() {
            EMPTY_LIST.as_ptr()
        } else {
            self.data.as_ptr()
        }
    }

    /// Returns a lightweight `Copy` read-only view of this list.
    #[inline]
    pub fn as_ref(&self) -> ListRef<'_, Owned<O>> {
        ListRef::from_cursor(OwnedPtr {
            ptr: self.raw_ptr(),
            _marker: PhantomData,
        })
    }

    /// Returns the homogeneous [`TagID`] of this list's elements.
    #[inline]
    pub fn element_tag_id(&self) -> TagID {
        if self.data.is_empty() {
            TagID::End
        } else {
            unsafe { *self.data.as_ptr().cast() }
        }
    }

    /// Returns `true` if this list has element type `T` or is empty.
    #[inline]
    pub fn element_is<T: ?Sized + ElementTag>(&self) -> bool {
        self.element_tag_id() == T::TAG_ID || self.is_empty()
    }

    /// Returns the number of elements in the list.
    #[inline]
    pub fn len(&self) -> usize {
        raw_list_len::<O>(&self.data)
    }

    /// Returns `true` if the list contains zero elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    impl_list_prim_slices!(
        bytes / bytes_mut => (Byte, i8),
        shorts / shorts_mut => (Short, byteorder::I16<O>),
        ints / ints_mut => (Int, byteorder::I32<O>),
        longs / longs_mut => (Long, byteorder::I64<O>),
        floats / floats_mut => (Float, byteorder::F32<O>),
        doubles / doubles_mut => (Double, byteorder::F64<O>),
    );

    /// Returns a statically typed view if this is a list of `TAG_Byte_Array` (or empty).
    #[inline]
    pub fn byte_arrays(&self) -> Option<TypedListRef<'_, [i8], Owned<O>>> {
        self.as_ref().byte_arrays()
    }

    /// Returns a statically typed view if this is a list of `TAG_String` (or empty).
    #[inline]
    pub fn strings(&self) -> Option<TypedListRef<'_, str, Owned<O>>> {
        self.as_ref().strings()
    }

    /// Returns a statically typed view if this is a list of `TAG_List` (or empty).
    #[inline]
    pub fn lists(&self) -> Option<TypedListRef<'_, List<BigEndian>, Owned<O>>> {
        self.as_ref().lists()
    }

    /// Returns a statically typed view if this is a list of `TAG_Compound` (or empty).
    #[inline]
    pub fn compounds(&self) -> Option<TypedListRef<'_, Compound<BigEndian>, Owned<O>>> {
        self.as_ref().compounds()
    }

    /// Returns a statically typed view if this is a list of `TAG_Int_Array` (or empty).
    #[inline]
    pub fn int_arrays(&self) -> Option<TypedListRef<'_, [i32], Owned<O>>> {
        self.as_ref().int_arrays()
    }

    /// Returns a statically typed view if this is a list of `TAG_Long_Array` (or empty).
    #[inline]
    pub fn long_arrays(&self) -> Option<TypedListRef<'_, [i64], Owned<O>>> {
        self.as_ref().long_arrays()
    }

    /// Dispatches `element_tag_id` once and returns a homogeneous [`ListKind`] enum.
    #[inline]
    pub fn typed(&self) -> ListKind<'_, Owned<O>> {
        self.as_ref().typed()
    }

    /// Downcasts this list to a statically typed [`TypedListRef<T, Owned<O>>`] if its element
    /// type matches `T` (or if the list is empty).
    #[inline]
    pub fn typed_as<T: ?Sized + ElementTag>(&self) -> Option<TypedListRef<'_, T, Owned<O>>> {
        self.as_ref().typed_as::<T>()
    }

    /// Converts this dynamic list into a statically typed [`TypedList<T, O>`] if its element type
    /// matches `T` (or if the list is empty).
    #[inline]
    pub fn into_typed<T: ?Sized + ElementTag>(self) -> Option<TypedList<T, O>> {
        if !self.element_is::<T>() {
            return None;
        }
        Some(unsafe { self.into_typed_unchecked::<T>() })
    }

    #[inline(always)]
    pub(crate) unsafe fn into_typed_unchecked<T: ?Sized + ElementTag>(self) -> TypedList<T, O> {
        let me = ManuallyDrop::new(self);
        let mut typed = TypedList {
            data: unsafe { ptr::read(&me.data) },
            _marker: PhantomData,
        };
        typed.fix_header_tag();
        typed
    }

    /// Looks up the element at `index`, returning [`ValueRef::End`] if out of bounds.
    #[inline]
    pub fn at(&self, index: usize) -> ValueRef<'_, Owned<O>> {
        self.as_ref().at(index)
    }

    /// Looks up the element at `index`.
    #[inline]
    pub fn get(&self, index: usize) -> Option<ValueRef<'_, Owned<O>>> {
        self.as_ref().get(index)
    }

    /// Looks up and downcasts the element at `index` to Rust tag type `T`.
    #[inline]
    pub fn get_as<T: ?Sized + Tag>(&self, index: usize) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().get_as::<T>(index)
    }

    /// Mutably borrows the element at `index` as a [`ValueMut`].
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<ValueMut<'_, O>> {
        if index >= self.len() {
            cold_path();
            return None;
        }
        let tag_id = self.element_tag_id();
        let slot = unsafe {
            self.data
                .as_mut_ptr()
                .add(5 + index * mutable_tag_size(tag_id))
        };
        Some(unsafe { ValueMut::from_slot(tag_id, slot) })
    }

    /// Mutably borrows and downcasts the element at `index` to Rust tag type `T`.
    #[inline]
    pub fn get_mut_as<T: ?Sized + Tag>(&mut self, index: usize) -> Option<T::Mut<'_, O>> {
        if index >= self.len() || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }
        let slot = unsafe {
            self.data
                .as_mut_ptr()
                .add(5 + index * mutable_tag_size(T::TAG_ID))
        };
        unsafe { T::read_mut::<O>(slot) }
    }

    /// Appends a value if its tag matches the list's element tag (or sets the element tag if the
    /// list is currently empty). Returns `Err(value)` without modifying the list on tag mismatch.
    #[inline]
    pub fn try_push<V: IntoNBT<O>>(&mut self, value: V) -> core::result::Result<(), V> {
        let count = self.len();
        let cur_tag = self.element_tag_id();
        if cur_tag != V::Tag::TAG_ID && count != 0 {
            cold_path();
            return Err(value);
        }
        let val = value.into_nbt();
        unsafe { raw_push_elem::<V::Tag, O>(&mut self.data, count, val) };
        Ok(())
    }

    /// Appends a value if its tag matches the list's element tag (or sets the element tag if the
    /// list is currently empty). Does nothing if the list is non-empty with a different tag.
    ///
    /// # Panics
    ///
    /// Panics if a string `value` is longer than [`MUTF8String::MAX_LEN`] bytes once MUTF-8
    /// encoded (see [`IntoNBT`]).
    #[inline]
    pub fn push<V: IntoNBT<O>>(&mut self, value: V) {
        let _ = self.try_push(value);
    }

    /// Appends a dynamically typed [`Value`], returning `Err(value)` if the list is non-empty with
    /// a different element tag.
    #[inline]
    pub fn push_value(&mut self, value: Value<O>) -> core::result::Result<(), Value<O>> {
        let count = self.len();
        let cur_tag = self.element_tag_id();
        let val_tag = value.tag_id();
        if cur_tag != val_tag && count != 0 {
            cold_path();
            return Err(value);
        }
        unsafe {
            match value {
                Value::End => raw_push_elem::<(), O>(&mut self.data, count, ()),
                Value::Byte(v) => raw_push_elem::<i8, O>(&mut self.data, count, v),
                Value::Short(v) => raw_push_elem::<i16, O>(&mut self.data, count, v),
                Value::Int(v) => raw_push_elem::<i32, O>(&mut self.data, count, v),
                Value::Long(v) => raw_push_elem::<i64, O>(&mut self.data, count, v),
                Value::Float(v) => raw_push_elem::<f32, O>(&mut self.data, count, v),
                Value::Double(v) => raw_push_elem::<f64, O>(&mut self.data, count, v),
                Value::ByteArray(v) => raw_push_elem::<[i8], O>(&mut self.data, count, v),
                Value::String(v) => raw_push_elem::<str, O>(&mut self.data, count, v),
                Value::List(v) => raw_push_elem::<List<BigEndian>, O>(&mut self.data, count, v),
                Value::Compound(v) => {
                    raw_push_elem::<Compound<BigEndian>, O>(&mut self.data, count, v)
                }
                Value::IntArray(v) => raw_push_elem::<[i32], O>(&mut self.data, count, v),
                Value::LongArray(v) => raw_push_elem::<[i64], O>(&mut self.data, count, v),
            }
        }
        Ok(())
    }

    /// Removes and returns the last element, or `None` if empty.
    #[inline]
    pub fn pop(&mut self) -> Option<Value<O>> {
        let count = self.len();
        if count == 0 {
            cold_path();
            return None;
        }
        let tag_id = self.element_tag_id();
        let elem_size = mutable_tag_size(tag_id);
        let new_bytes = self.data.len() - elem_size;
        unsafe {
            let base = self.data.as_mut_ptr();
            let val = Value::<O>::read_from_slot(tag_id, base.add(new_bytes));
            self.data.set_len(new_bytes);
            write_list_header::<O>(base, tag_id, (count - 1) as u32);
            Some(val)
        }
    }

    /// Removes and returns the last element only if its tag matches `T`.
    #[inline]
    pub fn pop_as<T: ?Sized + Tag>(&mut self) -> Option<T::Own<O>> {
        let count = self.len();
        if count == 0 || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }
        let elem_size = mutable_tag_size(T::TAG_ID);
        let new_bytes = self.data.len() - elem_size;
        unsafe {
            let base = self.data.as_mut_ptr();
            let slot = base.add(new_bytes);
            if !T::can_take_from_slot::<O>(slot) {
                cold_path();
                return None;
            }
            let val = T::take_own_from_slot::<O>(slot);
            self.data.set_len(new_bytes);
            write_list_header::<O>(base, T::TAG_ID, (count - 1) as u32);
            Some(val)
        }
    }

    /// Inserts `value` at `index` if `index <= len()` and `value`'s tag matches the list's element
    /// tag (or if the list is currently empty).
    ///
    /// # Panics
    ///
    /// Panics if a string `value` is longer than [`MUTF8String::MAX_LEN`] bytes once MUTF-8
    /// encoded (see [`IntoNBT`]).
    #[inline]
    pub fn insert<V: IntoNBT<O>>(&mut self, index: usize, value: V) {
        let count = self.len();
        if index > count {
            cold_path();
            return;
        }
        let cur_tag = self.element_tag_id();
        if cur_tag != V::Tag::TAG_ID && count != 0 {
            cold_path();
            return;
        }
        let val = value.into_nbt();
        unsafe { raw_insert_elem::<V::Tag, O>(&mut self.data, count, index, val) };
    }

    /// Removes and returns the element at `index`, shifting subsequent elements left.
    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<Value<O>> {
        let count = self.len();
        if index >= count {
            cold_path();
            return None;
        }
        let tag_id = self.element_tag_id();
        let elem_size = mutable_tag_size(tag_id);
        let pos_bytes = 5 + index * elem_size;
        let old_bytes = self.data.len();
        unsafe {
            let base = self.data.as_mut_ptr();
            let slot = base.add(pos_bytes);
            let val = Value::<O>::read_from_slot(tag_id, slot);
            ptr::copy(slot.add(elem_size), slot, old_bytes - pos_bytes - elem_size);
            self.data.set_len(old_bytes - elem_size);
            write_list_header::<O>(base, tag_id, (count - 1) as u32);
            Some(val)
        }
    }

    /// Removes and returns the element at `index` only if its tag matches `T`.
    #[inline]
    pub fn remove_as<T: ?Sized + Tag>(&mut self, index: usize) -> Option<T::Own<O>> {
        let count = self.len();
        if index >= count || self.element_tag_id() != T::TAG_ID {
            cold_path();
            return None;
        }
        let elem_size = mutable_tag_size(T::TAG_ID);
        let pos_bytes = 5 + index * elem_size;
        let old_bytes = self.data.len();
        unsafe {
            let base = self.data.as_mut_ptr();
            let slot = base.add(pos_bytes);
            if !T::can_take_from_slot::<O>(slot) {
                cold_path();
                return None;
            }
            let val = T::take_own_from_slot::<O>(slot);
            ptr::copy(slot.add(elem_size), slot, old_bytes - pos_bytes - elem_size);
            self.data.set_len(old_bytes - elem_size);
            write_list_header::<O>(base, T::TAG_ID, (count - 1) as u32);
            Some(val)
        }
    }

    /// Removes all elements while preserving the current `element_tag_id` and capacity.
    #[inline]
    pub fn clear(&mut self) {
        let count = self.len();
        if count > 0 {
            let tag_id = self.element_tag_id();
            if !tag_id.is_primitive() {
                unsafe {
                    drop_list_elements::<O>(tag_id, self.data.as_mut_ptr().add(5), count);
                }
            }
            unsafe {
                self.data.set_len(5);
                write_list_header::<O>(self.data.as_mut_ptr(), tag_id, 0);
            }
        }
    }

    /// Returns an immutable iterator yielding [`ValueRef`] elements.
    #[inline]
    pub fn iter(&self) -> ListIter<'_, Owned<O>> {
        self.as_ref().iter()
    }

    /// Returns a mutable iterator yielding [`ValueMut`] elements.
    #[inline]
    pub fn iter_mut(&mut self) -> ListIterMut<'_, O> {
        let count = self.len() as u32;
        let tag_id = self.element_tag_id();
        let data = if count == 0 {
            ptr::null_mut()
        } else {
            unsafe { self.data.as_mut_ptr().add(5) }
        };
        ListIterMut {
            tag_id,
            remaining: count,
            data,
            _marker: PhantomData,
        }
    }

    impl_inherent_write_methods!();
}

impl<O: ByteOrder> Drop for List<O> {
    fn drop(&mut self) {
        let tag_id = self.element_tag_id();
        if tag_id.is_primitive() {
            return;
        }
        let count = self.len();
        if count > 0 {
            unsafe {
                debug_assert_eq!(5 + count * SIZE_DYN, self.data.len());
                drop_list_elements::<O>(tag_id, self.data.as_mut_ptr().add(5), count);
            }
        }
    }
}

impl<O: ByteOrder> Clone for List<O> {
    fn clone(&self) -> Self {
        if self.data.is_empty() {
            return Self::new();
        }
        unsafe { clone_list_raw::<O, O>(self.data.as_ptr(), self.data.len()) }
    }
}

/// Deep-clones the non-empty list buffer (`[tag][len][elements]`) of `byte_len` bytes at `src`
/// from byte order `O` into `TARGET`.
pub(crate) unsafe fn clone_list_raw<O: ByteOrder, TARGET: ByteOrder>(
    src: *const u8,
    byte_len: usize,
) -> List<TARGET> {
    let same_order = const { same_byte_order::<O, TARGET>() };
    unsafe {
        let tag_id = *src.cast::<TagID>();
        let count = byteorder::U32::<O>::from_bytes(*src.add(1).cast()).get() as usize;
        if count == 0 {
            return List::with_tag_id(tag_id);
        }
        if tag_id.is_primitive() {
            if same_order {
                return List {
                    data: slice::from_raw_parts(src, byte_len).to_vec(),
                    _marker: PhantomData,
                };
            }
            let mut out = List {
                data: Vec::with_capacity(byte_len),
                _marker: PhantomData,
            };
            let base = out.data.as_mut_ptr();
            write_list_header::<TARGET>(base, tag_id, count as u32);
            match tag_id {
                TagID::End => {}
                TagID::Byte => {
                    ptr::copy_nonoverlapping(src.add(5), base.add(5), count);
                }
                TagID::Short => {
                    let s = slice::from_raw_parts(src.add(5).cast::<[u8; 2]>(), count);
                    let d = base.add(5).cast::<[u8; 2]>();
                    for (i, &b) in s.iter().enumerate() {
                        d.add(i).write(swap_u16::<O, TARGET>(b));
                    }
                }
                TagID::Int | TagID::Float => {
                    let s = slice::from_raw_parts(src.add(5).cast::<[u8; 4]>(), count);
                    let d = base.add(5).cast::<[u8; 4]>();
                    for (i, &b) in s.iter().enumerate() {
                        d.add(i).write(swap_u32::<O, TARGET>(b));
                    }
                }
                TagID::Long | TagID::Double => {
                    let s = slice::from_raw_parts(src.add(5).cast::<[u8; 8]>(), count);
                    let d = base.add(5).cast::<[u8; 8]>();
                    for (i, &b) in s.iter().enumerate() {
                        d.add(i).write(swap_u64::<O, TARGET>(b));
                    }
                }
                _ => {}
            }
            out.data.set_len(byte_len);
            return out;
        }
        let mut out = List {
            data: Vec::with_capacity(byte_len),
            _marker: PhantomData,
        };
        let base = out.data.as_mut_ptr();
        write_list_header::<TARGET>(base, tag_id, 0);
        out.data.set_len(5);

        macro_rules! clone_elems {
            ($src_ty:ty, $clone_expr:expr) => {{
                let mut src = src.add(5);
                let mut dst = base.add(5);
                for done in 0..count {
                    let borrowed = ptr::read_unaligned(src.cast::<ManuallyDrop<$src_ty>>());
                    let cloned = ($clone_expr)(&*borrowed);
                    slot_write(dst, cloned);
                    src = src.add(SIZE_DYN);
                    dst = dst.add(SIZE_DYN);
                    out.data.set_len(5 + (done + 1) * SIZE_DYN);
                    write_list_header::<TARGET>(base, tag_id, (done + 1) as u32);
                }
            }};
        }

        match tag_id {
            TagID::ByteArray => clone_elems!(Vec<i8>, |v: &Vec<i8>| v.clone()),
            TagID::String => clone_elems!(MUTF8String, |v: &MUTF8String| v.clone()),
            TagID::List => clone_elems!(List<O>, |v: &List<O>| {
                if v.data.is_empty() {
                    List::new()
                } else {
                    clone_list_raw::<O, TARGET>(v.data.as_ptr(), v.data.len())
                }
            }),
            TagID::Compound => clone_elems!(Compound<O>, |v: &Compound<O>| {
                if v.is_empty() {
                    Compound::new()
                } else {
                    clone_compound_raw::<O, TARGET>(v.data.as_ptr(), v.data.len())
                }
            }),
            TagID::IntArray => {
                clone_elems!(Vec<byteorder::I32<O>>, |v: &Vec<byteorder::I32<O>>| {
                    vec_copy_endian::<byteorder::I32<O>, byteorder::I32<TARGET>, 4>(
                        v.as_ptr(),
                        v.len(),
                        same_order,
                        swap_u32::<O, TARGET>,
                    )
                })
            }
            TagID::LongArray => {
                clone_elems!(Vec<byteorder::I64<O>>, |v: &Vec<byteorder::I64<O>>| {
                    vec_copy_endian::<byteorder::I64<O>, byteorder::I64<TARGET>, 8>(
                        v.as_ptr(),
                        v.len(),
                        same_order,
                        swap_u64::<O, TARGET>,
                    )
                })
            }
            _ => {}
        }
        out
    }
}

impl<'a, O: ByteOrder> IntoIterator for &'a List<O> {
    type Item = ValueRef<'a, Owned<O>>;
    type IntoIter = ListIter<'a, Owned<O>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, O: ByteOrder> IntoIterator for &'a mut List<O> {
    type Item = ValueMut<'a, O>;
    type IntoIter = ListIterMut<'a, O>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<O: ByteOrder> fmt::Debug for List<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

/// Mutable iterator over elements of a [`List`].
pub struct ListIterMut<'a, O: ByteOrder = BigEndian> {
    tag_id: TagID,
    remaining: u32,
    data: *mut u8,
    _marker: PhantomData<&'a mut O>,
}

unsafe impl<O: ByteOrder> Send for ListIterMut<'_, O> {}
unsafe impl<O: ByteOrder> Sync for ListIterMut<'_, O> {}

impl<O: ByteOrder> Default for ListIterMut<'_, O> {
    #[inline]
    fn default() -> Self {
        Self {
            tag_id: TagID::End,
            remaining: 0,
            data: ptr::null_mut(),
            _marker: PhantomData,
        }
    }
}

impl<'a, O: ByteOrder> Iterator for ListIterMut<'a, O> {
    type Item = ValueMut<'a, O>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let slot = self.data;
        self.data = unsafe { slot.add(mutable_tag_size(self.tag_id)) };
        Some(unsafe { ValueMut::from_slot(self.tag_id, slot) })
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
                            accum = f(accum, ValueMut::End);
                        }
                    }
                    $(TagID::$variant => {
                        let step = const { <$ty as ElementTag>::INLINE_SIZE };
                        for _ in 0..n {
                            let item = ValueMut::$variant(unsafe {
                                <$ty as ElementTag>::read_elem_mut::<O>(self.data)
                            });
                            self.data = unsafe { self.data.add(step) };
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

impl<O: ByteOrder> ExactSizeIterator for ListIterMut<'_, O> {}

/// An owned, homogeneous NBT list with statically known element type `T` (`i32`, `str`, `[i8]`,
/// [`Compound`], [`List`], etc.).
#[repr(transparent)]
pub struct TypedList<T: ?Sized + ElementTag, O: ByteOrder = BigEndian> {
    pub(crate) data: Vec<u8>,
    pub(crate) _marker: PhantomData<fn() -> (*const T, O)>,
}

impl<T: ?Sized + ElementTag, O: ByteOrder> Default for TypedList<T, O> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> TypedList<T, O> {
    /// Creates an empty typed list without heap allocation.
    #[inline]
    pub const fn new() -> Self {
        Self {
            data: Vec::new(),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn fix_header_tag(&mut self) {
        if !self.data.is_empty() {
            self.data[0] = T::TAG_ID as u8;
        }
    }

    #[inline(always)]
    pub(crate) fn raw_ptr(&self) -> *const u8 {
        if self.data.is_empty() {
            EMPTY_TYPED_LISTS[T::TAG_ID as usize].as_ptr()
        } else {
            self.data.as_ptr()
        }
    }

    /// Returns a lightweight `Copy` read-only view of this typed list.
    #[inline]
    pub fn as_ref(&self) -> TypedListRef<'_, T, Owned<O>> {
        TypedListRef::from_cursor(OwnedPtr {
            ptr: self.raw_ptr(),
            _marker: PhantomData,
        })
    }

    /// Returns this typed list as a dynamic [`ListRef`].
    #[inline]
    pub fn as_list_ref(&self) -> ListRef<'_, Owned<O>> {
        ListRef::from_cursor(OwnedPtr {
            ptr: self.raw_ptr(),
            _marker: PhantomData,
        })
    }

    /// Returns the number of elements in the list.
    #[inline]
    pub fn len(&self) -> usize {
        raw_list_len::<O>(&self.data)
    }

    /// Returns `true` if the list contains zero elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Looks up the typed element at `index`.
    #[inline]
    pub fn get(&self, index: usize) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().get(index)
    }

    /// Mutably borrows the typed element at `index`.
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<T::Mut<'_, O>> {
        if index >= self.len() {
            cold_path();
            return None;
        }
        let elem_size = const { T::INLINE_SIZE };
        let slot = unsafe { self.data.as_mut_ptr().add(5 + index * elem_size) };
        Some(unsafe { T::read_elem_mut::<O>(slot) })
    }

    /// Appends an element to the back of the list.
    ///
    /// # Panics
    ///
    /// Panics if a string `value` is longer than [`MUTF8String::MAX_LEN`] bytes once MUTF-8
    /// encoded (see [`IntoNBT`]).
    #[inline]
    pub fn push(&mut self, value: impl IntoNBT<O, Tag = T>) {
        let count = self.len();
        let val = value.into_nbt();
        unsafe { raw_push_elem::<T, O>(&mut self.data, count, val) };
    }

    /// Removes and returns the last element, or `None` if empty.
    #[inline]
    pub fn pop(&mut self) -> Option<T::Own<O>> {
        let count = self.len();
        if count == 0 {
            cold_path();
            return None;
        }
        let elem_size = const { T::INLINE_SIZE };
        let new_bytes = self.data.len() - elem_size;
        unsafe {
            let base = self.data.as_mut_ptr();
            let val = T::read_own_from_slot::<O>(base.add(new_bytes));
            self.data.set_len(new_bytes);
            write_list_header::<O>(base, T::TAG_ID, (count - 1) as u32);
            Some(val)
        }
    }

    /// Inserts an element at `index` (`index <= len()`), shifting subsequent elements right.
    ///
    /// # Panics
    ///
    /// Panics if a string `value` is longer than [`MUTF8String::MAX_LEN`] bytes once MUTF-8
    /// encoded (see [`IntoNBT`]).
    #[inline]
    pub fn insert(&mut self, index: usize, value: impl IntoNBT<O, Tag = T>) {
        let count = self.len();
        if index > count {
            cold_path();
            return;
        }
        let val = value.into_nbt();
        unsafe { raw_insert_elem::<T, O>(&mut self.data, count, index, val) };
    }

    /// Removes and returns the element at `index`, shifting subsequent elements left.
    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<T::Own<O>> {
        let count = self.len();
        if index >= count {
            cold_path();
            return None;
        }
        let elem_size = const { T::INLINE_SIZE };
        let pos_bytes = 5 + index * elem_size;
        let old_bytes = self.data.len();
        unsafe {
            let base = self.data.as_mut_ptr();
            let slot = base.add(pos_bytes);
            let val = T::read_own_from_slot::<O>(slot);
            ptr::copy(slot.add(elem_size), slot, old_bytes - pos_bytes - elem_size);
            self.data.set_len(old_bytes - elem_size);
            write_list_header::<O>(base, T::TAG_ID, (count - 1) as u32);
            Some(val)
        }
    }

    /// Removes all elements while retaining buffer capacity.
    #[inline]
    pub fn clear(&mut self) {
        let count = self.len();
        if count > 0 {
            if const { mem::needs_drop::<T::Own<O>>() } {
                unsafe {
                    let mut p = self.data.as_mut_ptr().add(5);
                    for _ in 0..count {
                        drop(slot_read::<T::Own<O>>(p));
                        p = p.add(SIZE_DYN);
                    }
                }
            }
            unsafe {
                self.data.set_len(5);
                write_list_header::<O>(self.data.as_mut_ptr(), T::TAG_ID, 0);
            }
        }
    }

    /// Converts this typed list into a dynamic [`List<O>`] without copying elements.
    #[inline]
    pub fn into_untyped(self) -> List<O> {
        List::from(self)
    }

    /// Returns a branchless typed iterator over the elements of this list.
    #[inline]
    pub fn iter(&self) -> TypedListIter<'_, T, Owned<O>> {
        self.as_ref().iter()
    }

    /// Returns a branchless mutable iterator over the elements of this list.
    #[inline]
    pub fn iter_mut(&mut self) -> TypedListIterMut<'_, T, O> {
        let count = self.len() as u32;
        let data = if count == 0 {
            ptr::null_mut()
        } else {
            unsafe { self.data.as_mut_ptr().add(5) }
        };
        TypedListIterMut {
            remaining: count,
            data,
            _marker: PhantomData,
        }
    }

    impl_inherent_write_methods!();
}

impl<O: ByteOrder> TypedList<i8, O> {
    /// Returns the underlying contiguous `&[i8]` slice.
    #[inline]
    pub fn as_slice(&self) -> &[i8] {
        let len = self.len();
        if len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.data.as_ptr().add(5).cast::<i8>(), len) }
        }
    }

    /// Returns the underlying contiguous `&mut [i8]` slice.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [i8] {
        let len = self.len();
        if len == 0 {
            &mut []
        } else {
            unsafe { slice::from_raw_parts_mut(self.data.as_mut_ptr().add(5).cast::<i8>(), len) }
        }
    }

    /// Appends a slice of `i8` bytes in a single `memcpy`.
    #[inline]
    pub fn extend_from_slice(&mut self, src: &[i8]) {
        if src.is_empty() {
            return;
        }
        let count = self.len();
        let new_count = count
            .checked_add(src.len())
            .filter(|&n| n <= u32::MAX as usize)
            .expect("list length too long");
        unsafe {
            if self.data.is_empty() {
                self.data.reserve(5 + src.len());
                let base = self.data.as_mut_ptr();
                write_list_header::<O>(base, TagID::Byte, new_count as u32);
                ptr::copy_nonoverlapping(src.as_ptr().cast::<u8>(), base.add(5), src.len());
                self.data.set_len(5 + src.len());
            } else {
                let old_bytes = self.data.len();
                self.data.reserve(src.len());
                let base = self.data.as_mut_ptr();
                write_list_header::<O>(base, TagID::Byte, new_count as u32);
                ptr::copy_nonoverlapping(src.as_ptr().cast::<u8>(), base.add(old_bytes), src.len());
                self.data.set_len(old_bytes + src.len());
            }
        }
    }
}

macro_rules! impl_typed_list_endian_slices {
    ($($prim:ty => ($wrapper:ident, $tag_variant:ident)),* $(,)?) => {
        $(
            impl<O: ByteOrder> TypedList<$prim, O> {
                /// Returns the contiguous 1-byte-aligned endian-encoded element slice without copying.
                #[inline]
                pub fn as_slice(&self) -> &[byteorder::$wrapper<O>] {
                    let len = self.len();
                    if len == 0 {
                        &[]
                    } else {
                        unsafe {
                            slice::from_raw_parts(
                                self.data.as_ptr().add(5).cast::<byteorder::$wrapper<O>>(),
                                len,
                            )
                        }
                    }
                }

                /// Returns a mutable contiguous 1-byte-aligned endian-encoded element slice.
                #[inline]
                pub fn as_mut_slice(&mut self) -> &mut [byteorder::$wrapper<O>] {
                    let len = self.len();
                    if len == 0 {
                        &mut []
                    } else {
                        unsafe {
                            slice::from_raw_parts_mut(
                                self.data.as_mut_ptr().add(5).cast::<byteorder::$wrapper<O>>(),
                                len,
                            )
                        }
                    }
                }

                /// Appends a slice of native elements in bulk.
                #[inline]
                pub fn extend_from_slice(&mut self, src: &[$prim]) {
                    if src.is_empty() {
                        return;
                    }
                    let elem_size = const { mem::size_of::<$prim>() };
                    let count = self.len();
                    let new_count = count
                        .checked_add(src.len())
                        .filter(|&n| n <= u32::MAX as usize)
                        .expect("list length too long");
                    let byte_len = src.len() * elem_size;
                    unsafe {
                        let dst_offset = if self.data.is_empty() {
                            self.data.reserve(5 + byte_len);
                            5
                        } else {
                            let old = self.data.len();
                            self.data.reserve(byte_len);
                            old
                        };
                        let base = self.data.as_mut_ptr();
                        write_list_header::<O>(base, TagID::$tag_variant, new_count as u32);
                        let dst = base.add(dst_offset);
                        if crate::same_byte_order::<O, zerocopy::NativeEndian>() {
                            ptr::copy_nonoverlapping(src.as_ptr().cast::<u8>(), dst, byte_len);
                        } else {
                            let mut p = dst.cast::<byteorder::$wrapper<O>>();
                            for &v in src {
                                p.write(byteorder::$wrapper::<O>::new(v));
                                p = p.add(1);
                            }
                        }
                        self.data.set_len(dst_offset + byte_len);
                    }
                }
            }
        )*
    };
}

impl_typed_list_endian_slices!(
    i16 => (I16, Short),
    i32 => (I32, Int),
    i64 => (I64, Long),
    f32 => (F32, Float),
    f64 => (F64, Double),
);

impl<T: ?Sized + ElementTag, V: IntoNBT<O, Tag = T>, O: ByteOrder> Extend<V> for TypedList<T, O> {
    fn extend<I: IntoIterator<Item = V>>(&mut self, iter: I) {
        for item in iter {
            self.push(item);
        }
    }
}

impl<T: ?Sized + ElementTag, V: IntoNBT<O, Tag = T>, O: ByteOrder> FromIterator<V>
    for TypedList<T, O>
{
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let mut list = Self::new();
        list.extend(iter);
        list
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> From<TypedList<T, O>> for List<O> {
    #[inline]
    fn from(tl: TypedList<T, O>) -> Self {
        let me = ManuallyDrop::new(tl);
        let data = unsafe { ptr::read(&me.data) };
        if data.is_empty() && const { T::TAG_ID as u8 != TagID::End as u8 } {
            Self {
                data: vec![T::TAG_ID as u8, 0, 0, 0, 0],
                _marker: PhantomData,
            }
        } else {
            Self {
                data,
                _marker: PhantomData,
            }
        }
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> Drop for TypedList<T, O> {
    fn drop(&mut self) {
        if const { !mem::needs_drop::<T::Own<O>>() } {
            return;
        }
        let count = self.len();
        if count > 0 {
            unsafe {
                debug_assert_eq!(5 + count * SIZE_DYN, self.data.len());
                let mut p = self.data.as_mut_ptr().add(5);
                for _ in 0..count {
                    drop(slot_read::<T::Own<O>>(p));
                    p = p.add(SIZE_DYN);
                }
            }
        }
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> Clone for TypedList<T, O> {
    fn clone(&self) -> Self {
        if self.data.is_empty() {
            return Self::new();
        }
        // Borrow as `List` without copying to reuse its panic-safe `Clone` logic.
        let tmp = ManuallyDrop::new(List::<O> {
            data: unsafe { ptr::read(&self.data) },
            _marker: PhantomData,
        });
        let cloned: List<O> = (*tmp).clone();
        unsafe { cloned.into_typed_unchecked::<T>() }
    }
}

impl<'a, T: ?Sized + ElementTag, O: ByteOrder> IntoIterator for &'a TypedList<T, O> {
    type Item = T::Ref<'a, Owned<O>>;
    type IntoIter = TypedListIter<'a, T, Owned<O>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: ?Sized + ElementTag, O: ByteOrder> IntoIterator for &'a mut TypedList<T, O> {
    type Item = T::Mut<'a, O>;
    type IntoIter = TypedListIterMut<'a, T, O>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> fmt::Debug for TypedList<T, O>
where
    for<'b> T::Ref<'b, Owned<O>>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

/// Mutable iterator over elements of a [`TypedList`].
pub struct TypedListIterMut<'a, T: ?Sized + ElementTag, O: ByteOrder = BigEndian> {
    remaining: u32,
    data: *mut u8,
    #[allow(clippy::type_complexity)]
    _marker: PhantomData<(&'a mut O, fn() -> *const T)>,
}

unsafe impl<T: ?Sized + ElementTag, O: ByteOrder> Send for TypedListIterMut<'_, T, O> {}
unsafe impl<T: ?Sized + ElementTag, O: ByteOrder> Sync for TypedListIterMut<'_, T, O> {}

impl<T: ?Sized + ElementTag, O: ByteOrder> Default for TypedListIterMut<'_, T, O> {
    #[inline]
    fn default() -> Self {
        Self {
            remaining: 0,
            data: ptr::null_mut(),
            _marker: PhantomData,
        }
    }
}

impl<'a, T: ?Sized + ElementTag, O: ByteOrder> Iterator for TypedListIterMut<'a, T, O> {
    type Item = T::Mut<'a, O>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let slot = self.data;
        let step = const { T::INLINE_SIZE };
        self.data = unsafe { slot.add(step) };
        Some(unsafe { T::read_elem_mut::<O>(slot) })
    }

    #[inline]
    fn fold<B, F>(mut self, mut accum: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        let step = const { T::INLINE_SIZE };
        for _ in 0..self.remaining {
            let item = unsafe { T::read_elem_mut::<O>(self.data) };
            self.data = unsafe { self.data.add(step) };
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

impl<T: ?Sized + ElementTag, O: ByteOrder> ExactSizeIterator for TypedListIterMut<'_, T, O> {}

impl<O: ByteOrder> PartialEq for List<O> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<O: ByteOrder, S: crate::Source> PartialEq<ListRef<'_, S>> for List<O> {
    #[inline]
    fn eq(&self, other: &ListRef<'_, S>) -> bool {
        self.as_ref() == *other
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> PartialEq for TypedList<T, O> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder, S: crate::Source> PartialEq<TypedListRef<'_, T, S>>
    for TypedList<T, O>
{
    #[inline]
    fn eq(&self, other: &TypedListRef<'_, T, S>) -> bool {
        self.as_ref() == *other
    }
}
