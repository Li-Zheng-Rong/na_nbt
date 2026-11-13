use std::{
    fmt,
    hint::assert_unchecked,
    io::Write,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ops::{Deref, DerefMut},
    ptr, slice,
};

use zerocopy::{byteorder, NativeEndian};

use crate::{
    same_byte_order, ByteOrder, CompoundOwn, ElementTag, ListOwn, MUTF8Str, MUTF8String, Result,
    TagID, TypedListOwn, Writable, EMPTY_COMPOUND, EMPTY_LIST,
};

pub(crate) const SIZE_DYN: usize = mem::size_of::<Vec<u8>>();

#[inline(always)]
pub(crate) const fn mutable_tag_size(tag_id: TagID) -> usize {
    const TAG_SIZES: [usize; 13] = [
        0, 1, 2, 4, 8, 4, 8, SIZE_DYN, SIZE_DYN, SIZE_DYN, SIZE_DYN, SIZE_DYN, SIZE_DYN,
    ];
    let tag_id = tag_id as usize;
    unsafe { assert_unchecked(tag_id < 13) };
    TAG_SIZES[tag_id]
}

mod private {
    pub trait Sealed {}
}

/// Sealed trait for heap-backed NBT container/array types stored in unaligned 24-byte slots.
pub trait SlotValue: private::Sealed + Sized {
    #[doc(hidden)]
    fn empty_placeholder() -> Self;
}

impl<T> private::Sealed for Vec<T> {}
impl<T> SlotValue for Vec<T> {
    #[inline(always)]
    fn empty_placeholder() -> Self {
        Vec::new()
    }
}

impl private::Sealed for MUTF8String {}
impl SlotValue for MUTF8String {
    #[inline(always)]
    fn empty_placeholder() -> Self {
        MUTF8String::new()
    }
}

impl<O: ByteOrder> private::Sealed for CompoundOwn<O> {}
impl<O: ByteOrder> SlotValue for CompoundOwn<O> {
    #[inline(always)]
    fn empty_placeholder() -> Self {
        CompoundOwn::new()
    }
}

impl<O: ByteOrder> private::Sealed for ListOwn<O> {}
impl<O: ByteOrder> SlotValue for ListOwn<O> {
    #[inline(always)]
    fn empty_placeholder() -> Self {
        ListOwn::new()
    }
}

impl<O: ByteOrder, T: ElementTag> private::Sealed for TypedListOwn<O, T> {}
impl<O: ByteOrder, T: ElementTag> SlotValue for TypedListOwn<O, T> {
    #[inline(always)]
    fn empty_placeholder() -> Self {
        TypedListOwn::new()
    }
}

#[inline(always)]
pub(crate) unsafe fn slot_write<T>(dst: *mut u8, val: T) {
    debug_assert_eq!(mem::size_of::<T>(), SIZE_DYN);
    unsafe {
        ptr::write_unaligned(dst.cast::<ManuallyDrop<T>>(), ManuallyDrop::new(val));
    }
}

#[inline(always)]
pub(crate) unsafe fn slot_read<T>(src: *const u8) -> T {
    debug_assert_eq!(mem::size_of::<T>(), SIZE_DYN);
    unsafe { ManuallyDrop::into_inner(ptr::read_unaligned(src.cast::<ManuallyDrop<T>>())) }
}

#[inline(always)]
pub(crate) unsafe fn slot_vec_parts<T>(slot: *const u8) -> (*const T, usize) {
    let v = unsafe { ptr::read_unaligned(slot.cast::<ManuallyDrop<Vec<T>>>()) };
    (v.as_ptr(), v.len())
}

#[inline(always)]
pub(crate) unsafe fn slot_borrow_slice<'a, T>(slot: *const u8) -> &'a [T] {
    let (ptr, len) = unsafe { slot_vec_parts::<T>(slot) };
    unsafe { slice::from_raw_parts(ptr, len) }
}

#[inline(always)]
pub(crate) unsafe fn slot_borrow_str<'a>(slot: *const u8) -> &'a MUTF8Str {
    unsafe { MUTF8Str::from_mutf8_unchecked(slot_borrow_slice::<u8>(slot)) }
}

#[inline(always)]
pub(crate) unsafe fn slot_compound_ptr(slot: *const u8) -> *const u8 {
    let (ptr, len) = unsafe { slot_vec_parts::<u8>(slot) };
    if len == 0 {
        EMPTY_COMPOUND.as_ptr()
    } else {
        ptr
    }
}

#[inline(always)]
pub(crate) unsafe fn slot_list_ptr(slot: *const u8) -> *const u8 {
    let (ptr, len) = unsafe { slot_vec_parts::<u8>(slot) };
    if len == 0 {
        EMPTY_LIST.as_ptr()
    } else {
        ptr
    }
}

