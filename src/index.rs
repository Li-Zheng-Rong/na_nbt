mod private {
    pub trait Sealed {}
    impl Sealed for usize {}
    impl Sealed for str {}
    impl Sealed for String {}
    impl<T: ?Sized + Sealed> Sealed for &T {}
}

pub trait Index: private::Sealed {
    #[doc(hidden)]
    fn dispatch<V, R>(
        &self,
        value: V,
        on_idx: impl FnOnce(V, usize) -> R,
        on_key: impl FnOnce(V, &str) -> R,
    ) -> R;
}

impl Index for usize {
    #[inline(always)]
    fn dispatch<V, R>(
        &self,
        value: V,
        on_idx: impl FnOnce(V, usize) -> R,
        _on_key: impl FnOnce(V, &str) -> R,
    ) -> R {
        on_idx(value, *self)
    }
}

impl Index for str {
    #[inline(always)]
    fn dispatch<V, R>(
        &self,
        value: V,
        _on_idx: impl FnOnce(V, usize) -> R,
        on_key: impl FnOnce(V, &str) -> R,
    ) -> R {
        on_key(value, self)
    }
}

impl Index for String {
    #[inline(always)]
    fn dispatch<V, R>(
        &self,
        value: V,
        _on_idx: impl FnOnce(V, usize) -> R,
        on_key: impl FnOnce(V, &str) -> R,
    ) -> R {
        on_key(value, self.as_str())
    }
}

impl<T: ?Sized + Index> Index for &T {
    #[inline(always)]
    fn dispatch<V, R>(
        &self,
        value: V,
        on_idx: impl FnOnce(V, usize) -> R,
        on_key: impl FnOnce(V, &str) -> R,
    ) -> R {
        (**self).dispatch(value, on_idx, on_key)
    }
}
