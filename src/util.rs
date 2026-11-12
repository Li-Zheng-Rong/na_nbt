use std::{
    borrow::{Borrow, Cow},
    mem,
};

use simd_cesu8::DecodingError;

use crate::StringOwn;

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

pub(crate) static EMPTY_LIST: [u8; 5] = [0; 5];
pub(crate) static EMPTY_COMPOUND: [u8; 1] = [0];

/// A MUTF-8 (Modified UTF-8) encoded string slice.
///
/// MUTF-8 is an encoding used by Java.
///
/// This type is used for NBT string values and keys, which are encoded in MUTF-8.
#[repr(transparent)]
pub struct MUTF8Str([u8]);

impl Default for &MUTF8Str {
    #[inline]
    fn default() -> Self {
        unsafe { MUTF8Str::from_mutf8_unchecked(&[]) }
    }
}

impl Borrow<MUTF8Str> for StringOwn {
    fn borrow(&self) -> &MUTF8Str {
        self.as_mutf8_str()
    }
}

impl ToOwned for MUTF8Str {
    type Owned = StringOwn;

    fn to_owned(&self) -> Self::Owned {
        StringOwn::from(self)
    }
}

impl MUTF8Str {
    /// Creates a `&MUTF8Str` from a byte slice without validation.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `bytes` contains valid MUTF-8 encoded data.
    ///
    /// MUTF-8 (Modified UTF-8) is an encoding used by Java.
    ///
    /// # Example
    ///
    /// ```
    /// use na_nbt::MUTF8Str;
    ///
    /// // Valid MUTF-8: standard ASCII
    /// let valid = b"hello";
    /// let s = unsafe { MUTF8Str::from_mutf8_unchecked(valid) };
    /// ```
    #[inline]
    pub const unsafe fn from_mutf8_unchecked(bytes: &[u8]) -> &Self {
        unsafe { mem::transmute(bytes) }
    }

