//! Serde benchmarks.
//!
//! * `compare_serde/{file}`: dynamic (DOM) values through each library's serde implementation —
//!   `*_serde_de` deserializes NBT bytes into the library's DOM type, `*_serde_ser` serializes that
//!   DOM back to NBT bytes. Only libraries whose DOM survives a serde round trip are listed:
//!   na_nbt v0.2 has no serde support for its DOM types, hematite's `Blob` narrows numbers to the
//!   smallest fitting type (and corrupts `inttest1023`), and crab_nbt turns byte arrays into lists.
//! * `compare_serde_typed/hypixel.nbt`: a `#[derive(Serialize, Deserialize)]` struct (owned
//!   `String` fields) describing the Hypixel item list, for every library with a serde data format.

use std::{collections::HashMap, fs::File, hint::black_box, io::Read};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

fn load(filename: &str) -> Vec<u8> {
    let mut file = File::open(format!("tests/{filename}")).unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).unwrap();
    let mut src = &contents[..];
    let mut decoder = GzDecoder::new(&mut src);
    let mut input = Vec::new();
    if decoder.read_to_end(&mut input).is_err() {
        input = contents;
    }
    input
}

/// Registers a benchmark only if the library handles this file; prints why otherwise, so a
/// missing row in the report is never silent.
macro_rules! bench_if_ok {
    ($group:ident, $name:literal, $file:expr, $probe:expr, $body:expr) => {
        match (|| -> Result<_, String> { $probe })() {
            Ok(_) => {
                $group.bench_function($name, |b| b.iter(|| black_box($body)));
            }
            Err(e) => eprintln!("skip {} on {}: {e}", $name, $file),
        }
    };
}

fn bench_dom(filename: &str, c: &mut Criterion) {
    let input = load(filename);
    let data = input.as_slice();

    let mut group = c.benchmark_group(format!("compare_serde/{filename}"));
    group.throughput(Throughput::Bytes(input.len() as u64));

    type Be = na_nbt::BigEndian;

    // ---------------------------------------------------------------- deserialize
    bench_if_ok!(
        group,
        "na_nbt_borrow_serde_de",
        filename,
        na_nbt::from_slice_be::<na_nbt::BorrowedDocument<Be>>(data).map_err(|e| e.to_string()),
        na_nbt::from_slice_be::<na_nbt::BorrowedDocument<Be>>(black_box(data)).unwrap()
    );
    bench_if_ok!(
        group,
        "na_nbt_owned_serde_de",
        filename,
        na_nbt::from_slice_be::<na_nbt::Value<Be>>(data).map_err(|e| e.to_string()),
        na_nbt::from_slice_be::<na_nbt::Value<Be>>(black_box(data)).unwrap()
    );
    bench_if_ok!(
        group,
        "fastnbt_serde_de",
        filename,
        fastnbt::from_bytes::<fastnbt::Value>(data).map_err(|e| e.to_string()),
        fastnbt::from_bytes::<fastnbt::Value>(black_box(data)).unwrap()
    );
    // ---------------------------------------------------------------- serialize
    let doc = na_nbt::read_borrowed::<Be>(data).unwrap();
    bench_if_ok!(
        group,
        "na_nbt_borrow_serde_ser",
        filename,
        na_nbt::to_vec_be(&doc).map_err(|e| e.to_string()),
        na_nbt::to_vec_be(black_box(&doc)).unwrap()
    );
    let owned = na_nbt::read_owned::<Be, Be>(data).unwrap();
    bench_if_ok!(
        group,
        "na_nbt_owned_serde_ser",
        filename,
        na_nbt::to_vec_be(&owned).map_err(|e| e.to_string()),
        na_nbt::to_vec_be(black_box(&owned)).unwrap()
    );
    if let Ok(value) = fastnbt::from_bytes::<fastnbt::Value>(data) {
        bench_if_ok!(
            group,
            "fastnbt_serde_ser",
            filename,
            fastnbt::to_bytes(&value).map_err(|e| e.to_string()),
            fastnbt::to_bytes(black_box(&value)).unwrap()
        );
    }
    group.finish();
}

// -------------------------------------------------------------------- typed (Hypixel items)

