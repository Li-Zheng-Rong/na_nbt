#[derive(Copy, Clone, Default, Debug)]
#[repr(C)]
pub struct Mark {
    /// Byte length of this List or Compound payload.
    pub byte_len: u32,
    /// Distance to the next sibling mark in the flat array (1 + nested marks).
    pub flat_next_mark: u32,
}
