use std::{
    borrow::{Borrow, BorrowMut},
    cmp::Ordering,
    collections::TryReserveError,
    fmt,
    hash::{Hash, Hasher},
    io::{self, IoSlice, Write},
    marker::PhantomData,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut, Index, IndexMut, RangeBounds},
    ptr,
    slice::{self, SliceIndex},
};

use simd_cesu8::DecodingError;
use zerocopy::Unalign;

use crate::{MUTF8Str, decode_mutf8_lossy, encode_mutf8};

macro_rules! impl_vec_common {
    ($ty:ident < $($lt:lifetime ,)? $elem:ident > $(, other_view: $other_view:ty)?) => {
        impl<$($lt,)? $elem> $ty<$($lt,)? $elem> {
            #[inline]
            fn with_vec<R>(&mut self, f: impl FnOnce(&mut Vec<$elem>) -> R) -> R {
                let mut vec = unsafe {
                    ManuallyDrop::new(Vec::from_raw_parts(
                        self.as_mut_ptr(),
                        self.len.get(),
                        self.cap.get(),
                    ))
                };
                let result = f(&mut vec);
                self.ptr.set(vec.as_mut_ptr().expose_provenance());
                self.len.set(vec.len());
                self.cap.set(vec.capacity());
                result
            }

            #[inline]
            pub fn len(&self) -> usize {
                self.len.get()
            }

            #[inline]
            pub fn is_empty(&self) -> bool {
                self.len.get() == 0
            }

            #[inline]
            pub fn capacity(&self) -> usize {
                self.cap.get()
            }

            #[inline]
            pub fn as_ptr(&self) -> *const $elem {
                ptr::with_exposed_provenance(self.ptr.get())
            }

            #[inline]
            pub fn as_mut_ptr(&mut self) -> *mut $elem {
                ptr::with_exposed_provenance_mut(self.ptr.get())
            }

            #[inline]
            pub fn as_slice(&self) -> &[$elem] {
                unsafe { slice::from_raw_parts(self.as_ptr(), self.len.get()) }
            }

            #[inline]
            pub fn as_mut_slice(&mut self) -> &mut [$elem] {
                unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len.get()) }
            }

            #[inline]
            pub unsafe fn set_len(&mut self, new_len: usize) {
                debug_assert!(new_len <= self.cap.get());
                self.len.set(new_len);
            }

            #[inline]
            pub fn reserve(&mut self, additional: usize) {
                self.with_vec(|v| v.reserve(additional));
            }

            #[inline]
            pub fn reserve_exact(&mut self, additional: usize) {
                self.with_vec(|v| v.reserve_exact(additional));
            }

            #[inline]
            pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
                self.with_vec(|v| v.try_reserve(additional))
            }

            #[inline]
            pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
                self.with_vec(|v| v.try_reserve_exact(additional))
            }

            #[inline]
            pub fn shrink_to_fit(&mut self) {
                self.with_vec(|v| v.shrink_to_fit());
            }

            #[inline]
            pub fn shrink_to(&mut self, min_capacity: usize) {
                self.with_vec(|v| v.shrink_to(min_capacity));
            }

            #[inline]
            pub fn spare_capacity_mut(&mut self) -> &mut [core::mem::MaybeUninit<$elem>] {
                self.with_vec(|v| {
                    let spare = v.spare_capacity_mut();
                    unsafe { core::slice::from_raw_parts_mut(spare.as_mut_ptr(), spare.len()) }
                })
            }

            #[inline]
            pub fn truncate(&mut self, len: usize) {
                self.with_vec(|v| v.truncate(len));
            }

            #[inline]
            pub fn clear(&mut self) {
                self.with_vec(|v| v.clear());
            }

            #[inline]
            pub fn push(&mut self, value: $elem) {
                self.with_vec(|v| v.push(value));
            }

            #[inline]
            pub fn pop(&mut self) -> Option<$elem> {
                self.with_vec(|v| v.pop())
            }

            #[inline]
            pub fn pop_if(&mut self, predicate: impl FnOnce(&mut $elem) -> bool) -> Option<$elem> {
                self.with_vec(|v| v.pop_if(predicate))
            }

            #[inline]
            pub fn insert(&mut self, index: usize, element: $elem) {
                self.with_vec(|v| v.insert(index, element));
            }

            #[inline]
            pub fn remove(&mut self, index: usize) -> $elem {
                self.with_vec(|v| v.remove(index))
            }

            #[inline]
            pub fn swap_remove(&mut self, index: usize) -> $elem {
                self.with_vec(|v| v.swap_remove(index))
            }

            #[inline]
            pub fn retain<F>(&mut self, f: F)
            where
                F: FnMut(&$elem) -> bool,
            {
                self.with_vec(|v| v.retain(f));
            }

            #[inline]
            pub fn retain_mut<F>(&mut self, f: F)
            where
                F: FnMut(&mut $elem) -> bool,
            {
                self.with_vec(|v| v.retain_mut(f));
            }

            #[inline]
            pub fn dedup(&mut self)
            where
                $elem: PartialEq,
            {
                self.with_vec(|v| v.dedup());
            }

            #[inline]
            pub fn dedup_by<F>(&mut self, same_bucket: F)
            where
                F: FnMut(&mut $elem, &mut $elem) -> bool,
            {
                self.with_vec(|v| v.dedup_by(same_bucket));
            }

            #[inline]
            pub fn dedup_by_key<F, K>(&mut self, key: F)
            where
                F: FnMut(&mut $elem) -> K,
                K: PartialEq,
            {
                self.with_vec(|v| v.dedup_by_key(key));
            }

            #[inline]
            pub fn extend_from_slice(&mut self, other: &[$elem])
            where
                $elem: Clone,
            {
                self.with_vec(|v| v.extend_from_slice(other));
            }

            #[inline]
            pub fn extend_from_within<R>(&mut self, src: R)
            where
                $elem: Copy,
                R: RangeBounds<usize>,
            {
                self.with_vec(|v| v.extend_from_within(src));
            }

            #[inline]
            pub fn resize(&mut self, new_len: usize, value: $elem)
            where
                $elem: Clone,
            {
                self.with_vec(|v| v.resize(new_len, value));
            }

            #[inline]
            pub fn resize_with<F>(&mut self, new_len: usize, f: F)
            where
                F: FnMut() -> $elem,
            {
                self.with_vec(|v| v.resize_with(new_len, f));
            }

            #[inline]
            pub fn splice_drop<R, I>(&mut self, range: R, replace_with: I)
            where
                R: RangeBounds<usize>,
                I: IntoIterator<Item = $elem>,
            {
                self.with_vec(|v| drop(v.splice(range, replace_with)));
            }

            #[inline]
            pub fn append(&mut self, other: &mut Vec<$elem>) {
                self.with_vec(|v| v.append(other));
            }

            $(
                #[inline]
                pub fn append_view(&mut self, other: &mut $other_view) {
                    other.with_vec(|other_vec| self.with_vec(|v| v.append(other_vec)));
                }
            )?

            #[inline]
            pub fn split_off(&mut self, at: usize) -> Vec<$elem> {
                self.with_vec(|v| v.split_off(at))
            }

            #[inline]
            pub fn drain_drop<R>(&mut self, range: R)
            where
                R: RangeBounds<usize>,
            {
                self.with_vec(|v| drop(v.drain(range)));
            }
        }

        impl<$($lt,)? $elem> Deref for $ty<$($lt,)? $elem> {
            type Target = [$elem];

            #[inline]
            fn deref(&self) -> &[$elem] {
                self.as_slice()
            }
        }

        impl<$($lt,)? $elem> DerefMut for $ty<$($lt,)? $elem> {
            #[inline]
            fn deref_mut(&mut self) -> &mut [$elem] {
                self.as_mut_slice()
            }
        }

        impl<$($lt,)? $elem, I: SliceIndex<[$elem]>> Index<I> for $ty<$($lt,)? $elem> {
            type Output = I::Output;

            #[inline]
            fn index(&self, index: I) -> &Self::Output {
                Index::index(self.as_slice(), index)
            }
        }

        impl<$($lt,)? $elem, I: SliceIndex<[$elem]>> IndexMut<I> for $ty<$($lt,)? $elem> {
            #[inline]
            fn index_mut(&mut self, index: I) -> &mut Self::Output {
                IndexMut::index_mut(self.as_mut_slice(), index)
            }
        }

        impl<$($lt,)? $elem: fmt::Debug> fmt::Debug for $ty<$($lt,)? $elem> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(self.as_slice(), f)
            }
        }

        impl<$($lt,)? $elem: PartialEq> PartialEq for $ty<$($lt,)? $elem> {
            fn eq(&self, other: &Self) -> bool {
                self.as_slice() == other.as_slice()
            }
        }

        impl<$($lt,)? $elem: PartialEq> PartialEq<Vec<$elem>> for $ty<$($lt,)? $elem> {
            fn eq(&self, other: &Vec<$elem>) -> bool {
                self.as_slice() == other.as_slice()
            }
        }

        impl<$($lt,)? $elem: PartialEq> PartialEq<[$elem]> for $ty<$($lt,)? $elem> {
            fn eq(&self, other: &[$elem]) -> bool {
                self.as_slice() == other
            }
        }

        impl<$($lt,)? $elem: PartialEq, const N: usize> PartialEq<[$elem; N]> for $ty<$($lt,)? $elem> {
            fn eq(&self, other: &[$elem; N]) -> bool {
                self.as_slice() == other
            }
        }

        impl<$($lt,)? $elem: PartialEq> PartialEq<&[$elem]> for $ty<$($lt,)? $elem> {
            fn eq(&self, other: &&[$elem]) -> bool {
                self.as_slice() == *other
            }
        }

        impl<$($lt,)? $elem: PartialEq> PartialEq<&mut [$elem]> for $ty<$($lt,)? $elem> {
            fn eq(&self, other: &&mut [$elem]) -> bool {
                self.as_slice() == *other
            }
        }

        impl<$($lt,)? $elem: Eq> Eq for $ty<$($lt,)? $elem> {}

        impl<$($lt,)? $elem: PartialOrd> PartialOrd for $ty<$($lt,)? $elem> {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                self.as_slice().partial_cmp(other.as_slice())
            }
        }

        impl<$($lt,)? $elem: Ord> Ord for $ty<$($lt,)? $elem> {
            fn cmp(&self, other: &Self) -> Ordering {
                self.as_slice().cmp(other.as_slice())
            }
        }

        impl<$($lt,)? $elem: Hash> Hash for $ty<$($lt,)? $elem> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.as_slice().hash(state);
            }
        }

        impl<$($lt,)? $elem> Borrow<[$elem]> for $ty<$($lt,)? $elem> {
            fn borrow(&self) -> &[$elem] {
                self.as_slice()
            }
        }

        impl<$($lt,)? $elem> BorrowMut<[$elem]> for $ty<$($lt,)? $elem> {
            fn borrow_mut(&mut self) -> &mut [$elem] {
                self.as_mut_slice()
            }
        }

        impl<$($lt,)? $elem> AsRef<[$elem]> for $ty<$($lt,)? $elem> {
            fn as_ref(&self) -> &[$elem] {
                self.as_slice()
            }
        }

        impl<$($lt,)? $elem> AsMut<[$elem]> for $ty<$($lt,)? $elem> {
            fn as_mut(&mut self) -> &mut [$elem] {
                self.as_mut_slice()
            }
        }

        impl<'iter, $($lt,)? $elem> IntoIterator for &'iter $ty<$($lt,)? $elem> {
            type Item = &'iter $elem;
            type IntoIter = slice::Iter<'iter, $elem>;

            fn into_iter(self) -> Self::IntoIter {
                self.iter()
            }
        }

        impl<'iter, $($lt,)? $elem> IntoIterator for &'iter mut $ty<$($lt,)? $elem> {
            type Item = &'iter mut $elem;
            type IntoIter = slice::IterMut<'iter, $elem>;

            fn into_iter(self) -> Self::IntoIter {
                self.as_mut_slice().iter_mut()
            }
        }

        impl<$($lt,)? $elem> Extend<$elem> for $ty<$($lt,)? $elem> {
            fn extend<I: IntoIterator<Item = $elem>>(&mut self, iter: I) {
                self.with_vec(|v| v.extend(iter));
            }
        }

        impl<'iter, $($lt,)? $elem: Copy + 'iter> Extend<&'iter $elem> for $ty<$($lt,)? $elem> {
            fn extend<I: IntoIterator<Item = &'iter $elem>>(&mut self, iter: I) {
                self.with_vec(|v| v.extend(iter));
            }
        }
    };
}