/// The same Hypixel item schema for every library. na_nbt maps a plain `Vec` to its own sequence
/// encoding and needs `#[serde(with = "na_nbt::list")]` for an NBT `TAG_List`; the other
/// libraries encode `Vec` as `TAG_List` directly.
macro_rules! hypixel_types {
    ($module:ident $(, #[$list:meta])?) => {
        mod $module {
            use super::*;

            #[derive(Serialize, Deserialize)]
            pub struct Root {
                $(#[$list])?
                i: Vec<Item>,
            }

            #[derive(Serialize, Deserialize)]
            pub struct Item {
                #[serde(default)]
                id: i16,
                #[serde(default, rename = "Count")]
                count: i8,
                #[serde(default, rename = "Damage")]
                damage: i16,
                #[serde(default)]
                tag: Tag,
            }

            #[derive(Serialize, Deserialize, Default)]
            pub struct Tag {
                #[serde(default)]
                display: Display,
                #[serde(default, rename = "ExtraAttributes")]
                extra: Extra,
            }

            #[derive(Serialize, Deserialize, Default)]
            pub struct Display {
                #[serde(default, rename = "Name")]
                name: String,
                #[serde(default, rename = "Lore")]
                $(#[$list])?
                lore: Vec<String>,
                #[serde(default)]
                color: i32,
            }

            #[derive(Serialize, Deserialize, Default)]
            pub struct Extra {
                #[serde(default)]
                id: String,
                #[serde(default)]
                modifier: String,
                #[serde(default)]
                timestamp: String,
                #[serde(default)]
                enchantments: HashMap<String, i32>,
            }
        }
    };
}

hypixel_types!(na03, #[serde(with = "na_nbt::list")]);
hypixel_types!(na02, #[serde(with = "na_nbt_02::list")]);
hypixel_types!(plain);

fn bench_typed(filename: &str, c: &mut Criterion) {
    let input = load(filename);
    let data = input.as_slice();

    let mut group = c.benchmark_group(format!("compare_serde_typed/{filename}"));
    group.throughput(Throughput::Bytes(input.len() as u64));

    bench_if_ok!(
        group,
        "na_nbt_typed_de",
        filename,
        na_nbt::from_slice_be::<na03::Root>(data).map_err(|e| e.to_string()),
        na_nbt::from_slice_be::<na03::Root>(black_box(data)).unwrap()
    );
    bench_if_ok!(
        group,
        "na_nbt_02_typed_de",
        filename,
        na_nbt_02::from_slice_be::<na02::Root>(data).map_err(|e| e.to_string()),
        na_nbt_02::from_slice_be::<na02::Root>(black_box(data)).unwrap()
    );
    bench_if_ok!(
        group,
        "fastnbt_typed_de",
        filename,
        fastnbt::from_bytes::<plain::Root>(data).map_err(|e| e.to_string()),
        fastnbt::from_bytes::<plain::Root>(black_box(data)).unwrap()
    );
    bench_if_ok!(
        group,
        "crab_typed_de",
        filename,
        crab_nbt::serde::de::from_bytes::<plain::Root>(&mut &data[..]).map_err(|e| e.to_string()),
        crab_nbt::serde::de::from_bytes::<plain::Root>(&mut black_box(data)).unwrap()
    );
    bench_if_ok!(
        group,
        "hematite_typed_de",
        filename,
        nbt::from_reader::<_, plain::Root>(data).map_err(|e| e.to_string()),
        nbt::from_reader::<_, plain::Root>(black_box(data)).unwrap()
    );

    // Each serializer gets the value its own library decoded (identical contents).
    if let Ok(root) = na_nbt::from_slice_be::<na03::Root>(data) {
        bench_if_ok!(
            group,
            "na_nbt_typed_ser",
            filename,
            na_nbt::to_vec_be(&root).map_err(|e| e.to_string()),
            na_nbt::to_vec_be(black_box(&root)).unwrap()
        );
    }
    if let Ok(root) = na_nbt_02::from_slice_be::<na02::Root>(data) {
        bench_if_ok!(
            group,
            "na_nbt_02_typed_ser",
            filename,
            na_nbt_02::to_vec_be(&root).map_err(|e| e.to_string()),
            na_nbt_02::to_vec_be(black_box(&root)).unwrap()
        );
    }
    if let Ok(root) = fastnbt::from_bytes::<plain::Root>(data) {
        bench_if_ok!(
            group,
            "fastnbt_typed_ser",
            filename,
            fastnbt::to_bytes(&root).map_err(|e| e.to_string()),
            fastnbt::to_bytes(black_box(&root)).unwrap()
        );
        bench_if_ok!(
            group,
            "crab_typed_ser",
            filename,
            crab_nbt::serde::ser::to_bytes(&root, String::new()).map_err(|e| e.to_string()),
            crab_nbt::serde::ser::to_bytes(black_box(&root), String::new()).unwrap()
        );
        bench_if_ok!(
            group,
            "hematite_typed_ser",
            filename,
            {
                let mut out = Vec::new();
                nbt::to_writer(&mut out, &root, None).map_err(|e| e.to_string())
            },
            {
                let mut out = Vec::new();
                nbt::to_writer(&mut out, black_box(&root), None).unwrap();
                out
            }
        );
    }
    group.finish();
}

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn bench(c: &mut Criterion) {
    for file in [
        "bigtest.nbt",
        "complex_player.dat",
        "hypixel.nbt",
        "inttest1023.nbt",
        "level.dat",
        "longtest1024.nbt",
        "simple_player.dat",
    ] {
        bench_dom(file, c);
    }
    bench_typed("hypixel.nbt", c);
}

criterion_group!(compare_serde, bench);
criterion_main!(compare_serde);
