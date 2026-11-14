use std::{
    borrow::{Borrow, Cow},
    fmt,
    hint::assert_unchecked,
    mem,
    ops::Deref,
    str::FromStr,
};

use simd_cesu8::DecodingError;

use crate::Error;

#[inline(always)]
#[cold]
pub(crate) const fn cold_path() {}

#[cold]
#[inline(never)]
fn decode_mutf8_lossy_slow(bytes: &[u8]) -> Cow<'_, str> {
    simd_cesu8::mutf8::decode_lossy(bytes)
}

#[cold]
#[inline(never)]
fn decode_mutf8_lossy_owned_slow(bytes: &[u8]) -> String {
    simd_cesu8::mutf8::decode_lossy(bytes).into_owned()
}

#[cold]
#[inline(never)]
fn encode_mutf8_slow(value: &str) -> Cow<'_, [u8]> {
    simd_cesu8::mutf8::encode(value)
}

#[cold]
#[inline(never)]
fn with_mutf8_key_non_short<R>(key: &str, f: impl FnOnce(&MUTF8Str) -> R) -> R {
    if !simd_cesu8::mutf8::needs_encoded(key) {
        f(unsafe { MUTF8Str::from_mutf8_unchecked(key.as_bytes()) })
    } else {
        let encoded = simd_cesu8::mutf8::encode(key);
        f(unsafe { MUTF8Str::from_mutf8_unchecked(&encoded) })
    }
}

/// Compares two `len`-byte keys, inlining the comparison for keys of up to 16 bytes.
///
/// Slice equality lowers to an out-of-line `bcmp` call, which dominates compound key lookups
/// because NBT keys are almost always short. Overlapping fixed-width loads stay within
/// `[0, len)` on both sides.
///
/// # Safety
///
/// `a` and `b` must each be valid for reads of `len` bytes.
#[inline(always)]
pub(crate) unsafe fn key_eq(a: *const u8, b: *const u8, len: usize) -> bool {
    unsafe {
        if len > 16 {
            std::slice::from_raw_parts(a, len) == std::slice::from_raw_parts(b, len)
        } else if len >= 8 {
            let head = a.cast::<u64>().read_unaligned() ^ b.cast::<u64>().read_unaligned();
            let tail = a.add(len - 8).cast::<u64>().read_unaligned()
                ^ b.add(len - 8).cast::<u64>().read_unaligned();
            (head | tail) == 0
        } else if len >= 4 {
            let head = a.cast::<u32>().read_unaligned() ^ b.cast::<u32>().read_unaligned();
            let tail = a.add(len - 4).cast::<u32>().read_unaligned()
                ^ b.add(len - 4).cast::<u32>().read_unaligned();
            (head | tail) == 0
        } else if len >= 2 {
            let head = a.cast::<u16>().read_unaligned() ^ b.cast::<u16>().read_unaligned();
            let tail = a.add(len - 2).cast::<u16>().read_unaligned()
                ^ b.add(len - 2).cast::<u16>().read_unaligned();
            (head | tail) == 0
        } else {
            len == 0 || *a == *b
        }
    }
}

#[inline(always)]
fn is_ascii_short_16(bytes: &[u8]) -> bool {
    let len = bytes.len();
    if len == 0 {
        return true;
    }
    if len < 4 {
        return (bytes[0] | bytes[len >> 1] | bytes[len - 1]) < 0x80;
    }
    if len < 8 {
        let a = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
        let b = u32::from_ne_bytes(bytes[len - 4..].try_into().unwrap());
        return ((a | b) & 0x8080_8080) == 0;
    }
    let a = u64::from_ne_bytes(bytes[..8].try_into().unwrap());
    let b = u64::from_ne_bytes(bytes[len - 8..].try_into().unwrap());
    ((a | b) & 0x8080_8080_8080_8080) == 0
}

