use std::{
    borrow::{Borrow, Cow},
    fmt, mem,
    ops::Deref,
};

use simd_cesu8::DecodingError;

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
fn with_mutf8_key_slow<R>(key: &str, f: impl FnOnce(&MUTF8Str) -> R) -> R {
    let encoded = simd_cesu8::mutf8::encode(key);
    f(unsafe { MUTF8Str::from_mutf8_unchecked(&encoded) })
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
    (((a | a.wrapping_sub(0x0101_0101_0101_0101))
        | (b | b.wrapping_sub(0x0101_0101_0101_0101)))
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
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let word = u64::from_ne_bytes(chunk.try_into().unwrap());
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
pub(crate) fn encode_mutf8(value: &str) -> Cow<'_, [u8]> {
    let bytes = value.as_bytes();
    if (bytes.len() <= 16 && is_non_null_ascii_short_16(bytes))
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
    if (bytes.len() < 64 && is_strict_mutf8_ascii_under_64(bytes))
        || !simd_cesu8::mutf8::needs_encoded(key)
    {
        f(unsafe { MUTF8Str::from_mutf8_unchecked(bytes) })
    } else {
        with_mutf8_key_slow(key, f)
    }
}

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

/// A MUTF-8 (Modified UTF-8) encoded string slice.
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

impl ToOwned for MUTF8Str {
    type Owned = MUTF8String;

    #[inline]
    fn to_owned(&self) -> Self::Owned {
        MUTF8String::from(self)
    }
}

impl MUTF8Str {
    /// Creates a `&MUTF8Str` from a byte slice without validation.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `bytes` contains valid MUTF-8 encoded data.
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

    #[inline]
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

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

/// An owned, growable MUTF-8 encoded string.
#[repr(transparent)]
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MUTF8String(pub(crate) Vec<u8>);

impl MUTF8String {
    #[inline]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }

    /// Creates an owned `MUTF8String` from raw MUTF-8 bytes without validation.
    ///
    /// # Safety
    ///
    /// `bytes` must contain valid MUTF-8 encoded data.
    #[inline]
    pub const unsafe fn from_mutf8_unchecked(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    #[inline(always)]
    pub(crate) fn from_raw_slice(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    #[inline]
    pub fn as_mutf8_str(&self) -> &MUTF8Str {
        unsafe { MUTF8Str::from_mutf8_unchecked(&self.0) }
    }

    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    #[inline]
    pub fn clear(&mut self) {
        self.0.clear();
    }

    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    #[inline]
    pub fn push(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        self.push_str(ch.encode_utf8(&mut buf));
    }

    #[inline]
    pub fn push_str(&mut self, s: &str) {
        self.0.extend_from_slice(&encode_mutf8(s));
    }

    #[inline]
    pub fn push_mutf8(&mut self, s: &MUTF8Str) {
        self.0.extend_from_slice(s.as_bytes());
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

impl From<&MUTF8Str> for MUTF8String {
    #[inline]
    fn from(s: &MUTF8Str) -> Self {
        Self(s.as_bytes().to_vec())
    }
}

impl From<&str> for MUTF8String {
    #[inline]
    fn from(s: &str) -> Self {
        Self(encode_mutf8(s).into_owned())
    }
}

impl From<String> for MUTF8String {
    #[inline]
    fn from(s: String) -> Self {
        let bytes = s.as_bytes();
        if (bytes.len() <= 16 && is_non_null_ascii_short_16(bytes))
            || !simd_cesu8::mutf8::needs_encoded(&s)
        {
            Self(s.into_bytes())
        } else {
            Self(encode_mutf8_slow(&s).into_owned())
        }
    }
}

impl From<Cow<'_, str>> for MUTF8String {
    #[inline]
    fn from(s: Cow<'_, str>) -> Self {
        match s {
            Cow::Borrowed(b) => Self::from(b),
            Cow::Owned(o) => Self::from(o),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BigEndian, CompoundOwn, Writable, read_borrowed, read_owned, tag};

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
        assert_eq!(got.as_ref(), exp.as_ref(), "encode_mutf8 mismatch for {s:?}");
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
        let own = MUTF8String::from(s);
        assert_eq!(own.as_bytes().as_ptr(), orig_ptr);
        assert_eq!(
            &*own.decode_strict().unwrap(),
            "allocated_ascii_and_中文_\u{FFFF}"
        );

        let mut s_mutf8 = MUTF8String::from(String::from("null\0emoji🎉"));
        assert_eq!(
            s_mutf8.as_bytes(),
            &*simd_cesu8::mutf8::encode("null\0emoji🎉")
        );
        assert_eq!(&*s_mutf8.decode_strict().unwrap(), "null\0emoji🎉");
        s_mutf8.push_str("_tail\0🚀");
        assert_eq!(
            &*s_mutf8.decode_strict().unwrap(),
            "null\0emoji🎉_tail\0🚀"
        );

        let mut root = CompoundOwn::<BigEndian>::new();
        root.insert("ascii_key", "ascii_val");
        root.insert("key\0with\0null", "val\0with\0null");
        root.insert("emoji🎉key_long_enough_for_16b", String::from("emoji🎉val"));

        assert_eq!(
            &*root
                .get_as::<tag::String>("ascii_key")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "ascii_val"
        );
        assert_eq!(
            &*root
                .get_as::<tag::String>("key\0with\0null")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null"
        );
        assert_eq!(
            &*root
                .get_as::<tag::String>("emoji🎉key_long_enough_for_16b")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "emoji🎉val"
        );

        let bytes = root.write_to_vec::<BigEndian>();
        let doc = read_borrowed::<BigEndian>(&bytes).unwrap();
        let doc_root = doc.root();
        let doc_compound = doc_root.as_::<tag::Compound>().unwrap();
        assert!(doc_compound.get("ascii_key").is_some());
        assert_eq!(
            &*doc_root
                .get_as::<tag::String>("key\0with\0null")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null"
        );
        assert_eq!(
            &*doc_root
                .get_as::<tag::String>("emoji🎉key_long_enough_for_16b")
                .unwrap()
                .decode_lossy(),
            "emoji🎉val"
        );

        let mut owned_doc = read_owned::<BigEndian, BigEndian>(&bytes).unwrap();
        let mut owned_root = owned_doc.as_mut_::<tag::Compound>().unwrap();
        if let Some(mut s_mut) = owned_root.get_mut_as::<tag::String>("key\0with\0null") {
            assert_eq!(&*s_mut.decode_strict().unwrap(), "val\0with\0null");
            s_mut.push_str("!");
        }
        assert_eq!(
            &*owned_root
                .get_as::<tag::String>("key\0with\0null")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null!"
        );
        assert!(owned_root.remove("key\0with\0null").is_some());
        assert!(owned_root.get("key\0with\0null").is_none());
    }
}