macro_rules! impl_vec_u8_write {
    ($ty:ty) => {
        impl Write for $ty {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.with_vec(|v| v.write(buf))
            }

            fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
                self.with_vec(|v| v.write_vectored(bufs))
            }

            fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
                self.with_vec(|v| v.write_all(buf))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
    };
}

macro_rules! impl_string_common {
    ($ty:ident $(<$lt:lifetime>)?) => {
        impl$(<$lt>)? $ty$(<$lt>)? {
            #[inline]
            fn with_string<R>(&mut self, f: impl FnOnce(&mut String) -> R) -> R {
                use std::borrow::Cow;

                let old_ptr = self.as_mut_ptr();
                let old_len = self.len.get();
                let old_cap = self.cap.get();

                let data = unsafe { slice::from_raw_parts(old_ptr, old_len) };
                let decoded = decode_mutf8_lossy(data);

                let mut string = match decoded {
                    Cow::Borrowed(_) => unsafe { String::from_raw_parts(old_ptr, old_len, old_cap) },
                    Cow::Owned(s) => {
                        unsafe {
                            drop(Vec::<u8>::from_raw_parts(old_ptr, old_len, old_cap));
                        }
                        s
                    }
                };

                let result = f(&mut string);
                let encoded = encode_mutf8(&string);

                match encoded {
                    Cow::Borrowed(_) => {
                        let mut string = ManuallyDrop::new(string);
                        let vec = unsafe { string.as_mut_vec() };
                        self.ptr.set(vec.as_mut_ptr().expose_provenance());
                        self.len.set(vec.len());
                        self.cap.set(vec.capacity());
                    }
                    Cow::Owned(vec) => {
                        drop(string);
                        let mut vec = ManuallyDrop::new(vec);
                        self.ptr.set(vec.as_mut_ptr().expose_provenance());
                        self.len.set(vec.len());
                        self.cap.set(vec.capacity());
                    }
                }

                result
            }

            #[inline]
            pub fn len(&self) -> usize {
                self.len.get()
            }

            #[inline]
            pub fn is_empty(&self) -> bool {
                self.len.get() == 0
            }

            #[inline]
            pub fn capacity(&self) -> usize {
                self.cap.get()
            }

            #[inline]
            pub fn as_bytes(&self) -> &[u8] {
                unsafe { slice::from_raw_parts(self.as_ptr(), self.len.get()) }
            }

            #[inline]
            pub fn as_mutf8_str(&self) -> &MUTF8Str {
                unsafe { MUTF8Str::from_mutf8_unchecked(self.as_bytes()) }
            }

            #[inline]
            pub fn decode_lossy(&self) -> std::borrow::Cow<'_, str> {
                self.as_mutf8_str().decode_lossy()
            }

            #[inline]
            pub fn decode_strict(
                &self,
            ) -> std::result::Result<std::borrow::Cow<'_, str>, DecodingError> {
                self.as_mutf8_str().decode_strict()
            }

            #[inline]
            pub fn decode(&self) -> std::result::Result<std::borrow::Cow<'_, str>, DecodingError> {
                self.as_mutf8_str().decode()
            }

            #[inline]
            pub fn decode_lossy_strict(&self) -> std::borrow::Cow<'_, str> {
                self.as_mutf8_str().decode_lossy_strict()
            }

            #[inline]
            pub fn as_ptr(&self) -> *const u8 {
                ptr::with_exposed_provenance(self.ptr.get())
            }

            #[inline]
            pub fn as_mut_ptr(&mut self) -> *mut u8 {
                ptr::with_exposed_provenance_mut(self.ptr.get())
            }

            #[inline]
            pub fn reserve(&mut self, additional: usize) {
                self.with_string(|s| s.reserve(additional));
            }

            #[inline]
            pub fn reserve_exact(&mut self, additional: usize) {
                self.with_string(|s| s.reserve_exact(additional));
            }

            #[inline]
            pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
                self.with_string(|s| s.try_reserve(additional))
            }

            #[inline]
            pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
                self.with_string(|s| s.try_reserve_exact(additional))
            }

            #[inline]
            pub fn shrink_to_fit(&mut self) {
                self.with_string(|s| s.shrink_to_fit());
            }

            #[inline]
            pub fn shrink_to(&mut self, min_capacity: usize) {
                self.with_string(|s| s.shrink_to(min_capacity));
            }

            #[inline]
            pub fn push_str(&mut self, string: &str) {
                self.with_string(|s| s.push_str(string));
            }

            #[inline]
            pub fn push(&mut self, ch: char) {
                self.with_string(|s| s.push(ch));
            }

            #[inline]
            pub fn truncate(&mut self, new_len: usize) {
                self.with_string(|s| s.truncate(new_len));
            }

            #[inline]
            pub fn pop(&mut self) -> Option<char> {
                self.with_string(|s| s.pop())
            }

            #[inline]
            pub fn remove(&mut self, idx: usize) -> char {
                self.with_string(|s| s.remove(idx))
            }

            #[inline]
            pub fn retain<F>(&mut self, f: F)
            where
                F: FnMut(char) -> bool,
            {
                self.with_string(|s| s.retain(f));
            }

            #[inline]
            pub fn insert(&mut self, idx: usize, ch: char) {
                self.with_string(|s| s.insert(idx, ch));
            }

            #[inline]
            pub fn insert_str(&mut self, idx: usize, string: &str) {
                self.with_string(|s| s.insert_str(idx, string));
            }

            #[inline]
            pub fn clear(&mut self) {
                self.with_string(|s| s.clear());
            }

            #[inline]
            pub fn split_off(&mut self, at: usize) -> String {
                self.with_string(|s| s.split_off(at))
            }

            #[inline]
            pub fn extend_from_within<R>(&mut self, src: R)
            where
                R: RangeBounds<usize>,
            {
                self.with_string(|s| s.extend_from_within(src));
            }

            #[inline]
            pub fn replace_range<R>(&mut self, range: R, replace_with: &str)
            where
                R: RangeBounds<usize>,
            {
                self.with_string(|s| s.replace_range(range, replace_with));
            }

            #[inline]
            pub fn drain_drop<R>(&mut self, range: R)
            where
                R: RangeBounds<usize>,
            {
                self.with_string(|s| drop(s.drain(range)));
            }
        }

        impl$(<$lt>)? fmt::Debug for $ty$(<$lt>)? {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(&*self.decode_lossy(), f)
            }
        }

        impl$(<$lt>)? fmt::Display for $ty$(<$lt>)? {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&*self.decode_lossy(), f)
            }
        }

        impl$(<$lt>)? PartialEq for $ty$(<$lt>)? {
            fn eq(&self, other: &Self) -> bool {
                self.as_bytes() == other.as_bytes()
            }
        }

        impl$(<$lt>)? PartialEq<String> for $ty$(<$lt>)? {
            fn eq(&self, other: &String) -> bool {
                &*self.decode_lossy() == other.as_str()
            }
        }

        impl$(<$lt>)? PartialEq<str> for $ty$(<$lt>)? {
            fn eq(&self, other: &str) -> bool {
                &*self.decode_lossy() == other
            }
        }

        impl$(<$lt>)? PartialEq<&str> for $ty$(<$lt>)? {
            fn eq(&self, other: &&str) -> bool {
                &*self.decode_lossy() == *other
            }
        }

        impl$(<$lt>)? Eq for $ty$(<$lt>)? {}

        impl$(<$lt>)? PartialOrd for $ty$(<$lt>)? {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl$(<$lt>)? Ord for $ty$(<$lt>)? {
            fn cmp(&self, other: &Self) -> Ordering {
                self.as_bytes().cmp(other.as_bytes())
            }
        }

        impl$(<$lt>)? Hash for $ty$(<$lt>)? {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.as_bytes().hash(state);
            }
        }

        impl$(<$lt>)? AsRef<[u8]> for $ty$(<$lt>)? {
            fn as_ref(&self) -> &[u8] {
                self.as_bytes()
            }
        }

        impl$(<$lt>)? Write for $ty$(<$lt>)? {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                match std::str::from_utf8(buf) {
                    Ok(s) => {
                        self.push_str(s);
                        Ok(buf.len())
                    }
                    Err(_) => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid UTF-8 in write",
                    )),
                }
            }

            fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
                match std::str::from_utf8(buf) {
                    Ok(s) => {
                        self.push_str(s);
                        Ok(())
                    }
                    Err(_) => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid UTF-8 in write_all",
                    )),
                }
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
    };
}