#[inline(always)]
fn is_non_null_ascii_short_16(bytes: &[u8]) -> bool {
    let len = bytes.len();
    if len == 0 {
        return true;
    }
    if len < 4 {
        return (bytes[0].wrapping_sub(1) < 0x7F)
            & (bytes[len >> 1].wrapping_sub(1) < 0x7F)
            & (bytes[len - 1].wrapping_sub(1) < 0x7F);
    }
    if len < 8 {
        let a = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
        let b = u32::from_ne_bytes(bytes[len - 4..].try_into().unwrap());
        return (((a | a.wrapping_sub(0x0101_0101)) | (b | b.wrapping_sub(0x0101_0101)))
            & 0x8080_8080)
            == 0;
    }
    let a = u64::from_ne_bytes(bytes[..8].try_into().unwrap());
    let b = u64::from_ne_bytes(bytes[len - 8..].try_into().unwrap());
    (((a | a.wrapping_sub(0x0101_0101_0101_0101)) | (b | b.wrapping_sub(0x0101_0101_0101_0101)))
        & 0x8080_8080_8080_8080)
        == 0
}

#[inline(always)]
fn is_valid_utf8_under_64(bytes: &[u8]) -> bool {
    let ok_ascii = if bytes.len() <= 16 {
        is_ascii_short_16(bytes)
    } else {
        bytes[0] < 0x80 && bytes.is_ascii()
    };
    ok_ascii || std::str::from_utf8(bytes).is_ok()
}

#[inline(always)]
fn is_strict_mutf8_ascii_under_64(bytes: &[u8]) -> bool {
    if bytes.len() <= 16 {
        return is_non_null_ascii_short_16(bytes);
    }
    if bytes[0].wrapping_sub(1) >= 0x7F {
        return false;
    }
    let (chunks, _) = bytes.as_chunks::<8>();
    for &chunk in chunks {
        let word = u64::from_ne_bytes(chunk);
        if ((word | word.wrapping_sub(0x0101_0101_0101_0101)) & 0x8080_8080_8080_8080) != 0 {
            return false;
        }
    }
    let last = u64::from_ne_bytes(bytes[bytes.len() - 8..].try_into().unwrap());
    ((last | last.wrapping_sub(0x0101_0101_0101_0101)) & 0x8080_8080_8080_8080) == 0
}

#[inline(always)]
pub(crate) fn decode_mutf8_lossy(bytes: &[u8]) -> Cow<'_, str> {
    if bytes.len() < 64 {
        if is_valid_utf8_under_64(bytes) {
            // SAFETY: Validated as ASCII or UTF-8 above.
            Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(bytes) })
        } else {
            decode_mutf8_lossy_slow(bytes)
        }
    } else {
        simd_cesu8::mutf8::decode_lossy(bytes)
    }
}

#[inline(always)]
fn encode_mutf8(value: &str) -> Cow<'_, [u8]> {
    let bytes = value.as_bytes();
    if (bytes.len() < 64 && is_strict_mutf8_ascii_under_64(bytes))
        || !simd_cesu8::mutf8::needs_encoded(value)
    {
        Cow::Borrowed(bytes)
    } else {
        encode_mutf8_slow(value)
    }
}

#[inline(always)]
pub(crate) fn with_mutf8_key<R>(key: &str, f: impl FnOnce(&MUTF8Str) -> R) -> R {
    let bytes = key.as_bytes();
    if bytes.len() < 64 && is_strict_mutf8_ascii_under_64(bytes) {
        f(unsafe { MUTF8Str::from_mutf8_unchecked(bytes) })
    } else {
        with_mutf8_key_non_short(key, f)
    }
}

/// Marker trait for wire byte order ([`BigEndian`](zerocopy::BigEndian) or
/// [`LittleEndian`](zerocopy::LittleEndian)).
pub trait ByteOrder: zerocopy::ByteOrder + Send + Sync + 'static {}

impl<T: zerocopy::ByteOrder + Send + Sync + 'static> ByteOrder for T {}

#[inline(always)]
pub(crate) const fn same_byte_order<A: ByteOrder, B: ByteOrder>() -> bool {
    matches!(
        (A::ORDER, B::ORDER),
        (zerocopy::Order::BigEndian, zerocopy::Order::BigEndian)
            | (zerocopy::Order::LittleEndian, zerocopy::Order::LittleEndian)
    )
}

