use core::{
    mem::{ManuallyDrop, MaybeUninit},
    ptr,
};

use zerocopy::byteorder;

use crate::{cold_path, ByteOrder, Error, Result, TagID};

#[derive(Copy, Clone, Default, Debug)]
#[repr(C, align(8))]
pub(crate) struct Mark {
    /// Byte length of this `List` or `Compound` payload.
    pub byte_len: u32,
    /// Distance to the next sibling mark in the flat array (`1 + nested marks`).
    pub flat_next_mark: u32,
}

pub(crate) const INLINE_MARKS: usize = 8;

/// Inline-first structural index storing up to [`INLINE_MARKS`] container marks without heap
/// allocation, spilling seamlessly to a heap buffer for larger documents.
pub(crate) struct SmallMarks {
    heap: *mut Mark,
    len: usize,
    /// `0` when marks live in `inline[..len]`; `> 0` when `heap` owns an allocation of `cap` marks.
    cap: usize,
    inline: [MaybeUninit<Mark>; INLINE_MARKS],
}

unsafe impl Send for SmallMarks {}
unsafe impl Sync for SmallMarks {}

impl SmallMarks {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            heap: ptr::null_mut(),
            len: 0,
            cap: 0,
            inline: [const { MaybeUninit::uninit() }; INLINE_MARKS],
        }
    }

    #[inline(always)]
    pub(crate) fn as_ptr(&self) -> *const Mark {
        if self.cap == 0 {
            self.inline.as_ptr().cast::<Mark>()
        } else {
            self.heap.cast_const()
        }
    }
}

impl Clone for SmallMarks {
    fn clone(&self) -> Self {
        let len = self.len;
        if len <= INLINE_MARKS {
            let mut out = Self::new();
            out.len = len;
            unsafe {
                ptr::copy_nonoverlapping(
                    self.as_ptr().cast::<MaybeUninit<Mark>>(),
                    out.inline.as_mut_ptr(),
                    len,
                );
            }
            out
        } else {
            let mut v = Vec::<MaybeUninit<Mark>>::with_capacity(len);
            let new_ptr = v.as_mut_ptr().cast::<Mark>();
            let new_cap = v.capacity();
            let _ = ManuallyDrop::new(v);
            unsafe {
                ptr::copy_nonoverlapping(
                    self.heap.cast::<MaybeUninit<Mark>>(),
                    new_ptr.cast::<MaybeUninit<Mark>>(),
                    len,
                );
            }
            Self {
                heap: new_ptr,
                len,
                cap: new_cap,
                inline: [const { MaybeUninit::uninit() }; INLINE_MARKS],
            }
        }
    }
}

impl Drop for SmallMarks {
    #[inline]
    fn drop(&mut self) {
        if self.cap != 0 {
            unsafe {
                drop(Vec::<MaybeUninit<Mark>>::from_raw_parts(
                    self.heap.cast::<MaybeUninit<Mark>>(),
                    0,
                    self.cap,
                ));
            }
        }
    }
}

const INLINE_STACK_CAP: usize = 32;

#[derive(Copy, Clone, PartialEq, Eq)]
enum ParentKind {
    Compound,
    ListCompound,
    ListList,
}

#[derive(Copy, Clone)]
struct ParentFrame {
    start: *const u8,
    mark_idx: u32,
    remaining: u32,
    kind: ParentKind,
}

struct FrameStack {
    inline: [MaybeUninit<ParentFrame>; INLINE_STACK_CAP],
    overflow: Vec<ParentFrame>,
    len: usize,
}

