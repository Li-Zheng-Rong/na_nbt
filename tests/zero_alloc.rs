//! Verifies that `read_borrowed` performs zero heap allocations on small NBT documents
//! (up to 8 `Compound`/`List` containers) and leaks zero allocations when spilling to heap.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use na_nbt::{tag, BigEndian, CompoundOwn, TypedListOwn, ValueRef, Writable};

struct CountingAlloc;

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static LIVE_ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        LIVE_ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_ALLOCS.fetch_sub(1, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn count_allocs<R>(f: impl FnOnce() -> R) -> (usize, R) {
    let before = ALLOC_COUNT.load(Ordering::SeqCst);
    let out = f();
    let after = ALLOC_COUNT.load(Ordering::SeqCst);
    (after - before, out)
}

#[test]
fn test_zero_heap_alloc_for_small_nbts_and_clean_spill() {
    // 1. Small packet / item-tag style NBT with 10 nested Compounds + Lists (<= 16 marks)
    //    plus large primitive arrays and strings.
    let mut root = CompoundOwn::<BigEndian>::default();
    root.insert("id", 276i16);
    root.insert("Count", 1i8);
    root.insert("ints", (0..1024).collect::<Vec<i32>>());
    root.insert("longs", (0..1024).collect::<Vec<i64>>());

    let mut item_tag = CompoundOwn::<BigEndian>::default();
    let mut display = CompoundOwn::<BigEndian>::default();
    display.insert("Name", "Diamond Sword");
    let mut lore = TypedListOwn::<BigEndian, tag::String>::default();
    lore.push("Line 1");
    lore.push("Line 2");
    display.insert("Lore", lore);
    item_tag.insert("display", display);

    let mut extra = CompoundOwn::<BigEndian>::default();
    extra.insert("id", "HYPERION");
    let mut ench = CompoundOwn::<BigEndian>::default();
    ench.insert("sharpness", 7i32);
    ench.insert("smite", 7i32);
    extra.insert("enchantments", ench);
    item_tag.insert("ExtraAttributes", extra);

    let mut pos = TypedListOwn::<BigEndian, tag::Double>::default();
    pos.push(100.5);
    pos.push(64.0);
    pos.push(-200.25);
    root.insert("Pos", pos);
    root.insert("tag", item_tag);

    let small_bytes = root.write_to_vec::<BigEndian>();

    // Parse, clone, and query nested fields: MUST perform 0 heap allocations!
    let (allocs, ()) = count_allocs(|| {
        let doc = na_nbt::read_borrowed::<BigEndian>(&small_bytes).unwrap();
        let cloned = doc.clone();
        assert_eq!(doc.get_as::<tag::Short>("id"), Some(276));
        assert_eq!(
            cloned
                .get("ints")
                .and_then(|v| v.as_::<tag::IntArray>())
                .map(|s| s.len()),
            Some(1024)
        );
        let tag_ref = cloned.get_as::<tag::Compound>("tag").unwrap();
        let ench_ref = tag_ref
            .get_as::<tag::Compound>("ExtraAttributes")
            .unwrap()
            .get_as::<tag::Compound>("enchantments")
            .unwrap();
        assert_eq!(ench_ref.get_as::<tag::Int>("sharpness"), Some(7));
        let lore_ref = tag_ref
            .get_as::<tag::Compound>("display")
            .unwrap()
            .get_as::<tag::TypedList<tag::String>>("Lore")
            .unwrap();
        assert_eq!(lore_ref.get(1).unwrap().decode_lossy().as_ref(), "Line 2");
        if let ValueRef::Compound(c) = doc.root() {
            assert!(!c.is_empty());
        } else {
            panic!("expected compound root");
        }
    });
    assert_eq!(
        allocs, 0,
        "small NBT parse + clone + query must perform 0 heap allocations"
    );

    // 2. Large NBT (> 16 containers) spills cleanly to heap and frees all memory on Drop
    let mut big = CompoundOwn::<BigEndian>::default();
    let mut items = TypedListOwn::<BigEndian, tag::Compound>::default();
    for i in 0..40i32 {
        let mut entry = CompoundOwn::<BigEndian>::default();
        entry.insert("idx", i);
        let mut sub = CompoundOwn::<BigEndian>::default();
        sub.insert("v", i * 10);
        entry.insert("sub", sub);
        items.push(entry);
    }
    big.insert("items", items);
    let big_bytes = big.write_to_vec::<BigEndian>();

    let live_before = LIVE_ALLOCS.load(Ordering::SeqCst);
    {
        let doc = na_nbt::read_borrowed::<BigEndian>(&big_bytes).unwrap();
        let doc2 = doc.clone();
        let list = doc2
            .get_as::<tag::TypedList<tag::Compound>>("items")
            .unwrap();
        assert_eq!(list.len(), 40);
        assert_eq!(
            list.get(39)
                .unwrap()
                .get_as::<tag::Compound>("sub")
                .unwrap()
                .get_as::<tag::Int>("v"),
            Some(390)
        );
        // Truncated large NBT after spilling must also free its heap buffer cleanly
        let truncated = &big_bytes[..big_bytes.len() - 5];
        assert!(na_nbt::read_borrowed::<BigEndian>(truncated).is_err());
    }
    let live_after = LIVE_ALLOCS.load(Ordering::SeqCst);
    assert_eq!(live_before, live_after, "spilled SmallMarks leaked memory");
}