pub(crate) static EMPTY_LIST: [u8; 5] = [0; 5];
pub(crate) static EMPTY_COMPOUND: [u8; 1] = [0];
pub(crate) static EMPTY_TYPED_LISTS: [[u8; 5]; 13] = [
    [0, 0, 0, 0, 0],
    [1, 0, 0, 0, 0],
    [2, 0, 0, 0, 0],
    [3, 0, 0, 0, 0],
    [4, 0, 0, 0, 0],
    [5, 0, 0, 0, 0],
    [6, 0, 0, 0, 0],
    [7, 0, 0, 0, 0],
    [8, 0, 0, 0, 0],
    [9, 0, 0, 0, 0],
    [10, 0, 0, 0, 0],
    [11, 0, 0, 0, 0],
    [12, 0, 0, 0, 0],
];

/// An NBT string byte slice (nominally Modified UTF-8).
///
/// Zero-copy views and byte-slice compound keys wrap raw wire bytes without upfront encoding
/// validation; [`decode`](Self::decode), [`decode_strict`](Self::decode_strict), and lossy
/// helpers validate or replace invalid sequences on demand, so arbitrary bytes never cause
/// undefined behavior.
#[repr(transparent)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MUTF8Str([u8]);

impl Default for &MUTF8Str {
    #[inline]
    fn default() -> Self {
        unsafe { MUTF8Str::from_mutf8_unchecked(&[]) }
    }
}

impl fmt::Debug for MUTF8Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.decode_lossy(), f)
    }
}

impl fmt::Display for MUTF8Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.decode_lossy(), f)
    }
}

/// # Panics
///
/// `to_owned` panics if the slice is longer than [`MUTF8String::MAX_LEN`]; use
/// [`MUTF8String::try_from`] to handle that case.
impl ToOwned for MUTF8Str {
    type Owned = MUTF8String;

    #[inline]
    #[track_caller]
    fn to_owned(&self) -> Self::Owned {
        mutf8_or_panic(self)
    }
}

impl MUTF8Str {
    /// Creates a `&MUTF8Str` from a byte slice without validation.
    ///
    /// # Safety
    ///
    /// Callers expecting strict MUTF-8 semantics should pass valid MUTF-8 encoded data. Passing
    /// arbitrary bytes does not cause memory unsafety (`MUTF8Str` validates on decode, and parsed
    /// NBT wire strings / raw byte keys may contain arbitrary bytes).
    ///
    /// ```
    /// use na_nbt::MUTF8Str;
    ///
    /// let s = unsafe { MUTF8Str::from_mutf8_unchecked(b"hello") };
    /// assert_eq!(s.as_bytes(), b"hello");
    /// ```
    #[inline]
    pub const unsafe fn from_mutf8_unchecked(bytes: &[u8]) -> &Self {
        unsafe { mem::transmute(bytes) }
    }