impl FrameStack {
    #[inline(always)]
    fn new() -> Self {
        Self {
            inline: [const { MaybeUninit::uninit() }; INLINE_STACK_CAP],
            overflow: Vec::new(),
            len: 0,
        }
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[cold]
    #[inline(never)]
    fn push_slow(&mut self, frame: ParentFrame) {
        self.overflow.push(frame);
        self.len += 1;
    }

    #[inline(always)]
    fn push(&mut self, frame: ParentFrame) {
        if self.len < INLINE_STACK_CAP {
            unsafe { self.inline.get_unchecked_mut(self.len).write(frame) };
            self.len += 1;
        } else {
            self.push_slow(frame);
        }
    }

    #[inline(always)]
    fn last_mut(&mut self) -> &mut ParentFrame {
        debug_assert!(self.len > 0);
        if self.len <= INLINE_STACK_CAP {
            unsafe {
                self.inline
                    .get_unchecked_mut(self.len - 1)
                    .assume_init_mut()
            }
        } else {
            let idx = self.len - INLINE_STACK_CAP - 1;
            unsafe { self.overflow.get_unchecked_mut(idx) }
        }
    }

    #[inline(always)]
    fn pop(&mut self) -> ParentFrame {
        debug_assert!(self.len > 0);
        self.len -= 1;
        if self.len < INLINE_STACK_CAP {
            unsafe { self.inline.get_unchecked(self.len).assume_init_read() }
        } else {
            unsafe { self.overflow.pop().unwrap_unchecked() }
        }
    }
}

struct MarkBuf<'a> {
    ptr: *mut Mark,
    len: usize,
    cap: usize,
    marks: &'a mut SmallMarks,
}

impl<'a> MarkBuf<'a> {
    #[inline(always)]
    fn new(marks: &'a mut SmallMarks) -> Self {
        debug_assert_eq!(marks.cap, 0);
        let ptr = marks.inline.as_mut_ptr().cast::<Mark>();
        Self {
            ptr,
            len: 0,
            cap: INLINE_MARKS,
            marks,
        }
    }

    #[cold]
    #[inline(never)]
    fn reserve_slow(&mut self, additional: usize, rem_bytes: usize) {
        let required = self.len + additional;
        if self.marks.cap == 0 {
            let target_cap = (self.len + 1 + rem_bytes / 24)
                .max(INLINE_MARKS * 2)
                .max(required);
            let mut v = Vec::<MaybeUninit<Mark>>::with_capacity(target_cap);
            let new_ptr = v.as_mut_ptr().cast::<Mark>();
            let actual_cap = v.capacity();
            let _ = ManuallyDrop::new(v);
            unsafe {
                ptr::copy_nonoverlapping(
                    self.ptr.cast::<MaybeUninit<Mark>>(),
                    new_ptr.cast::<MaybeUninit<Mark>>(),
                    self.len,
                );
            }
            self.marks.heap = new_ptr;
            self.marks.cap = actual_cap;
            self.ptr = new_ptr;
            self.cap = actual_cap;
        } else {
            self.marks.cap = 0;
            let mut v = unsafe {
                Vec::<MaybeUninit<Mark>>::from_raw_parts(
                    self.ptr.cast::<MaybeUninit<Mark>>(),
                    self.len,
                    self.cap,
                )
            };
            v.reserve(additional);
            let new_ptr = v.as_mut_ptr().cast::<Mark>();
            let actual_cap = v.capacity();
            let _ = ManuallyDrop::new(v);
            self.marks.heap = new_ptr;
            self.marks.cap = actual_cap;
            self.ptr = new_ptr;
            self.cap = actual_cap;
        }
    }

    #[inline(always)]
    fn ensure_slots(&mut self, additional: usize, rem_bytes: usize) {
        if self.cap - self.len < additional {
            self.reserve_slow(additional, rem_bytes);
        }
    }

    #[inline(always)]
    fn alloc_slot(&mut self, rem_bytes: usize) -> u32 {
        let len = self.len;
        if len == self.cap {
            self.reserve_slow(1, rem_bytes);
        }
        self.len = len + 1;
        len as u32
    }

    #[inline(always)]
    fn push_leaf(&mut self, byte_len: u32, rem_bytes: usize) {
        let idx = self.alloc_slot(rem_bytes) as usize;
        unsafe {
            self.ptr.add(idx).write(Mark {
                byte_len,
                flat_next_mark: 1,
            });
        }
    }