#[repr(C)]
pub struct VecMut<'a, T> {
    pub(crate) ptr: &'a mut Unalign<usize>,
    pub(crate) len: &'a mut Unalign<usize>,
    pub(crate) cap: &'a mut Unalign<usize>,
    _marker: PhantomData<T>,
}

unsafe impl<T: Send> Send for VecMut<'_, T> {}
unsafe impl<T: Sync> Sync for VecMut<'_, T> {}

impl<'a, T> VecMut<'a, T> {
    #[inline]
    pub unsafe fn new(
        ptr: &'a mut Unalign<usize>,
        len: &'a mut Unalign<usize>,
        cap: &'a mut Unalign<usize>,
    ) -> Self {
        debug_assert!(len.get() <= cap.get());
        Self {
            ptr,
            len,
            cap,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub unsafe fn new_clone(&mut self) -> Self {
        VecMut {
            ptr: unsafe { &mut *(self.ptr as *mut _) },
            len: unsafe { &mut *(self.len as *mut _) },
            cap: unsafe { &mut *(self.cap as *mut _) },
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn into_raw_parts(
        self,
    ) -> (
        &'a mut Unalign<usize>,
        &'a mut Unalign<usize>,
        &'a mut Unalign<usize>,
    ) {
        (self.ptr, self.len, self.cap)
    }
}

impl_vec_common!(VecMut<'a, T>, other_view: VecMut<'_, T>);
impl_vec_u8_write!(VecMut<'_, u8>);

#[repr(C)]
pub struct VecOwn<T> {
    pub(crate) ptr: Unalign<usize>,
    pub(crate) len: Unalign<usize>,
    pub(crate) cap: Unalign<usize>,
    _marker: PhantomData<T>,
}

unsafe impl<T: Send> Send for VecOwn<T> {}
unsafe impl<T: Sync> Sync for VecOwn<T> {}

impl<T> Default for VecOwn<T> {
    fn default() -> Self {
        vec![].into()
    }
}

impl<T> VecOwn<T> {
    #[inline]
    pub unsafe fn new(ptr: Unalign<usize>, len: Unalign<usize>, cap: Unalign<usize>) -> Self {
        debug_assert!(len.get() <= cap.get());
        Self {
            ptr,
            len,
            cap,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub const fn to_mut<'a>(&'a mut self) -> VecMut<'a, T> {
        VecMut {
            ptr: &mut self.ptr,
            len: &mut self.len,
            cap: &mut self.cap,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn into_raw_parts(self) -> (Unalign<usize>, Unalign<usize>, Unalign<usize>) {
        let me = ManuallyDrop::new(self);
        (me.ptr, me.len, me.cap)
    }
}

impl_vec_common!(VecOwn<T>, other_view: VecOwn<T>);
impl_vec_u8_write!(VecOwn<u8>);

impl<T> From<Vec<T>> for VecOwn<T> {
    #[inline]
    fn from(value: Vec<T>) -> Self {
        let mut me = ManuallyDrop::new(value);
        VecOwn {
            ptr: Unalign::new(me.as_mut_ptr().expose_provenance()),
            len: Unalign::new(me.len()),
            cap: Unalign::new(me.capacity()),
            _marker: PhantomData,
        }
    }
}

impl<T: Clone> From<&[T]> for VecOwn<T> {
    fn from(value: &[T]) -> Self {
        value.to_vec().into()
    }
}

impl<T> Drop for VecOwn<T> {
    fn drop(&mut self) {
        unsafe { Vec::from_raw_parts(self.as_mut_ptr(), self.len.get(), self.cap.get()) };
    }
}

#[repr(C)]
pub struct StringMut<'a> {
    pub(crate) ptr: &'a mut Unalign<usize>,
    pub(crate) len: &'a mut Unalign<usize>,
    pub(crate) cap: &'a mut Unalign<usize>,
}

unsafe impl Send for StringMut<'_> {}
unsafe impl Sync for StringMut<'_> {}

impl<'a> StringMut<'a> {
    #[inline]
    pub unsafe fn new(
        ptr: &'a mut Unalign<usize>,
        len: &'a mut Unalign<usize>,
        cap: &'a mut Unalign<usize>,
    ) -> Self {
        debug_assert!(len.get() <= cap.get());
        Self { ptr, len, cap }
    }

    #[inline]
    pub fn into_raw_parts(
        self,
    ) -> (
        &'a mut Unalign<usize>,
        &'a mut Unalign<usize>,
        &'a mut Unalign<usize>,
    ) {
        (self.ptr, self.len, self.cap)
    }
}

impl_string_common!(StringMut<'a>);

#[repr(C)]
pub struct StringOwn {
    pub(crate) ptr: Unalign<usize>,
    pub(crate) len: Unalign<usize>,
    pub(crate) cap: Unalign<usize>,
}

unsafe impl Send for StringOwn {}
unsafe impl Sync for StringOwn {}

impl Default for StringOwn {
    fn default() -> Self {
        vec![].into()
    }
}

impl StringOwn {
    #[inline]
    pub unsafe fn new(ptr: Unalign<usize>, len: Unalign<usize>, cap: Unalign<usize>) -> Self {
        debug_assert!(len.get() <= cap.get());
        Self { ptr, len, cap }
    }

    #[inline]
    pub const fn to_mut<'a>(&'a mut self) -> StringMut<'a> {
        StringMut {
            ptr: &mut self.ptr,
            len: &mut self.len,
            cap: &mut self.cap,
        }
    }

    #[inline]
    pub fn into_raw_parts(self) -> (Unalign<usize>, Unalign<usize>, Unalign<usize>) {
        let me = ManuallyDrop::new(self);
        (me.ptr, me.len, me.cap)
    }
}

impl_string_common!(StringOwn);

impl From<Vec<u8>> for StringOwn {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        let mut encoded = ManuallyDrop::new(value);
        StringOwn {
            ptr: Unalign::new(encoded.as_mut_ptr().expose_provenance()),
            len: Unalign::new(encoded.len()),
            cap: Unalign::new(encoded.capacity()),
        }
    }
}

impl From<&[u8]> for StringOwn {
    fn from(value: &[u8]) -> Self {
        value.to_vec().into()
    }
}

impl From<String> for StringOwn {
    #[inline]
    fn from(value: String) -> Self {
        match encode_mutf8(&value) {
            std::borrow::Cow::Borrowed(_) => value.into_bytes().into(),
            std::borrow::Cow::Owned(encoded) => encoded.into(),
        }
    }
}

impl From<&str> for StringOwn {
    #[inline]
    fn from(value: &str) -> Self {
        encode_mutf8(value).into_owned().into()
    }
}

impl From<&MUTF8Str> for StringOwn {
    #[inline]
    fn from(value: &MUTF8Str) -> Self {
        value.as_bytes().into()
    }
}

impl Drop for StringOwn {
    fn drop(&mut self) {
        unsafe {
            drop(Vec::<u8>::from_raw_parts(
                self.as_mut_ptr(),
                self.len.get(),
                self.cap.get(),
            ));
        }
    }
}