    /// Returns the raw MUTF-8 bytes of this string slice.
    #[inline]
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the byte length of the encoded MUTF-8 slice.
    #[inline]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the encoded byte length is zero.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// See [`simd_cesu8::mutf8::decode_lossy`].
    #[inline]
    pub fn decode_lossy(&self) -> Cow<'_, str> {
        decode_mutf8_lossy(&self.0)
    }

    /// See [`simd_cesu8::mutf8::decode_strict`].
    #[inline]
    pub fn decode_strict(&self) -> Result<Cow<'_, str>, DecodingError> {
        if self.0.len() < 64 && is_strict_mutf8_ascii_under_64(&self.0) {
            Ok(Cow::Borrowed(unsafe {
                std::str::from_utf8_unchecked(&self.0)
            }))
        } else {
            simd_cesu8::mutf8::decode_strict(&self.0)
        }
    }

    /// See [`simd_cesu8::mutf8::decode`].
    #[inline]
    pub fn decode(&self) -> Result<Cow<'_, str>, DecodingError> {
        if self.0.len() < 64
            && (if self.0.len() <= 16 {
                is_ascii_short_16(&self.0)
            } else {
                self.0[0] < 0x80 && self.0.is_ascii()
            })
        {
            Ok(Cow::Borrowed(unsafe {
                std::str::from_utf8_unchecked(&self.0)
            }))
        } else {
            simd_cesu8::mutf8::decode(&self.0)
        }
    }

    /// See [`simd_cesu8::mutf8::decode_lossy_strict`].
    #[inline]
    pub fn decode_lossy_strict(&self) -> Cow<'_, str> {
        if self.0.len() < 64 && is_strict_mutf8_ascii_under_64(&self.0) {
            Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(&self.0) })
        } else {
            simd_cesu8::mutf8::decode_lossy_strict(&self.0)
        }
    }

    /// Decodes lossily into an owned [`String`].
    #[inline]
    pub fn to_utf8_string(&self) -> String {
        if self.0.len() < 64 {
            if is_valid_utf8_under_64(&self.0) {
                unsafe { String::from_utf8_unchecked(self.0.to_vec()) }
            } else {
                decode_mutf8_lossy_owned_slow(&self.0)
            }
        } else {
            simd_cesu8::mutf8::decode_lossy(&self.0).into_owned()
        }
    }
}

/// An owned, growable MUTF-8 encoded string of at most [`MUTF8String::MAX_LEN`] bytes.
///
/// Every MUTF-8 string in NBT (compound keys, root names and `TAG_String` payloads) is stored
/// with a `u16` length prefix, the same limit as Java's `DataOutput.writeUTF`. `MUTF8String`
/// keeps that limit as a type invariant: every constructor and every mutating method checks the
/// encoded length, so a value stored in an owned [`Compound`](crate::Compound) or
/// [`List`](crate::List) can always be written without truncation.
///
/// Conversions from Rust strings are fallible ([`TryFrom`]); `push*` methods panic on overflow
/// and have non-panicking `try_push*` counterparts.
#[repr(transparent)]
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MUTF8String(Vec<u8>);

#[cold]
#[inline(never)]
#[track_caller]
fn mutf8_too_long(len: usize) -> ! {
    panic!(
        "MUTF-8 string too long: {len} bytes (NBT strings hold at most {} bytes)",
        MUTF8String::MAX_LEN
    )
}

impl MUTF8String {
    /// Maximum encoded length in bytes (`u16::MAX`).
    pub const MAX_LEN: usize = u16::MAX as usize;

    /// Creates an empty `MUTF8String` without heap allocation.
    #[inline]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Creates an empty `MUTF8String` with pre-allocated byte capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }

    /// Creates an owned `MUTF8String` from raw MUTF-8 bytes without validating the encoding.
    ///
    /// # Panics
    ///
    /// Panics if `bytes` is longer than [`MAX_LEN`](Self::MAX_LEN).
    ///
    /// # Safety
    ///
    /// Callers expecting strict MUTF-8 semantics should pass valid MUTF-8 encoded data. Arbitrary
    /// bytes with `bytes.len() <= MAX_LEN` do not cause memory unsafety (`MUTF8String` validates
    /// on decode).
    #[inline]
    #[track_caller]
    pub const unsafe fn from_mutf8_unchecked(bytes: Vec<u8>) -> Self {
        assert!(
            bytes.len() <= Self::MAX_LEN,
            "MUTF-8 string too long (NBT strings hold at most 65535 bytes)"
        );
        Self(bytes)
    }

