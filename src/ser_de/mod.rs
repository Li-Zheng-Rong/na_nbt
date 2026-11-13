mod de;
mod dom;
pub mod marker;
mod ser;
pub mod tag_probe;

pub(crate) const EMPTY_LIST_MARKERS: [&str; 13] = [
    "na_nbt:empty_list_0",
    "na_nbt:empty_list_1",
    "na_nbt:empty_list_2",
    "na_nbt:empty_list_3",
    "na_nbt:empty_list_4",
    "na_nbt:empty_list_5",
    "na_nbt:empty_list_6",
    "na_nbt:empty_list_7",
    "na_nbt:empty_list_8",
    "na_nbt:empty_list_9",
    "na_nbt:empty_list_10",
    "na_nbt:empty_list_11",
    "na_nbt:empty_list_12",
];

#[inline]
pub(crate) fn parse_empty_list_marker(name: &str) -> Option<u8> {
    let suffix = name.strip_prefix("na_nbt:empty_list_")?;
    let id: u8 = suffix.parse().ok()?;
    (id <= 12).then_some(id)
}

pub use de::{
    from_reader, from_reader_be, from_reader_le, from_reader_nameless, from_reader_nameless_be,
    from_reader_nameless_le, from_slice, from_slice_be, from_slice_le, from_slice_nameless,
    from_slice_nameless_be, from_slice_nameless_le, Deserializer,
};
pub use marker::{byte_array, int_array, list, long_array};
pub use ser::{
    to_vec, to_vec_be, to_vec_le, to_vec_named, to_vec_named_be, to_vec_named_le, to_vec_nameless,
    to_vec_nameless_be, to_vec_nameless_le, to_writer, to_writer_be, to_writer_le, to_writer_named,
    to_writer_named_be, to_writer_named_le, to_writer_nameless, to_writer_nameless_be,
    to_writer_nameless_le, MapSerializer, SeqSerializer, Serializer,
};
pub use tag_probe::tag_of;
