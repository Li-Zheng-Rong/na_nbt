//! Verifies that `read_borrowed` (and Serde `BorrowedDocument`) performs zero heap allocations
//! on small NBT documents (up to 8 `Compound`/`List` containers) and leaks zero allocations
//! when spilling to heap.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use na_nbt::{BigEndian, Compound, TypedList, ValueRef};

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
    // 1. Small packet / item-tag style NBT with <= 8 nested Compounds + Lists (<= 16 marks)
    //    plus large primitive arrays and strings.
    let mut root = Compound::<BigEndian>::default();
    root.insert("id", 276i16);
    root.insert("Count", 1i8);
    root.insert("ints", (0..1024).collect::<Vec<i32>>());
    root.insert("longs", (0..1024).collect::<Vec<i64>>());

    let mut item_tag = Compound::<BigEndian>::default();
    let mut display = Compound::<BigEndian>::default();
    display.insert("Name", "Diamond Sword");
    let mut lore = TypedList::<str, BigEndian>::default();
    lore.push("Line 1");
    lore.push("Line 2");
    display.insert("Lore", lore);
    item_tag.insert("display", display);

    let mut extra = Compound::<BigEndian>::default();
    extra.insert("id", "HYPERION");
    let mut ench = Compound::<BigEndian>::default();
    ench.insert("sharpness", 7i32);
    ench.insert("smite", 7i32);
    extra.insert("enchantments", ench);
    item_tag.insert("ExtraAttributes", extra);

    let mut pos = TypedList::<f64, BigEndian>::default();
    pos.push(100.5);
    pos.push(64.0);
    pos.push(-200.25);
    root.insert("Pos", pos);
    root.insert("tag", item_tag);

    let small_bytes = root.write_to_vec_be();
    let small_nameless = root.write_nameless_to_vec_be();

    // Parse, clone, and query nested fields: MUST perform 0 heap allocations!
    let (allocs, ()) = count_allocs(|| {
        let doc = na_nbt::read_borrowed_be(&small_bytes).unwrap();
        let cloned = doc.clone();
        assert_eq!(doc.get_short("id"), Some(276));
        assert_eq!(cloned.get_int_array("ints").map(|s| s.len()), Some(1024));
        assert_eq!(
            cloned
                .at("tag")
                .at("ExtraAttributes")
                .at("enchantments")
                .at("sharpness")
                .as_int(),
            Some(7)
        );
        let lore_ref = cloned
            .at("tag")
            .at("display")
            .get_list("Lore")
            .and_then(|l| l.strings())
            .unwrap();
        assert_eq!(lore_ref.get(1).unwrap().as_bytes(), b"Line 2");
        if let ValueRef::Compound(c) = doc.root() {
            assert!(!c.is_empty());
        } else {
            panic!("expected compound root");
        }

        let nameless_doc = na_nbt::read_borrowed_nameless_be(&small_nameless).unwrap();
        assert!(nameless_doc.root_name().is_none());
        assert_eq!(nameless_doc.get_short("id"), Some(276));
    });
    assert_eq!(
        allocs, 0,
        "small NBT parse + clone + query must perform 0 heap allocations"
    );

    #[cfg(feature = "serde")]
    {
        #[derive(serde::Deserialize)]
        struct ItemSlot<'a> {
            id: i16,
            #[serde(rename = "Count")]
            count: i8,
            #[serde(borrow)]
            tag: na_nbt::BorrowedDocument<'a>,
        }

        let (serde_allocs, ()) = count_allocs(|| {
            let doc: na_nbt::BorrowedDocument<'_> = na_nbt::from_slice_be(&small_bytes).unwrap();
            assert_eq!(doc.get_short("id"), Some(276));

            let slot: ItemSlot<'_> = na_nbt::from_slice_be(&small_bytes).unwrap();
            assert_eq!(slot.id, 276);
            assert_eq!(slot.count, 1);
            assert_eq!(
                slot.tag
                    .at("ExtraAttributes")
                    .at("enchantments")
                    .at("smite")
                    .as_int(),
                Some(7)
            );
        });
        assert_eq!(
            serde_allocs, 0,
            "Serde BorrowedDocument + hybrid struct deserialization on small NBT must perform 0 heap allocations"
        );
    }

    // 2. Large NBT (> 8 containers) spills cleanly to heap and frees all memory on Drop
    let mut big = Compound::<BigEndian>::default();
    let mut items = TypedList::<Compound, BigEndian>::default();
    for i in 0..40i32 {
        let mut entry = Compound::<BigEndian>::default();
        entry.insert("idx", i);
        let mut sub = Compound::<BigEndian>::default();
        sub.insert("v", i * 10);
        entry.insert("sub", sub);
        items.push(entry);
    }
    big.insert("items", items);
    let big_bytes = big.write_to_vec_be();

    let live_before = LIVE_ALLOCS.load(Ordering::SeqCst);
    {
        let doc = na_nbt::read_borrowed_be(&big_bytes).unwrap();
        let doc2 = doc.clone();
        let list = doc2.get_list("items").and_then(|l| l.compounds()).unwrap();
        assert_eq!(list.len(), 40);
        assert_eq!(list.get(39).unwrap().at("sub").at("v").as_int(), Some(390));
        // Truncated large NBT after spilling must also free its heap buffer cleanly
        let truncated = &big_bytes[..big_bytes.len() - 5];
        assert!(na_nbt::read_borrowed_be(truncated).is_err());
    }
    let live_after = LIVE_ALLOCS.load(Ordering::SeqCst);
    assert_eq!(live_before, live_after, "spilled SmallMarks leaked memory");
}
