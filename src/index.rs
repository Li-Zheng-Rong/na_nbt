use std::borrow::Cow;

use crate::{with_mutf8_key, MUTF8Str, MUTF8String};

mod private {
    use std::borrow::Cow;

    use crate::{MUTF8Str, MUTF8String};

    pub trait SealedKey {}
    impl SealedKey for str {}
    impl SealedKey for String {}
    impl SealedKey for Cow<'_, str> {}
    impl SealedKey for MUTF8Str {}
    impl SealedKey for MUTF8String {}
    impl SealedKey for [u8] {}
    impl<const N: usize> SealedKey for [u8; N] {}
    impl<T: ?Sized + SealedKey> SealedKey for &T {}

    pub trait SealedIndex {}
    impl SealedIndex for usize {}
    impl SealedIndex for str {}
    impl SealedIndex for String {}
    impl SealedIndex for Cow<'_, str> {}
    impl SealedIndex for MUTF8Str {}
    impl SealedIndex for MUTF8String {}
    impl<T: ?Sized + SealedIndex> SealedIndex for &T {}
}

/// Key type accepted by [`Compound`](crate::Compound) and [`CompoundRef`](crate::CompoundRef)
/// lookup, insertion, and removal methods (`&str`, `String`, `Cow<str>`, `&MUTF8Str`,
/// `MUTF8String`, or raw MUTF-8 `&[u8]`).
pub trait AsKey: private::SealedKey {
    #[doc(hidden)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R;
}

impl AsKey for str {
    #[inline(always)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R {
        with_mutf8_key(self, f)
    }
}

impl AsKey for String {
    #[inline(always)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R {
        with_mutf8_key(self.as_str(), f)
    }
}

impl AsKey for Cow<'_, str> {
    #[inline(always)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R {
        with_mutf8_key(self.as_ref(), f)
    }
}

impl AsKey for MUTF8Str {
    #[inline(always)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R {
        f(self)
    }
}

impl AsKey for MUTF8String {
    #[inline(always)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R {
        f(self.as_mutf8_str())
    }
}

impl AsKey for [u8] {
    #[inline(always)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R {
        f(unsafe { MUTF8Str::from_mutf8_unchecked(self) })
    }
}

impl<const N: usize> AsKey for [u8; N] {
    #[inline(always)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R {
        f(unsafe { MUTF8Str::from_mutf8_unchecked(self.as_slice()) })
    }
}

impl<T: ?Sized + AsKey> AsKey for &T {
    #[inline(always)]
    fn with_mutf8<R>(&self, f: impl FnOnce(&MUTF8Str) -> R) -> R {
        (**self).with_mutf8(f)
    }
}

/// Path index into an NBT [`Compound`](crate::Compound) (by string/MUTF-8 key) or
/// [`List`](crate::List) (by `usize` element offset).
pub trait Index: private::SealedIndex {
    #[doc(hidden)]
    fn dispatch<V, R>(
        &self,
        value: V,
        on_idx: impl FnOnce(V, usize) -> R,
        on_key: impl FnOnce(V, &MUTF8Str) -> R,
    ) -> R;
}

impl Index for usize {
    #[inline(always)]
    fn dispatch<V, R>(
        &self,
        value: V,
        on_idx: impl FnOnce(V, usize) -> R,
        _on_key: impl FnOnce(V, &MUTF8Str) -> R,
    ) -> R {
        on_idx(value, *self)
    }
}

macro_rules! impl_key_index {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Index for $ty {
                #[inline(always)]
                fn dispatch<V, R>(
                    &self,
                    value: V,
                    _on_idx: impl FnOnce(V, usize) -> R,
                    on_key: impl FnOnce(V, &MUTF8Str) -> R,
                ) -> R {
                    self.with_mutf8(|k| on_key(value, k))
                }
            }
        )*
    };
}

impl_key_index!(str, String, Cow<'_, str>, MUTF8Str, MUTF8String);

impl<T: ?Sized + Index> Index for &T {
    #[inline(always)]
    fn dispatch<V, R>(
        &self,
        value: V,
        on_idx: impl FnOnce(V, usize) -> R,
        on_key: impl FnOnce(V, &MUTF8Str) -> R,
    ) -> R {
        (**self).dispatch(value, on_idx, on_key)
    }
}