    /// Wraps bytes whose length is known to be in range (parsed NBT data).
    #[inline(always)]
    pub(crate) fn from_raw_slice(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() <= Self::MAX_LEN);
        Self(bytes.to_vec())
    }

    #[inline(always)]
    fn from_slice_checked(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > Self::MAX_LEN {
            cold_path();
            return Err(Error::LEN(bytes.len()));
        }
        Ok(Self(bytes.to_vec()))
    }

    #[inline(always)]
    fn from_vec_checked(bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.len() > Self::MAX_LEN {
            cold_path();
            return Err(Error::LEN(bytes.len()));
        }
        Ok(Self(bytes))
    }

    #[inline(always)]
    fn from_encoded(bytes: Cow<'_, [u8]>) -> Result<Self, Error> {
        match bytes {
            Cow::Borrowed(b) => Self::from_slice_checked(b),
            Cow::Owned(v) => Self::from_vec_checked(v),
        }
    }

    /// Borrows this string as a [`&MUTF8Str`](MUTF8Str) slice.
    #[inline]
    pub fn as_mutf8_str(&self) -> &MUTF8Str {
        // SAFETY: Every constructor and mutator preserves `self.0.len() <= Self::MAX_LEN`.
        unsafe {
            assert_unchecked(self.0.len() <= Self::MAX_LEN);
            MUTF8Str::from_mutf8_unchecked(&self.0)
        }
    }

    /// Consumes this string and returns the underlying MUTF-8 byte vector.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Clears the string, removing all contents while retaining allocated capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Reserves capacity for at least `additional` more MUTF-8 bytes.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    /// Bytes that can still be appended before reaching [`MAX_LEN`](Self::MAX_LEN).
    #[inline]
    pub fn remaining(&self) -> usize {
        unsafe { assert_unchecked(self.0.len() <= Self::MAX_LEN) };
        Self::MAX_LEN - self.0.len()
    }

    #[inline(always)]
    fn try_extend(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > self.remaining() {
            cold_path();
            return Err(Error::LEN(self.0.len() + bytes.len()));
        }
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    /// Appends a single Unicode character encoded as MUTF-8.
    ///
    /// # Panics
    ///
    /// Panics if the result would exceed [`MAX_LEN`](Self::MAX_LEN) bytes.
    #[inline]
    #[track_caller]
    pub fn push(&mut self, ch: char) {
        if let Err(Error::LEN(len)) = self.try_push(ch) {
            mutf8_too_long(len);
        }
    }

    /// Encodes a UTF-8 `&str` as MUTF-8 and appends it to this string.
    ///
    /// # Panics
    ///
    /// Panics if the result would exceed [`MAX_LEN`](Self::MAX_LEN) bytes.
    #[inline]
    #[track_caller]
    pub fn push_str(&mut self, s: &str) {
        if let Err(Error::LEN(len)) = self.try_push_str(s) {
            mutf8_too_long(len);
        }
    }

    /// Appends an already-encoded [`&MUTF8Str`](MUTF8Str) directly.
    ///
    /// # Panics
    ///
    /// Panics if the result would exceed [`MAX_LEN`](Self::MAX_LEN) bytes.
    #[inline]
    #[track_caller]
    pub fn push_mutf8(&mut self, s: &MUTF8Str) {
        if let Err(Error::LEN(len)) = self.try_push_mutf8(s) {
            mutf8_too_long(len);
        }
    }

    /// Like [`push`](Self::push), but returns [`Error::LEN`] (leaving `self` unchanged) instead
    /// of panicking when the result would exceed [`MAX_LEN`](Self::MAX_LEN) bytes.
    #[inline]
    pub fn try_push(&mut self, ch: char) -> Result<(), Error> {
        let code = ch as u32;
        if code.wrapping_sub(1) < 0x7F {
            return self.try_extend(&[code as u8]);
        }
        if code == 0 {
            return self.try_extend(&[0xC0, 0x80]);
        }
        let mut buf = [0u8; 4];
        self.try_push_str(ch.encode_utf8(&mut buf))
    }

    /// Like [`push_str`](Self::push_str), but returns [`Error::LEN`] (leaving `self` unchanged)
    /// instead of panicking when the result would exceed [`MAX_LEN`](Self::MAX_LEN) bytes.
    #[inline]
    pub fn try_push_str(&mut self, s: &str) -> Result<(), Error> {
        let bytes = s.as_bytes();
        if bytes.len() < 64 && is_strict_mutf8_ascii_under_64(bytes) {
            return self.try_extend(bytes);
        }
        self.try_extend(&encode_mutf8(s))
    }

    /// Like [`push_mutf8`](Self::push_mutf8), but returns [`Error::LEN`] (leaving `self`
    /// unchanged) instead of panicking when the result would exceed [`MAX_LEN`](Self::MAX_LEN)
    /// bytes.
    #[inline]
    pub fn try_push_mutf8(&mut self, s: &MUTF8Str) -> Result<(), Error> {
        self.try_extend(s.as_bytes())
    }
}