    #[inline]
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// see [`simd_cesu8::mutf8::decode_lossy`]
    #[inline]
    pub fn decode_lossy(&self) -> Cow<'_, str> {
        decode_mutf8_lossy(&self.0)
    }

    /// see [`simd_cesu8::mutf8::decode_strict`]
    #[inline]
    pub fn decode_strict(&self) -> Result<Cow<'_, str>, DecodingError> {
        if self.0.len() < 64 && is_strict_mutf8_ascii_under_64(&self.0) {
            // SAFETY: Non-null ASCII bytes (0x01..=0x7F) are valid strict MUTF-8 and UTF-8.
            Ok(Cow::Borrowed(unsafe {
                std::str::from_utf8_unchecked(&self.0)
            }))
        } else {
            simd_cesu8::mutf8::decode_strict(&self.0)
        }
    }

    /// see [`simd_cesu8::mutf8::decode`]
    #[inline]
    pub fn decode(&self) -> Result<Cow<'_, str>, DecodingError> {
        if self.0.len() < 64
            && (if self.0.len() <= 16 {
                is_ascii_short_16(&self.0)
            } else {
                self.0[0] < 0x80 && self.0.is_ascii()
            })
        {
            // SAFETY: ASCII bytes (0x00..=0x7F) are always valid UTF-8.
            Ok(Cow::Borrowed(unsafe {
                std::str::from_utf8_unchecked(&self.0)
            }))
        } else {
            simd_cesu8::mutf8::decode(&self.0)
        }
    }

    /// see [`simd_cesu8::mutf8::decode_lossy_strict`]
    #[inline]
    pub fn decode_lossy_strict(&self) -> Cow<'_, str> {
        if self.0.len() < 64 && is_strict_mutf8_ascii_under_64(&self.0) {
            // SAFETY: Non-null ASCII bytes (0x01..=0x7F) are valid strict MUTF-8 and UTF-8.
            Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(&self.0) })
        } else {
            simd_cesu8::mutf8::decode_lossy_strict(&self.0)
        }
    }

    /// Calls [`Self::decode_lossy`] and converts the result to a [`String`].
    #[inline]
    pub fn to_utf8_string(&self) -> String {
        if self.0.len() < 64 {
            if is_valid_utf8_under_64(&self.0) {
                // SAFETY: Validated as ASCII or UTF-8 above.
                unsafe { String::from_utf8_unchecked(self.0.to_vec()) }
            } else {
                decode_mutf8_lossy_owned_slow(&self.0)
            }
        } else {
            simd_cesu8::mutf8::decode_lossy(&self.0).into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{MUTF8Str, decode_mutf8_lossy, encode_mutf8};
    use crate::{
        BigEndian, CompoundMut, CompoundOwn, CompoundRef, StringOwn, ValueMut, ValueRef, Writable,
        read_borrowed, read_owned, tag,
    };

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
        assert_eq!(decode_mutf8_lossy(bytes), exp_lossy);
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
        assert_decode_parity(&vec![b'z'; 180]);
        assert_decode_parity("§6Aspect of the End §7[+100]".as_bytes());

        // Raw 0x00 at every SWAR boundary position: valid in non-strict decode/decode_lossy,
        // invalid in decode_strict / decode_lossy_strict!
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

        // Valid MUTF-8 null (0xC0 0x80), 2-byte, 3-byte BMP (\u{FFFF}), 6-byte surrogate, and raw 4-byte UTF-8
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
    fn test_non_serde_string_own_zero_copy_and_compound_mutf8() {
        // StringOwn::from(String) reuses heap buffer when no MUTF-8 re-encoding is needed
        let s = String::from("allocated_ascii_and_中文_\u{FFFF}");
        let orig_ptr = s.as_ptr();
        let own = StringOwn::from(s);
        assert_eq!(own.as_ptr(), orig_ptr);
        assert_eq!(&*own.decode_strict().unwrap(), "allocated_ascii_and_中文_\u{FFFF}");

        // StringOwn::from(String) re-encodes when '\0' or 4-byte UTF-8 is present
        let mut s_mutf8 = StringOwn::from(String::from("null\0emoji🎉"));
        assert_eq!(s_mutf8.as_bytes(), &*simd_cesu8::mutf8::encode("null\0emoji🎉"));
        assert_eq!(&*s_mutf8.decode_strict().unwrap(), "null\0emoji🎉");
        s_mutf8.push_str("_tail\0🚀");
        assert_eq!(&*s_mutf8.decode_strict().unwrap(), "null\0emoji🎉_tail\0🚀");

        // CompoundOwn + read_borrowed + read_owned round-trip with ASCII, CJK, null, and emoji keys/values
        let mut root = CompoundOwn::<BigEndian>::default();
        root.insert("ascii_key", "ascii_val");
        root.insert("key\0with\0null", "val\0with\0null");
        root.insert("emoji🎉key_long_enough_for_16b", String::from("emoji🎉val"));

        assert_eq!(
            &*root.get_::<tag::String>("ascii_key").unwrap().decode_strict().unwrap(),
            "ascii_val"
        );
        assert_eq!(
            &*root
                .get_::<tag::String>("key\0with\0null")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null"
        );
        assert_eq!(
            &*root
                .get_::<tag::String>("emoji🎉key_long_enough_for_16b")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "emoji🎉val"
        );

        let bytes = root.write_to_vec::<BigEndian>();
        let doc = read_borrowed::<BigEndian>(&bytes).unwrap();
        let doc_root = doc.root();
        let doc_compound = doc_root.ref_::<tag::Compound>().unwrap();
        assert!(doc_compound.get("ascii_key").is_some());
        assert_eq!(
            &*doc_root
                .get_::<tag::String>("key\0with\0null")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null"
        );
        assert_eq!(
            &*doc_root
                .get_::<tag::String>("emoji🎉key_long_enough_for_16b")
                .unwrap()
                .decode_lossy(),
            "emoji🎉val"
        );

        let mut owned_doc = read_owned::<BigEndian, BigEndian>(&bytes).unwrap();
        let mut owned_root = owned_doc.to_mut().into_::<tag::Compound>().unwrap();
        if let Some(mut s_mut) = owned_root.get_mut_::<tag::String>("key\0with\0null") {
            assert_eq!(&*s_mut.decode_strict().unwrap(), "val\0with\0null");
            s_mut.push_str("!");
        }
        assert_eq!(
            &*owned_root
                .get_::<tag::String>("key\0with\0null")
                .unwrap()
                .decode_strict()
                .unwrap(),
            "val\0with\0null!"
        );
        assert!(owned_root.remove("key\0with\0null").is_some());
        assert!(owned_root.get("key\0with\0null").is_none());
    }
}