    #[inline(always)]
    fn finish_slot(&mut self, idx: u32, byte_len: u32) {
        let idx = idx as usize;
        let flat_next_mark = (self.len - idx) as u32;
        unsafe {
            self.ptr.add(idx).write(Mark {
                byte_len,
                flat_next_mark,
            });
        }
    }

    #[inline(always)]
    fn commit(self) {
        self.marks.len = self.len;
    }
}

#[inline(always)]
unsafe fn skip_str<O: ByteOrder>(pos: *const u8, end_pos: *const u8) -> Result<*const u8> {
    let rem = (end_pos as usize) - (pos as usize);
    if rem < 2 {
        cold_path();
        return Err(Error::EOF);
    }
    let slen = unsafe { byteorder::U16::<O>::from_bytes(*pos.cast()).get() } as usize;
    if rem < 2 + slen {
        cold_path();
        return Err(Error::EOF);
    }
    Ok(unsafe { pos.add(2 + slen) })
}

#[inline(always)]
unsafe fn skip_array<O: ByteOrder>(
    pos: *const u8,
    end_pos: *const u8,
    elem_size: usize,
) -> Result<*const u8> {
    let rem = (end_pos as usize) - (pos as usize);
    if rem < 4 {
        cold_path();
        return Err(Error::EOF);
    }
    let alen = unsafe { byteorder::U32::<O>::from_bytes(*pos.cast()).get() } as usize;
    let Some(bytes) = alen.checked_mul(elem_size) else {
        cold_path();
        return Err(Error::EOF);
    };
    if rem - 4 < bytes {
        cold_path();
        return Err(Error::EOF);
    }
    Ok(unsafe { pos.add(4 + bytes) })
}

enum ListNext {
    Leaf(*const u8),
    Container { pos: *const u8, frame: ParentFrame },
}

#[inline(always)]
unsafe fn parse_list<O: ByteOrder>(
    list_start: *const u8,
    end_pos: *const u8,
    mark: &mut MarkBuf,
) -> Result<ListNext> {
    unsafe {
        let rem = (end_pos as usize) - (list_start as usize);
        if rem < 5 {
            cold_path();
            return Err(Error::EOF);
        }
        let elem_type = *list_start;
        let count = byteorder::U32::<O>::from_bytes(*list_start.add(1).cast()).get() as usize;
        let mut pos = list_start.add(5);
        let rem_after = rem - 5;

        match elem_type {
            0..=6 => {
                let Some(bytes) = count.checked_mul(TagID::elem_size_raw(elem_type)) else {
                    cold_path();
                    return Err(Error::EOF);
                };
                if rem_after < bytes {
                    cold_path();
                    return Err(Error::EOF);
                }
                mark.push_leaf((5 + bytes) as u32, rem_after - bytes);
                Ok(ListNext::Leaf(pos.add(bytes)))
            }
            7 | 11 | 12 => {
                let elem_size = TagID::elem_size_raw(elem_type);
                if count.checked_mul(4).is_none_or(|min| rem_after < min) {
                    cold_path();
                    return Err(Error::EOF);
                }
                for _ in 0..count {
                    pos = skip_array::<O>(pos, end_pos, elem_size)?;
                }
                mark.push_leaf(
                    ((pos as usize) - (list_start as usize)) as u32,
                    (end_pos as usize) - (pos as usize),
                );
                Ok(ListNext::Leaf(pos))
            }
            8 => {
                if count.checked_mul(2).is_none_or(|min| rem_after < min) {
                    cold_path();
                    return Err(Error::EOF);
                }
                for _ in 0..count {
                    pos = skip_str::<O>(pos, end_pos)?;
                }
                mark.push_leaf(
                    ((pos as usize) - (list_start as usize)) as u32,
                    (end_pos as usize) - (pos as usize),
                );
                Ok(ListNext::Leaf(pos))
            }
            9 | 10 => {
                if count == 0 {
                    mark.push_leaf(5, rem_after);
                    return Ok(ListNext::Leaf(pos));
                }
                let min_elem = if elem_type == 9 { 5 } else { 1 };
                if count
                    .checked_mul(min_elem)
                    .is_none_or(|min| rem_after < min)
                {
                    cold_path();
                    return Err(Error::EOF);
                }
                mark.ensure_slots(1 + count, rem_after);
                let list_mark = mark.alloc_slot(rem_after);
                Ok(ListNext::Container {
                    pos,
                    frame: ParentFrame {
                        start: list_start,
                        mark_idx: list_mark,
                        remaining: (count - 1) as u32,
                        kind: if elem_type == 10 {
                            ParentKind::ListCompound
                        } else {
                            ParentKind::ListList
                        },
                    },
                })
            }
            _ => {
                cold_path();
                Err(Error::INVALID(elem_type))
            }
        }
    }
}

