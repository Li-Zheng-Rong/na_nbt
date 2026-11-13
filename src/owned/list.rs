use std::{
    fmt,
    io::Write,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ptr,
};

use zerocopy::byteorder;

use crate::{
    ByteOrder, CompoundOwn, EMPTY_LIST, EMPTY_TYPED_LISTS, ElementTag, IntoNBT, ListIter, ListRef,
    MUTF8String, Owned, Result, Tag, TagID, TypedListIter, TypedListRef, ValueMut, ValueOwn,
    ValueRef, Writable, cold_path,
    owned::{
        OwnedPtr,
        slot::{SIZE_DYN, drop_list_elements, mutable_tag_size, slot_read, slot_write},
        write_list_sink,
    },
    tag,
    write::{ByteSink, IoSink, sink_to_vec},
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

/// An owned, flat-buffer untyped NBT list.
#[repr(transparent)]
pub struct ListOwn<O: ByteOrder> {
    pub(crate) data: Vec<u8>,
    pub(crate) _marker: PhantomData<fn() -> O>,
}

impl<O: ByteOrder> Default for ListOwn<O> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<O: ByteOrder> ListOwn<O> {
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
    pub fn with_element<T: ElementTag>() -> Self {
        if const { T::TAG_ID as u8 == TagID::End as u8 } {
            Self::new()
        } else {
            Self {
                data: vec![T::TAG_ID as u8, 0, 0, 0, 0],
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

    #[inline]
    pub fn as_ref(&self) -> ListRef<'_, Owned<O>> {
        ListRef::from_cursor(OwnedPtr {
            ptr: self.raw_ptr(),
            _marker: PhantomData,
        })
    }

    #[inline]
    pub fn element_tag_id(&self) -> TagID {
        if self.data.is_empty() {
            TagID::End
        } else {
            unsafe { *self.data.as_ptr().cast() }
        }
    }

    #[inline]
    pub fn element_is<T: ElementTag>(&self) -> bool {
        self.element_tag_id() == T::TAG_ID || self.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        if self.data.is_empty() {
            0
        } else {
            byteorder::U32::<O>::from_bytes(unsafe { *self.data.as_ptr().add(1).cast() }).get()
                as usize
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<ValueRef<'_, Owned<O>>> {
        self.as_ref().get(index)
    }

    #[inline]
    pub fn get_as<T: Tag>(&self, index: usize) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().get_as::<T>(index)
    }

    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<ValueMut<'_, O>> {
        if index >= self.len() {
            cold_path();
            return None;
        }
        let tag_id = self.element_tag_id();
        let slot = unsafe { self.data.as_mut_ptr().add(5 + index * mutable_tag_size(tag_id)) };
        Some(unsafe { ValueMut::from_slot(tag_id, slot) })
    }

    #[inline]
    pub fn get_mut_as<T: Tag>(&mut self, index: usize) -> Option<T::Mut<'_, O>> {
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
    /// list is currently empty with `TagID::End`).
    #[inline]
    pub fn push<V: IntoNBT<O>>(&mut self, value: V) {
        let cur_tag = self.element_tag_id();
        if cur_tag != V::Tag::TAG_ID {
            cold_path();
            if !(cur_tag == TagID::End && self.is_empty()) {
                return;
            }
        }
        let val = value.into_nbt();
        let elem_size = const { <V::Tag as ElementTag>::INLINE_SIZE };
        let count = self.len();
        assert!(count < u32::MAX as usize, "list length too long");

        unsafe {
            if self.data.is_empty() {
                self.data.reserve(5 + elem_size);
                let base = self.data.as_mut_ptr();
                write_list_header::<O>(base, V::Tag::TAG_ID, 1);
                <V::Tag as ElementTag>::write_own_to_slot::<O>(base.add(5), val);
                self.data.set_len(5 + elem_size);
            } else {
                let old_bytes = self.data.len();
                self.data.reserve(elem_size);
                let base = self.data.as_mut_ptr();
                write_list_header::<O>(base, V::Tag::TAG_ID, (count + 1) as u32);
                <V::Tag as ElementTag>::write_own_to_slot::<O>(base.add(old_bytes), val);
                self.data.set_len(old_bytes + elem_size);
            }
        }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<ValueOwn<O>> {
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
            let val = ValueOwn::<O>::read_from_slot(tag_id, base.add(new_bytes));
            self.data.set_len(new_bytes);
            write_list_header::<O>(base, tag_id, (count - 1) as u32);
            Some(val)
        }
    }

    #[inline]
    pub fn pop_as<T: Tag>(&mut self) -> Option<T::Own<O>> {
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

    #[inline]
    pub fn insert<V: IntoNBT<O>>(&mut self, index: usize, value: V) {
        let count = self.len();
        if index > count {
            cold_path();
            return;
        }
        let cur_tag = self.element_tag_id();
        if cur_tag != V::Tag::TAG_ID {
            cold_path();
            if !(cur_tag == TagID::End && count == 0) {
                return;
            }
        }
        assert!(count < u32::MAX as usize, "list length too long");
        let val = value.into_nbt();
        let elem_size = const { <V::Tag as ElementTag>::INLINE_SIZE };

        unsafe {
            if self.data.is_empty() {
                self.data.reserve(5 + elem_size);
                let base = self.data.as_mut_ptr();
                write_list_header::<O>(base, V::Tag::TAG_ID, 1);
                <V::Tag as ElementTag>::write_own_to_slot::<O>(base.add(5), val);
                self.data.set_len(5 + elem_size);
            } else {
                let old_bytes = self.data.len();
                let pos_bytes = 5 + index * elem_size;
                self.data.reserve(elem_size);
                let base = self.data.as_mut_ptr();
                let slot = base.add(pos_bytes);
                ptr::copy(slot, slot.add(elem_size), old_bytes - pos_bytes);
                <V::Tag as ElementTag>::write_own_to_slot::<O>(slot, val);
                write_list_header::<O>(base, V::Tag::TAG_ID, (count + 1) as u32);
                self.data.set_len(old_bytes + elem_size);
            }
        }
    }

    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<ValueOwn<O>> {
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
            let val = ValueOwn::<O>::read_from_slot(tag_id, slot);
            ptr::copy(slot.add(elem_size), slot, old_bytes - pos_bytes - elem_size);
            self.data.set_len(old_bytes - elem_size);
            write_list_header::<O>(base, tag_id, (count - 1) as u32);
            Some(val)
        }
    }

    #[inline]
    pub fn remove_as<T: Tag>(&mut self, index: usize) -> Option<T::Own<O>> {
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

    #[inline]
    pub fn typed<T: ElementTag>(&self) -> Option<TypedListRef<'_, Owned<O>, T>> {
        self.as_ref().typed::<T>()
    }

    #[inline]
    pub fn into_typed<T: ElementTag>(self) -> Option<TypedListOwn<O, T>> {
        if !self.element_is::<T>() {
            return None;
        }
        Some(unsafe { self.into_typed_unchecked::<T>() })
    }

    #[inline(always)]
    pub(crate) unsafe fn into_typed_unchecked<T: ElementTag>(self) -> TypedListOwn<O, T> {
        let me = ManuallyDrop::new(self);
        let mut typed = TypedListOwn {
            data: unsafe { ptr::read(&me.data) },
            _marker: PhantomData,
        };
        typed.fix_header_tag();
        typed
    }

    #[inline]
    pub fn iter(&self) -> ListIter<'_, Owned<O>> {
        self.as_ref().iter()
    }

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
}

impl<O: ByteOrder> Drop for ListOwn<O> {
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

impl<O: ByteOrder> Clone for ListOwn<O> {
    fn clone(&self) -> Self {
        if self.data.is_empty() {
            return Self::new();
        }
        let tag_id = self.element_tag_id();
        if tag_id.is_primitive() || self.is_empty() {
            return Self {
                data: self.data.clone(),
                _marker: PhantomData,
            };
        }
        let count = self.len();
        let mut out = ListOwn {
            data: Vec::with_capacity(self.data.len()),
            _marker: PhantomData,
        };
        unsafe {
            let base = out.data.as_mut_ptr();
            write_list_header::<O>(base, tag_id, 0);
            out.data.set_len(5);

            macro_rules! clone_elems {
                ($ty:ty) => {{
                    let mut src = self.data.as_ptr().add(5);
                    let mut dst = base.add(5);
                    for done in 0..count {
                        let borrowed = ptr::read_unaligned(src.cast::<ManuallyDrop<$ty>>());
                        let cloned: $ty = (&*borrowed).clone();
                        slot_write(dst, cloned);
                        src = src.add(SIZE_DYN);
                        dst = dst.add(SIZE_DYN);
                        out.data.set_len(5 + (done + 1) * SIZE_DYN);
                        write_list_header::<O>(base, tag_id, (done + 1) as u32);
                    }
                }};
            }

            match tag_id {
                TagID::ByteArray => clone_elems!(Vec<i8>),
                TagID::String => clone_elems!(MUTF8String),
                TagID::List => clone_elems!(ListOwn<O>),
                TagID::Compound => clone_elems!(CompoundOwn<O>),
                TagID::IntArray => clone_elems!(Vec<byteorder::I32<O>>),
                TagID::LongArray => clone_elems!(Vec<byteorder::I64<O>>),
                _ => {}
            }
        }
        out
    }
}

impl<'a, O: ByteOrder> IntoIterator for &'a ListOwn<O> {
    type Item = ValueRef<'a, Owned<O>>;
    type IntoIter = ListIter<'a, Owned<O>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, O: ByteOrder> IntoIterator for &'a mut ListOwn<O> {
    type Item = ValueMut<'a, O>;
    type IntoIter = ListIterMut<'a, O>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<O: ByteOrder> fmt::Debug for ListOwn<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

impl<O: ByteOrder> Writable for ListOwn<O> {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        sink_to_vec(3 + self.data.len().max(5), |out| unsafe {
            out.write_bytes(&[TagID::List as u8, 0, 0])?;
            write_list_sink::<O, TARGET, _>(self.raw_ptr(), out)
        })
    }

    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        let mut sink = IoSink(writer);
        unsafe {
            sink.write_bytes(&[TagID::List as u8, 0, 0])?;
            write_list_sink::<O, TARGET, _>(self.raw_ptr(), &mut sink)
        }
    }
}

/// Mutable iterator over elements of a [`ListOwn`].
pub struct ListIterMut<'a, O: ByteOrder> {
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
            ($($t:ident),* $(,)?) => {
                match self.tag_id {
                    TagID::End => {
                        for _ in 0..n {
                            accum = f(accum, ValueMut::End);
                        }
                    }
                    $(TagID::$t => {
                        let step = const { <tag::$t as ElementTag>::INLINE_SIZE };
                        for _ in 0..n {
                            let item = ValueMut::$t(unsafe {
                                <tag::$t as ElementTag>::read_elem_mut::<O>(self.data)
                            });
                            self.data = unsafe { self.data.add(step) };
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

impl<O: ByteOrder> ExactSizeIterator for ListIterMut<'_, O> {}

/// An owned, homogeneous NBT list with statically known element tag `T`.
#[repr(transparent)]
pub struct TypedListOwn<O: ByteOrder, T: ElementTag> {
    pub(crate) data: Vec<u8>,
    pub(crate) _marker: PhantomData<fn() -> (O, T)>,
}

impl<O: ByteOrder, T: ElementTag> Default for TypedListOwn<O, T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<O: ByteOrder, T: ElementTag> TypedListOwn<O, T> {
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

    #[inline]
    pub fn as_ref(&self) -> TypedListRef<'_, Owned<O>, T> {
        TypedListRef::from_cursor(OwnedPtr {
            ptr: self.raw_ptr(),
            _marker: PhantomData,
        })
    }

    #[inline]
    pub fn len(&self) -> usize {
        if self.data.is_empty() {
            0
        } else {
            byteorder::U32::<O>::from_bytes(unsafe { *self.data.as_ptr().add(1).cast() }).get()
                as usize
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<T::Ref<'_, Owned<O>>> {
        self.as_ref().get(index)
    }

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

    #[inline]
    pub fn push(&mut self, value: impl IntoNBT<O, Tag = T>) {
        let count = self.len();
        assert!(count < u32::MAX as usize, "list length too long");
        let val = value.into_nbt();
        let elem_size = const { T::INLINE_SIZE };
        unsafe {
            if self.data.is_empty() {
                self.data.reserve(5 + elem_size);
                let base = self.data.as_mut_ptr();
                write_list_header::<O>(base, T::TAG_ID, 1);
                T::write_own_to_slot::<O>(base.add(5), val);
                self.data.set_len(5 + elem_size);
            } else {
                let old_bytes = self.data.len();
                self.data.reserve(elem_size);
                let base = self.data.as_mut_ptr();
                write_list_header::<O>(base, T::TAG_ID, (count + 1) as u32);
                T::write_own_to_slot::<O>(base.add(old_bytes), val);
                self.data.set_len(old_bytes + elem_size);
            }
        }
    }

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

    #[inline]
    pub fn insert(&mut self, index: usize, value: impl IntoNBT<O, Tag = T>) {
        let count = self.len();
        if index > count {
            cold_path();
            return;
        }
        assert!(count < u32::MAX as usize, "list length too long");
        let val = value.into_nbt();
        let elem_size = const { T::INLINE_SIZE };
        unsafe {
            if self.data.is_empty() {
                self.data.reserve(5 + elem_size);
                let base = self.data.as_mut_ptr();
                write_list_header::<O>(base, T::TAG_ID, 1);
                T::write_own_to_slot::<O>(base.add(5), val);
                self.data.set_len(5 + elem_size);
            } else {
                let old_bytes = self.data.len();
                let pos_bytes = 5 + index * elem_size;
                self.data.reserve(elem_size);
                let base = self.data.as_mut_ptr();
                let slot = base.add(pos_bytes);
                ptr::copy(slot, slot.add(elem_size), old_bytes - pos_bytes);
                T::write_own_to_slot::<O>(slot, val);
                write_list_header::<O>(base, T::TAG_ID, (count + 1) as u32);
                self.data.set_len(old_bytes + elem_size);
            }
        }
    }

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

    #[inline]
    pub fn into_untyped(self) -> ListOwn<O> {
        ListOwn::from(self)
    }

    #[inline]
    pub fn iter(&self) -> TypedListIter<'_, Owned<O>, T> {
        self.as_ref().iter()
    }

    #[inline]
    pub fn iter_mut(&mut self) -> TypedListIterMut<'_, O, T> {
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
}

impl<O: ByteOrder, T: ElementTag> From<TypedListOwn<O, T>> for ListOwn<O> {
    #[inline]
    fn from(tl: TypedListOwn<O, T>) -> Self {
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

impl<O: ByteOrder, T: ElementTag> Drop for TypedListOwn<O, T> {
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

impl<O: ByteOrder, T: ElementTag> Clone for TypedListOwn<O, T> {
    fn clone(&self) -> Self {
        if self.data.is_empty() {
            return Self::new();
        }
        // Borrow as ListOwn without copying to reuse its panic-safe Clone logic
        let tmp = ManuallyDrop::new(ListOwn::<O> {
            data: unsafe { ptr::read(&self.data) },
            _marker: PhantomData,
        });
        let cloned: ListOwn<O> = (*tmp).clone();
        unsafe { cloned.into_typed_unchecked::<T>() }
    }
}

impl<'a, O: ByteOrder, T: ElementTag> IntoIterator for &'a TypedListOwn<O, T> {
    type Item = T::Ref<'a, Owned<O>>;
    type IntoIter = TypedListIter<'a, Owned<O>, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, O: ByteOrder, T: ElementTag> IntoIterator for &'a mut TypedListOwn<O, T> {
    type Item = T::Mut<'a, O>;
    type IntoIter = TypedListIterMut<'a, O, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<O: ByteOrder, T: ElementTag> fmt::Debug for TypedListOwn<O, T>
where
    for<'b> T::Ref<'b, Owned<O>>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_ref(), f)
    }
}

impl<O: ByteOrder, T: ElementTag> Writable for TypedListOwn<O, T> {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        sink_to_vec(3 + self.data.len().max(5), |out| unsafe {
            out.write_bytes(&[TagID::List as u8, 0, 0])?;
            write_list_sink::<O, TARGET, _>(self.raw_ptr(), out)
        })
    }

    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        let mut sink = IoSink(writer);
        unsafe {
            sink.write_bytes(&[TagID::List as u8, 0, 0])?;
            write_list_sink::<O, TARGET, _>(self.raw_ptr(), &mut sink)
        }
    }
}

/// Mutable iterator over elements of a [`TypedListOwn`].
pub struct TypedListIterMut<'a, O: ByteOrder, T: ElementTag> {
    remaining: u32,
    data: *mut u8,
    _marker: PhantomData<&'a mut (O, T)>,
}

unsafe impl<O: ByteOrder, T: ElementTag> Send for TypedListIterMut<'_, O, T> {}
unsafe impl<O: ByteOrder, T: ElementTag> Sync for TypedListIterMut<'_, O, T> {}

impl<O: ByteOrder, T: ElementTag> Default for TypedListIterMut<'_, O, T> {
    #[inline]
    fn default() -> Self {
        Self {
            remaining: 0,
            data: ptr::null_mut(),
            _marker: PhantomData,
        }
    }
}

impl<'a, O: ByteOrder, T: ElementTag> Iterator for TypedListIterMut<'a, O, T> {
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

impl<O: ByteOrder, T: ElementTag> ExactSizeIterator for TypedListIterMut<'_, O, T> {}