#[inline]
pub(crate) unsafe fn drop_compound_entries<O: ByteOrder>(mut ptr: *mut u8, end: *mut u8) {
    unsafe {
        while ptr < end {
            let tag_id = *ptr.cast::<TagID>();
            ptr = ptr.add(1);
            if tag_id == TagID::End {
                debug_assert_eq!(ptr, end);
                return;
            }
            let name_len = byteorder::U16::<O>::from_bytes(*ptr.cast()).get() as usize;
            ptr = ptr.add(2 + name_len);
            match tag_id {
                TagID::ByteArray => drop(slot_read::<Vec<i8>>(ptr)),
                TagID::String => drop(slot_read::<MUTF8String>(ptr)),
                TagID::List => drop(slot_read::<ListOwn<O>>(ptr)),
                TagID::Compound => drop(slot_read::<CompoundOwn<O>>(ptr)),
                TagID::IntArray => drop(slot_read::<Vec<byteorder::I32<O>>>(ptr)),
                TagID::LongArray => drop(slot_read::<Vec<byteorder::I64<O>>>(ptr)),
                _ => {}
            }
            ptr = ptr.add(mutable_tag_size(tag_id));
        }
    }
}

#[inline]
pub(crate) unsafe fn drop_list_elements<O: ByteOrder>(tag_id: TagID, ptr: *mut u8, count: usize) {
    unsafe fn drop_slots<T>(mut ptr: *mut u8, count: usize) {
        for _ in 0..count {
            unsafe {
                drop(slot_read::<T>(ptr));
                ptr = ptr.add(SIZE_DYN);
            }
        }
    }

    unsafe {
        match tag_id {
            TagID::ByteArray => drop_slots::<Vec<i8>>(ptr, count),
            TagID::String => drop_slots::<MUTF8String>(ptr, count),
            TagID::List => drop_slots::<ListOwn<O>>(ptr, count),
            TagID::Compound => drop_slots::<CompoundOwn<O>>(ptr, count),
            TagID::IntArray => drop_slots::<Vec<byteorder::I32<O>>>(ptr, count),
            TagID::LongArray => drop_slots::<Vec<byteorder::I64<O>>>(ptr, count),
            _ => {}
        }
    }
}

/// RAII mutable borrow guard for a dynamic NBT value (`CompoundOwn`, `ListOwn`, `Vec`, `MUTF8String`)
/// stored in an unaligned 24-byte slot.
///
/// Dereferences directly to `&mut T` so all standard methods on `T` are available without indirection.
pub struct Mut<'a, T: SlotValue> {
    value: ManuallyDrop<T>,
    slot: *mut u8,
    _marker: PhantomData<&'a mut T>,
}

unsafe impl<T: SlotValue + Send> Send for Mut<'_, T> {}
unsafe impl<T: SlotValue + Sync> Sync for Mut<'_, T> {}

impl<'a, T: SlotValue> Mut<'a, T> {
    #[inline(always)]
    pub(crate) unsafe fn from_slot(slot: *mut u8) -> Self {
        debug_assert_eq!(mem::size_of::<T>(), SIZE_DYN);
        unsafe {
            let value = ptr::read_unaligned(slot.cast::<ManuallyDrop<T>>());
            ptr::write_unaligned(
                slot.cast::<ManuallyDrop<T>>(),
                ManuallyDrop::new(T::empty_placeholder()),
            );
            Self {
                value,
                slot,
                _marker: PhantomData,
            }
        }
    }

    #[inline(always)]
    pub(crate) fn from_mut(target: &'a mut T) -> Self {
        unsafe { Self::from_slot((target as *mut T).cast::<u8>()) }
    }
}

impl<'a, O: ByteOrder> Mut<'a, ListOwn<O>> {
    /// Attempts to convert this mutable list guard into a [`TypedListOwn<O, T>`] guard.
    ///
    /// Succeeds if `self.element_is::<T>()` (either matching element tag or empty list).
    #[inline]
    pub fn into_typed<T: ElementTag>(
        self,
    ) -> core::result::Result<Mut<'a, TypedListOwn<O, T>>, Self> {
        if !self.element_is::<T>() {
            return Err(self);
        }
        let mut me = ManuallyDrop::new(self);
        let slot = me.slot;
        let list = unsafe { ManuallyDrop::take(&mut me.value) };
        let typed = unsafe { list.into_typed_unchecked::<T>() };
        Ok(Mut {
            value: ManuallyDrop::new(typed),
            slot,
            _marker: PhantomData,
        })
    }
}