impl Deref for MUTF8String {
    type Target = MUTF8Str;

    #[inline]
    fn deref(&self) -> &MUTF8Str {
        self.as_mutf8_str()
    }
}

impl Borrow<MUTF8Str> for MUTF8String {
    #[inline]
    fn borrow(&self) -> &MUTF8Str {
        self.as_mutf8_str()
    }
}

impl AsRef<MUTF8Str> for MUTF8String {
    #[inline]
    fn as_ref(&self) -> &MUTF8Str {
        self.as_mutf8_str()
    }
}

impl AsRef<[u8]> for MUTF8String {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for MUTF8String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_mutf8_str(), f)
    }
}

impl fmt::Display for MUTF8String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_mutf8_str(), f)
    }
}

impl TryFrom<&MUTF8Str> for MUTF8String {
    type Error = Error;

    #[inline]
    fn try_from(s: &MUTF8Str) -> Result<Self, Error> {
        Self::from_slice_checked(s.as_bytes())
    }
}

impl FromStr for MUTF8String {
    type Err = Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Error> {
        Self::try_from(s)
    }
}

impl TryFrom<&str> for MUTF8String {
    type Error = Error;

    #[inline]
    fn try_from(s: &str) -> Result<Self, Error> {
        let bytes = s.as_bytes();
        if bytes.len() < 64 && is_strict_mutf8_ascii_under_64(bytes) {
            return Ok(Self(bytes.to_vec()));
        }
        if !simd_cesu8::mutf8::needs_encoded(s) {
            return Self::from_slice_checked(bytes);
        }
        Self::from_encoded(encode_mutf8_slow(s))
    }
}

impl TryFrom<&String> for MUTF8String {
    type Error = Error;

    #[inline]
    fn try_from(s: &String) -> Result<Self, Error> {
        Self::try_from(s.as_str())
    }
}

impl TryFrom<String> for MUTF8String {
    type Error = Error;

    #[inline]
    fn try_from(s: String) -> Result<Self, Error> {
        let bytes = s.as_bytes();
        if bytes.len() < 64 && is_strict_mutf8_ascii_under_64(bytes) {
            return Ok(Self(s.into_bytes()));
        }
        if !simd_cesu8::mutf8::needs_encoded(&s) {
            return Self::from_vec_checked(s.into_bytes());
        }
        Self::from_encoded(encode_mutf8_slow(&s))
    }
}

impl TryFrom<Cow<'_, str>> for MUTF8String {
    type Error = Error;

    #[inline]
    fn try_from(s: Cow<'_, str>) -> Result<Self, Error> {
        match s {
            Cow::Borrowed(b) => Self::try_from(b),
            Cow::Owned(o) => Self::try_from(o),
        }
    }
}

