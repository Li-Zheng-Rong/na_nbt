mod array;
mod compound;
mod config;
mod document;
mod list;
mod mark;
mod read;
pub(crate) mod size;
mod string;
mod typed_list;
mod value;
mod write;

use zerocopy::byteorder;

use crate::{ByteOrder, ConfigRef, TagID, cold_path};

#[inline]
unsafe fn read_root<'s, O: ByteOrder, D: document::Document>(
    source: *const u8,
    mark: *const mark::Mark,
    doc: &D,
) -> value::ReadonlyValue<'s, O, D> {
    unsafe {
        let root_tag = *source.cast::<TagID>();
        if root_tag == TagID::End {
            cold_path();
            return value::ReadonlyValue::End(());
        }
        let name_len = byteorder::U16::<O>::from_bytes(*source.add(1).cast()).get() as usize;
        <config::ImmutableConfig<O, D> as ConfigRef>::read_value(
            root_tag,
            (source.add(3 + name_len), mark, doc),
        )
    }
}

mod borrowed {
    use crate::{ByteOrder, Result, immutable::document::Document};

    use super::*;

    pub type BorrowedValue<'s, O> = value::ReadonlyValue<'s, O, ()>;

    impl Document for () {
        #[inline]
        fn empty() -> Self {}
    }

    pub fn read_borrowed<'s, O: ByteOrder>(source: &'s [u8]) -> Result<BorrowedDocument<'s, O>> {
        unsafe {
            read::read_unsafe::<O, _>(source.as_ptr(), source.len(), |mark| BorrowedDocument {
                mark,
                source: source.as_ptr(),
                _marker: core::marker::PhantomData::<(&'s (), O)>,
            })
        }
    }

    pub struct BorrowedDocument<'s, O: ByteOrder> {
        mark: Vec<mark::Mark>,
        source: *const u8,
        _marker: core::marker::PhantomData<(&'s (), O)>,
    }

    impl<'s, O: ByteOrder> BorrowedDocument<'s, O> {
        #[inline]
        pub fn root<'doc>(&'doc self) -> BorrowedValue<'doc, O> {
            unsafe { read_root(self.source, self.mark.as_ptr(), &()) }
        }
    }

    unsafe impl<'s, O: ByteOrder> Send for BorrowedDocument<'s, O> {}
    unsafe impl<'s, O: ByteOrder> Sync for BorrowedDocument<'s, O> {}
}

pub use borrowed::read_borrowed;

#[cfg(feature = "shared")]
mod shared {
    use std::sync::Arc;

    use bytes::Bytes;

    use crate::{ByteOrder, Result, immutable::document::Document};

    use super::*;

    pub type SharedValue<O> = value::ReadonlyValue<'static, O, Arc<SharedDocument>>;

    impl Document for Arc<SharedDocument> {
        fn empty() -> Self {
            Arc::new(SharedDocument {
                mark: Vec::new(),
                source: Bytes::new(),
            })
        }
    }

    pub fn read_shared<O: ByteOrder>(source: Bytes) -> Result<SharedValue<O>> {
        Ok(unsafe {
            read::read_unsafe::<O, _>(source.as_ptr(), source.len(), |mark| {
                Arc::new(SharedDocument { mark, source })
            })?
            .root()
        })
    }

    /// A parsed NBT document with shared ownership.
    ///
    /// This type holds the source data and parsing metadata for [`SharedValue`]s.
    /// It is managed through `Arc` and should not typically be used directly.
    pub struct SharedDocument {
        mark: Vec<mark::Mark>,
        source: Bytes,
    }

    impl SharedDocument {
        /// Returns the root value of the document.
        #[inline]
        pub fn root<O: ByteOrder>(self: Arc<Self>) -> SharedValue<O> {
            unsafe { read_root(self.source.as_ptr(), self.mark.as_ptr(), &self) }
        }
    }
}

#[cfg(feature = "shared")]
pub use shared::read_shared;