impl<T: SlotValue> Deref for Mut<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: SlotValue> DerefMut for Mut<'_, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: SlotValue> Drop for Mut<'_, T> {
    #[inline(always)]
    fn drop(&mut self) {
        unsafe {
            let v = ManuallyDrop::take(&mut self.value);
            ptr::write_unaligned(self.slot.cast::<ManuallyDrop<T>>(), ManuallyDrop::new(v));
        }
    }
}

impl<T: SlotValue + fmt::Debug> fmt::Debug for Mut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: SlotValue + fmt::Display> fmt::Display for Mut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: SlotValue + PartialEq<U>, U> PartialEq<U> for Mut<'_, T> {
    #[inline]
    fn eq(&self, other: &U) -> bool {
        (**self).eq(other)
    }
}

impl<T: SlotValue + Writable> Writable for Mut<'_, T> {
    #[inline]
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        (**self).write_to_vec::<TARGET>()
    }

    #[inline]
    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        (**self).write_to_writer::<TARGET>(writer)
    }
}

/// Sealed trait for multi-byte NBT scalars (`i16`, `i32`, `i64`, `f32`, `f64`).
pub trait Scalar: private::Sealed + Copy + Send + Sync + 'static {
    #[doc(hidden)]
    unsafe fn load_raw(slot: *const u8, swap: bool) -> Self;
    #[doc(hidden)]
    unsafe fn store_raw(self, slot: *mut u8, swap: bool);
}

macro_rules! impl_scalar {
    ($ty:ty, $uint:ty, $from_bits:expr, $to_bits:expr) => {
        impl private::Sealed for $ty {}
        impl Scalar for $ty {
            #[inline(always)]
            unsafe fn load_raw(slot: *const u8, swap: bool) -> Self {
                let raw = unsafe { ptr::read_unaligned(slot.cast::<$uint>()) };
                let bits = if swap { raw.swap_bytes() } else { raw };
                ($from_bits)(bits)
            }

            #[inline(always)]
            unsafe fn store_raw(self, slot: *mut u8, swap: bool) {
                let raw: $uint = ($to_bits)(self);
                let out = if swap { raw.swap_bytes() } else { raw };
                unsafe { ptr::write_unaligned(slot.cast::<$uint>(), out) };
            }
        }
    };
}

impl_scalar!(i16, u16, |b| b as i16, |v| v as u16);
impl_scalar!(i32, u32, |b| b as i32, |v| v as u32);
impl_scalar!(i64, u64, |b| b as i64, |v| v as u64);
impl_scalar!(f32, u32, f32::from_bits, f32::to_bits);
impl_scalar!(f64, u64, f64::from_bits, f64::to_bits);

/// RAII mutable borrow guard for a multi-byte NBT scalar (`i16`, `i32`, `i64`, `f32`, `f64`),
/// transparently handling unaligned storage and byte-order conversion.
pub struct ScalarMut<'a, T: Scalar> {
    value: T,
    slot: *mut u8,
    swap: bool,
    _marker: PhantomData<&'a mut T>,
}

unsafe impl<T: Scalar> Send for ScalarMut<'_, T> {}
unsafe impl<T: Scalar> Sync for ScalarMut<'_, T> {}

impl<'a, T: Scalar> ScalarMut<'a, T> {
    #[inline(always)]
    pub(crate) unsafe fn from_endian<O: ByteOrder>(slot: *mut u8) -> Self {
        let swap = const { !same_byte_order::<O, NativeEndian>() };
        Self {
            value: unsafe { T::load_raw(slot, swap) },
            slot,
            swap,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn from_native(target: &'a mut T) -> Self {
        Self {
            value: *target,
            slot: (target as *mut T).cast::<u8>(),
            swap: false,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn get(&self) -> T {
        self.value
    }

    #[inline(always)]
    pub fn set(&mut self, value: T) {
        self.value = value;
    }
}

impl<T: Scalar> Deref for ScalarMut<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: Scalar> DerefMut for ScalarMut<'_, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: Scalar> Drop for ScalarMut<'_, T> {
    #[inline(always)]
    fn drop(&mut self) {
        unsafe { self.value.store_raw(self.slot, self.swap) };
    }
}

impl<T: Scalar + fmt::Debug> fmt::Debug for ScalarMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.value, f)
    }
}

impl<T: Scalar + fmt::Display> fmt::Display for ScalarMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.value, f)
    }
}

impl<T: Scalar + PartialEq<U>, U> PartialEq<U> for ScalarMut<'_, T> {
    #[inline]
    fn eq(&self, other: &U) -> bool {
        self.value.eq(other)
    }
}

impl<T: Scalar + Writable> Writable for ScalarMut<'_, T> {
    #[inline]
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        self.value.write_to_vec::<TARGET>()
    }

    #[inline]
    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        self.value.write_to_writer::<TARGET>(writer)
    }
}
