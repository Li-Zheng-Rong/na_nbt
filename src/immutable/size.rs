use std::hint::assert_unchecked;

use zerocopy::byteorder;

use crate::{ByteOrder, TagID, immutable::mark::Mark};

const TAG_ELEM_SIZES: [u8; 13] = [0, 1, 2, 4, 8, 4, 8, 1, 0, 0, 0, 4, 8];

#[inline(always)]
pub(crate) const unsafe fn tag_elem_size(tag_id: u8) -> usize {
    unsafe { assert_unchecked(tag_id < 13) };
    TAG_ELEM_SIZES[tag_id as usize] as usize
}

/// Returns `(data_byte_advance, mark_advance)` for `tag_id` at `data`.
///
/// # Safety
///
/// `data` must point to a valid payload for `tag_id`, and `mark` must point to
/// the corresponding [`Mark`] when `tag_id` is `List` or `Compound`.
#[inline(always)]
pub const unsafe fn immutable_tag_size<O: ByteOrder>(
    tag_id: TagID,
    data: *const u8,
    mark: *const Mark,
) -> (usize, usize) {
    match tag_id {
        TagID::End
        | TagID::Byte
        | TagID::Short
        | TagID::Int
        | TagID::Long
        | TagID::Float
        | TagID::Double => (unsafe { tag_elem_size(tag_id as u8) }, 0),
        TagID::String => (
            2 + byteorder::U16::<O>::from_bytes(unsafe { *data.cast() }).get() as usize,
            0,
        ),
        TagID::List | TagID::Compound => {
            let m = unsafe { *mark };
            (m.byte_len as usize, m.flat_next_mark as usize)
        }
        TagID::ByteArray | TagID::IntArray | TagID::LongArray => (
            4 + byteorder::U32::<O>::from_bytes(unsafe { *data.cast() }).get() as usize
                * unsafe { tag_elem_size(tag_id as u8) },
            0,
        ),
    }
}