/// Panicking conversion used by the infallible convenience APIs (`IntoNBT`, `ToOwned`).
#[inline(always)]
#[track_caller]
pub(crate) fn mutf8_or_panic<T>(value: T) -> MUTF8String
where
    MUTF8String: TryFrom<T, Error = Error>,
{
    match MUTF8String::try_from(value) {
        Ok(s) => s,
        Err(Error::LEN(len)) => mutf8_too_long(len),
        Err(_) => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_borrowed, read_owned, BigEndian, Compound};

    fn assert_decode_parity(bytes: &[u8]) {
        let m = unsafe { MUTF8Str::from_mutf8_unchecked(bytes) };

        let got_lossy = m.decode_lossy();
        let exp_lossy = simd_cesu8::mutf8::decode_lossy(bytes);
        assert_eq!(got_lossy, exp_lossy, "decode_lossy mismatch for {bytes:?}");
        assert_eq!(
            matches!(got_lossy, Cow::Borrowed(_)),
            matches!(exp_lossy, Cow::Borrowed(_)),
            "decode_lossy Cow variant mismatch for {bytes:?}"
        );
        assert_eq!(m.to_utf8_string(), exp_lossy.as_ref());

        let got_decode = m.decode();
        let exp_decode = simd_cesu8::mutf8::decode(bytes);
        assert_eq!(got_decode, exp_decode, "decode mismatch for {bytes:?}");
        if let (Ok(g), Ok(e)) = (&got_decode, &exp_decode) {
            assert_eq!(
                matches!(g, Cow::Borrowed(_)),
                matches!(e, Cow::Borrowed(_)),
                "decode Cow variant mismatch for {bytes:?}"
            );
        }

        let got_strict = m.decode_strict();
        let exp_strict = simd_cesu8::mutf8::decode_strict(bytes);
        assert_eq!(
            got_strict, exp_strict,
            "decode_strict mismatch for {bytes:?}"
        );
        if let (Ok(g), Ok(e)) = (&got_strict, &exp_strict) {
            assert_eq!(
                matches!(g, Cow::Borrowed(_)),
                matches!(e, Cow::Borrowed(_)),
                "decode_strict Cow variant mismatch for {bytes:?}"
            );
        }

        let got_lossy_strict = m.decode_lossy_strict();
        let exp_lossy_strict = simd_cesu8::mutf8::decode_lossy_strict(bytes);
        assert_eq!(
            got_lossy_strict, exp_lossy_strict,
            "decode_lossy_strict mismatch for {bytes:?}"
        );
        assert_eq!(
            matches!(got_lossy_strict, Cow::Borrowed(_)),
            matches!(exp_lossy_strict, Cow::Borrowed(_)),
            "decode_lossy_strict Cow variant mismatch for {bytes:?}"
        );
    }

    fn assert_encode_parity(s: &str, expect_borrowed: bool) {
        let got = encode_mutf8(s);
        let exp = simd_cesu8::mutf8::encode(s);
        assert_eq!(
            got.as_ref(),
            exp.as_ref(),
            "encode_mutf8 mismatch for {s:?}"
        );
        assert_eq!(
            matches!(got, Cow::Borrowed(_)),
            expect_borrowed,
            "encode_mutf8 Cow variant mismatch for {s:?}"
        );
        assert_eq!(
            matches!(exp, Cow::Borrowed(_)),
            expect_borrowed,
            "simd_cesu8 Borrowed expectation mismatch for {s:?}"
        );
    }

    #[test]
    fn test_mutf8_decode_fast_paths_and_strict_null_parity() {
        for len in 0..=66 {
            let ascii: Vec<u8> = (0..len).map(|i| b'a' + (i % 26) as u8).collect();
            assert_decode_parity(&ascii);
            let s = std::str::from_utf8(&ascii).unwrap();
            assert_encode_parity(s, true);
            for pos in 0..len {
                let mut with_nul = ascii.clone();
                with_nul[pos] = 0x00;
                assert_decode_parity(&with_nul);
                assert_encode_parity(std::str::from_utf8(&with_nul).unwrap(), false);

                let mut with_bad = ascii.clone();
                with_bad[pos] = 0x80;
                assert_decode_parity(&with_bad);
            }
        }
        assert_decode_parity(&[b'z'; 180]);
        assert_decode_parity("§6Aspect of the End §7[+100]".as_bytes());

        for pos in 0..=18 {
            let mut buf = vec![b'x'; 19];
            buf[pos] = 0x00;
            let m = unsafe { MUTF8Str::from_mutf8_unchecked(&buf) };
            assert!(matches!(m.decode_lossy(), Cow::Borrowed(_)));
            assert!(matches!(m.decode(), Ok(Cow::Borrowed(_))));
            assert!(m.decode_strict().is_err());
            assert_decode_parity(&buf);

            buf[pos] = 0x80;
            assert_decode_parity(&buf);

            buf[pos] = 0xFF;
            assert_decode_parity(&buf);
        }

        assert_decode_parity(b"a\xC0\x80b");
        assert_decode_parity("é".as_bytes());
        assert_decode_parity("中文测试字符串".as_bytes());
        assert_decode_parity("\u{FFFF}".as_bytes());
        assert_decode_parity(&simd_cesu8::mutf8::encode("hello\0🎉world"));
        assert_decode_parity("🎉".as_bytes());
    }

    #[test]
    fn test_mutf8_encode_fast_paths_and_boundaries() {
        for s in [
            "",
            "\x01",
            "\x7F",
            "Data",
            "0123456789abcdef",
            "0123456789abcdef_long_ascii_key_name",
            "é",
            "中文",
            "\u{FFFF}",
            "0123456789abc\u{FFFF}defgh",
        ] {
            assert_encode_parity(s, true);
        }

        for s in [
            "\0",
            "abc\0def",
            "0123456789abcdef\0tail",
            "\u{10000}",
            "🎉",
            "\u{10FFFF}",
            "0123456789abcdef🎉tail",
        ] {
            assert_encode_parity(s, false);
        }
    }

    #[test]
    fn test_mutf8_string_zero_copy_and_compound_roundtrip() {
        let s = String::from("allocated_ascii_and_中文_\u{FFFF}");
        let orig_ptr = s.as_ptr();
        let own = MUTF8String::try_from(s).unwrap();
        assert_eq!(own.as_bytes().as_ptr(), orig_ptr);
        assert_eq!(
            &*own.decode_strict().unwrap(),
            "allocated_ascii_and_中文_\u{FFFF}"
        );

        let mut s_mutf8 = MUTF8String::try_from(String::from("null\0emoji🎉")).unwrap();
        assert_eq!(
            s_mutf8.as_bytes(),
            &*simd_cesu8::mutf8::encode("null\0emoji🎉")
        );
        assert_eq!(&*s_mutf8.decode_strict().unwrap(), "null\0emoji🎉");
        s_mutf8.push_str("_tail\0🚀");
        assert_eq!(&*s_mutf8.decode_strict().unwrap(), "null\0emoji🎉_tail\0🚀");

        let mut root = Compound::<BigEndian>::new();
        root.insert("ascii_key", "ascii_val");
        root.insert("key\0with\0null", "val\0with\0null");
        root.insert("emoji🎉key_long_enough_for_16b", String::from("emoji🎉val"));

        assert_eq!(
            &*root
                .get_string("ascii_key")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "ascii_val"
        );
        assert_eq!(
            &*root
                .get_string("key\0with\0null")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null"
        );
        assert_eq!(
            &*root
                .get_string("emoji🎉key_long_enough_for_16b")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "emoji🎉val"
        );

        let bytes = root.write_to_vec::<BigEndian>();
        let doc = read_borrowed::<BigEndian>(&bytes).unwrap();
        let doc_root = doc.root();
        let doc_compound = doc_root.as_compound().unwrap();
        assert!(doc_compound.get("ascii_key").is_some());
        assert_eq!(
            &*doc_root
                .at("key\0with\0null")
                .as_string()
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null"
        );
        assert_eq!(
            &*doc_root
                .at("emoji🎉key_long_enough_for_16b")
                .as_string()
                .unwrap()
                .decode_lossy(),
            "emoji🎉val"
        );

        let mut owned_doc = read_owned::<BigEndian, BigEndian>(&bytes).unwrap();
        let owned_root = owned_doc.as_compound_mut().unwrap();
        if let Some(mut s_mut) = owned_root.get_string_mut("key\0with\0null") {
            assert_eq!(&*s_mut.decode_strict().unwrap(), "val\0with\0null");
            s_mut.push_str("!");
        }
        assert_eq!(
            &*owned_root
                .get_string("key\0with\0null")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null!"
        );
        assert!(owned_root.remove("key\0with\0null").is_some());
        assert!(owned_root.get("key\0with\0null").is_none());
    }
}