pub(crate) fn parse_marks<O: ByteOrder>(src: &[u8], out_marks: &mut SmallMarks) -> Result<()> {
    let mut pos = src.as_ptr();
    let len = src.len();
    if len == 0 {
        cold_path();
        return Err(Error::EOF);
    }
    if len > u32::MAX as usize {
        cold_path();
        return Err(Error::LEN(len));
    }

    unsafe {
        let end_pos = pos.add(len);
        let root_tag = *pos;
        if root_tag == 0 {
            cold_path();
            return if len == 1 {
                Ok(())
            } else {
                Err(Error::REMAIN(len - 1))
            };
        }

        if len < 3 {
            cold_path();
            return Err(Error::EOF);
        }
        let root_name_len = byteorder::U16::<O>::from_bytes(*pos.add(1).cast()).get() as usize;
        let root_hdr = 3 + root_name_len;
        if len < root_hdr {
            cold_path();
            return Err(Error::EOF);
        }
        pos = pos.add(root_hdr);
        let rem = len - root_hdr;

        enum Mode {
            Compound,
            ListElement,
        }

        let mut mark = MarkBuf::new(out_marks);
        let mut stack = FrameStack::new();
        let mut cur_start: *const u8 = pos;
        let mut cur_mark: u32 = 0;
        let mut mode: Mode;

        match root_tag {
            1..=6 | 7 | 8 | 11 | 12 => {
                cold_path();
                let end = match root_tag {
                    1..=6 => {
                        let size = TagID::elem_size_raw(root_tag);
                        if rem < size {
                            cold_path();
                            return Err(Error::EOF);
                        }
                        pos.add(size)
                    }
                    8 => skip_str::<O>(pos, end_pos)?,
                    _ => skip_array::<O>(pos, end_pos, TagID::elem_size_raw(root_tag))?,
                };
                if end != end_pos {
                    cold_path();
                    return Err(Error::REMAIN((end_pos as usize) - (end as usize)));
                }
                return Ok(());
            }
            9 => mode = Mode::ListElement,
            10 => {
                cur_mark = mark.alloc_slot(rem);
                mode = Mode::Compound;
            }
            _ => {
                cold_path();
                return Err(Error::INVALID(root_tag));
            }
        }

        macro_rules! ascend {
            () => {{
                loop {
                    if stack.is_empty() {
                        if pos != end_pos {
                            cold_path();
                            return Err(Error::REMAIN((end_pos as usize) - (pos as usize)));
                        }
                        mark.commit();
                        return Ok(());
                    }
                    let top = stack.last_mut();
                    match top.kind {
                        ParentKind::Compound => {
                            let frame = stack.pop();
                            cur_start = frame.start;
                            cur_mark = frame.mark_idx;
                            mode = Mode::Compound;
                            break;
                        }
                        ParentKind::ListCompound => {
                            if top.remaining > 0 {
                                top.remaining -= 1;
                                cur_mark = mark.alloc_slot((end_pos as usize) - (pos as usize));
                                cur_start = pos;
                                mode = Mode::Compound;
                                break;
                            }
                            let frame = stack.pop();
                            mark.finish_slot(
                                frame.mark_idx,
                                ((pos as usize) - (frame.start as usize)) as u32,
                            );
                        }
                        ParentKind::ListList => {
                            if top.remaining > 0 {
                                top.remaining -= 1;
                                mode = Mode::ListElement;
                                break;
                            }
                            let frame = stack.pop();
                            mark.finish_slot(
                                frame.mark_idx,
                                ((pos as usize) - (frame.start as usize)) as u32,
                            );
                        }
                    }
                }
            }};
        }

        loop {
            match mode {
                Mode::Compound => 'comp_loop: loop {
                    let rem = (end_pos as usize) - (pos as usize);
                    if rem == 0 {
                        cold_path();
                        return Err(Error::EOF);
                    }

                    let tag_id = *pos;
                    if tag_id == 0 {
                        pos = pos.add(1);
                        mark.finish_slot(cur_mark, ((pos as usize) - (cur_start as usize)) as u32);
                        ascend!();
                        if matches!(mode, Mode::Compound) {
                            continue 'comp_loop;
                        }
                        break 'comp_loop;
                    }

                    if rem < 3 {
                        cold_path();
                        return Err(Error::EOF);
                    }
                    let name_len =
                        byteorder::U16::<O>::from_bytes(*pos.add(1).cast()).get() as usize;
                    let hdr = 3 + name_len;
                    if rem < hdr {
                        cold_path();
                        return Err(Error::EOF);
                    }
                    pos = pos.add(hdr);
                    let rem_val = rem - hdr;

                    match tag_id {
                        1..=6 => {
                            let size = TagID::elem_size_raw(tag_id);
                            if rem_val < size {
                                cold_path();
                                return Err(Error::EOF);
                            }
                            pos = pos.add(size);
                        }
                        7 | 11 | 12 => {
                            pos = skip_array::<O>(pos, end_pos, TagID::elem_size_raw(tag_id))?
                        }
                        8 => pos = skip_str::<O>(pos, end_pos)?,
                        9 => match parse_list::<O>(pos, end_pos, &mut mark)? {
                            ListNext::Leaf(end) => pos = end,
                            ListNext::Container { pos: p, frame } => {
                                pos = p;
                                stack.push(ParentFrame {
                                    start: cur_start,
                                    mark_idx: cur_mark,
                                    remaining: 0,
                                    kind: ParentKind::Compound,
                                });
                                stack.push(frame);
                                if frame.kind == ParentKind::ListCompound {
                                    cur_mark = mark.alloc_slot((end_pos as usize) - (pos as usize));
                                    cur_start = pos;
                                } else {
                                    mode = Mode::ListElement;
                                    break 'comp_loop;
                                }
                            }
                        },
                        10 => {
                            stack.push(ParentFrame {
                                start: cur_start,
                                mark_idx: cur_mark,
                                remaining: 0,
                                kind: ParentKind::Compound,
                            });
                            cur_mark = mark.alloc_slot(rem_val);
                            cur_start = pos;
                        }
                        _ => {
                            cold_path();
                            return Err(Error::INVALID(tag_id));
                        }
                    }
                },
                Mode::ListElement => match parse_list::<O>(pos, end_pos, &mut mark)? {
                    ListNext::Leaf(end) => {
                        pos = end;
                        ascend!();
                    }
                    ListNext::Container { pos: p, frame } => {
                        pos = p;
                        stack.push(frame);
                        if frame.kind == ParentKind::ListCompound {
                            cur_mark = mark.alloc_slot((end_pos as usize) - (pos as usize));
                            cur_start = pos;
                            mode = Mode::Compound;
                        }
                    }
                },
            }
        }
    }
}
