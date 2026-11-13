# na_nbt

A high-performance NBT (Named Binary Tag) library for Rust with zero-copy parsing, flat-buffer owned mutation, and seamless Serde integration.
See [documentation](https://docs.rs/na_nbt) for full API details.

> ⚠️ **Note:** This crate is under active development. APIs may change between versions.
> Issues and contributions are welcome!

## Features

- **Zero-copy parsing** — Read NBT data with [`BorrowedDocument`](https://docs.rs/na_nbt/latest/na_nbt/type.BorrowedDocument.html) / [`SharedDocument`](https://docs.rs/na_nbt/latest/na_nbt/type.SharedDocument.html); small NBT documents (up to 8 containers) perform **zero heap allocations**
- **Flat-buffer owned DOM** — Build and mutate [`Compound`](https://docs.rs/na_nbt/latest/na_nbt/struct.Compound.html), [`List`](https://docs.rs/na_nbt/latest/na_nbt/struct.List.html), [`TypedList`](https://docs.rs/na_nbt/latest/na_nbt/struct.TypedList.html), and [`Value`](https://docs.rs/na_nbt/latest/na_nbt/enum.Value.html) in place
- **Ergonomic navigation & list views** — Chainable `at()` path queries, direct NBT-named getters (`get_int`, `get_str`, `get_string`, `get_compound`, ...), zero-copy list slices (`bytes`, `ints`, ...), and single-dispatch [`ListKind`](https://docs.rs/na_nbt/latest/na_nbt/enum.ListKind.html)
- **Serde + DOM interoperability** — Derive `Serialize` / `Deserialize` on Rust structs and freely embed `Compound`, `Value`, `TypedList<T>`, `MUTF8String`, `&'a MUTF8Str`, or zero-copy `BorrowedDocument<'a>`
- **Endianness & root formats** — Convert between `BigEndian` (Java Edition) and `LittleEndian` (Bedrock Edition) on read or write, with full support for named roots and Minecraft 1.20.2+ nameless network roots

## Installation

```toml
[dependencies]
na_nbt = "0.3"
```

### Optional Features

`serde`, `shared`, and `i128` are enabled by default. To customize features:

```toml
[dependencies]
na_nbt = { version = "0.3", default-features = false }

# Or enable only what you need:
na_nbt = { version = "0.3", default-features = false, features = ["serde"] }
```

| Feature   | Default | Description                                                          | Dependencies |
| --------- | ------- | -------------------------------------------------------------------- | ------------ |
| `serde`   | Yes     | Serialize/deserialize Rust types and hybrid DOM structures           | `serde`      |
| `shared`  | Yes     | `SharedDocument` backed by reference-counted `bytes::Bytes`          | `bytes`      |
| `i128`    | Yes     | Map `i128` / `u128` (e.g. Minecraft UUIDs) to 4-element `IntArray`   | —            |
| `nightly` | No      | Enable nightly SIMD acceleration in `simd_cesu8`                     | —            |

## Quick Start

### 1. Zero-Copy Borrowed Reading

```rust
use na_nbt::read_borrowed_be;

fn inspect(bytes: &[u8]) -> na_nbt::Result<()> {
    let doc = read_borrowed_be(bytes)?;

    // Chainable `at()` returns `ValueRef::End` on missing keys or out-of-bounds indices
    let health = doc.at("Player").at("Health").as_short().unwrap_or(20);
    let first_x = doc.at("Player").at("Pos").at(0).as_double();

    // Direct typed getters and zero-copy list views
    if let Some(items) = doc.get_list("Inventory").and_then(|l| l.compounds()) {
        for item in items {
            let id = item.get_str("id");
            let count = item.get_byte("Count").unwrap_or(1);
            let _ = (health, first_x, id, count);
        }
    }
    Ok(())
}
```

### 2. Owned Building & In-Place Mutation

```rust
use na_nbt::{Compound, TypedList};

let mut player = Compound::new();
player.insert("Name", "Alex");
player.insert("Health", 20i16);

let mut pos = TypedList::<f64>::new();
pos.extend_from_slice(&[100.5, 64.0, -200.25]);
player.insert("Pos", pos);

if let Some(mut hp) = player.get_short_mut("Health") {
    *hp -= 4;
}

// Standard root (`[tag_id][name_len][name][payload]`) or 1.20.2+ nameless network root:
let bytes = player.write_to_vec_be();
let network_bytes = player.write_nameless_to_vec_be();
```

### 3. Serde + Hybrid DOM

```rust
use na_nbt::{from_slice_be, to_vec_be, BorrowedDocument, Compound};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct ItemSlot<'a> {
    id: &'a str,
    count: i8,
    // Keep dynamic or version-dependent subtrees as zero-copy `BorrowedDocument` or mutable `Compound`!
    #[serde(borrow)]
    components: BorrowedDocument<'a>,
    #[serde(flatten)]
    extra: Compound,
}
```

## Todo

- [x] **More convenient APIs**
- [x] **Documentation**
- [x] **Tests and fuzzing**
- [ ] **MSRV testing**
- [ ] **`no_std` support**
- [ ] **Older Rust support**

## Benchmarks

na_nbt is the fastest NBT library in Rust. Tests are adapted from `simdnbt`'s benchmark suite (measured in the same run on Intel Core i9-14900KF; both `na_nbt v0.3` and `na_nbt v0.2` are included for direct version comparison).

### Read Performance

![Read Performance - Linear](./images/read_linear.png)
![Read Performance - Logarithmic](./images/read_log.png)

| Library              | bigtest.nbt      | complex_player.dat | hypixel.nbt      | inttest1023.nbt  | level.dat        | longtest1024.nbt | simple_player.dat |
| -------------------- | ---------------- | ------------------ | ---------------- | ---------------- | ---------------- | ---------------- | ----------------- |
| na_nbt v0.3 (borrow) | 31.24 GB/s (#1)  | 6.00 GB/s (#2)     | 12.21 GB/s (#1)  | 448.49 GB/s (#1) | 6.50 GB/s (#2)   | 906.01 GB/s (#1) | 8.00 GB/s (#1)    |
| na_nbt v0.2 (borrow) | 28.82 GB/s (#2)  | 6.14 GB/s (#1)     | 12.16 GB/s (#3)  | 235.46 GB/s (#2) | 6.50 GB/s (#1)   | 468.34 GB/s (#2) | 7.10 GB/s (#3)    |
| na_nbt v0.3 (shared) | 27.17 GB/s (#3)  | 5.94 GB/s (#3)     | 12.19 GB/s (#2)  | 201.24 GB/s (#3) | 6.41 GB/s (#3)   | 392.13 GB/s (#3) | 7.51 GB/s (#2)    |
| simdnbt (borrow)     | 18.01 GB/s (#5)  | 4.72 GB/s (#5)     | 9.27 GB/s (#5)   | 157.57 GB/s (#5) | 5.26 GB/s (#5)   | 321.75 GB/s (#5) | 4.94 GB/s (#5)    |
| na_nbt v0.2 (shared) | 21.24 GB/s (#4)  | 5.80 GB/s (#4)     | 11.91 GB/s (#4)  | 90.80 GB/s (#6)  | 6.39 GB/s (#4)   | 180.72 GB/s (#6) | 5.65 GB/s (#4)    |
| ussr (borrow)        | 7.46 GB/s (#6)   | 1.30 GB/s (#8)     | 3.18 GB/s (#6)   | 160.16 GB/s (#4) | 1.45 GB/s (#8)   | 352.18 GB/s (#4) | 1.93 GB/s (#8)    |
| na_nbt v0.2 (owned)  | 6.91 GB/s (#7)   | 2.20 GB/s (#6)     | 2.73 GB/s (#7)   | 78.44 GB/s (#7)  | 2.32 GB/s (#6)   | 75.38 GB/s (#7)  | 3.05 GB/s (#6)    |
| na_nbt v0.3 (owned)  | 6.79 GB/s (#8)   | 1.84 GB/s (#7)     | 2.72 GB/s (#8)   | 67.63 GB/s (#8)  | 2.03 GB/s (#7)   | 70.38 GB/s (#8)  | 2.86 GB/s (#7)    |
| simdnbt (owned)      | 4.59 GB/s (#9)   | 1.03 GB/s (#9)     | 2.10 GB/s (#9)   | 18.41 GB/s (#9)  | 1.09 GB/s (#9)   | 16.84 GB/s (#11) | 1.38 GB/s (#9)    |
| azalea               | 2.94 GB/s (#10)  | 0.60 GB/s (#11)    | 1.44 GB/s (#10)  | 5.59 GB/s (#11)  | 0.60 GB/s (#11)  | 12.21 GB/s (#13) | 0.77 GB/s (#11)   |
| shen                 | 2.73 GB/s (#11)  | 0.75 GB/s (#10)    | 1.10 GB/s (#11)  | 0.78 GB/s (#12)  | 0.80 GB/s (#10)  | 26.45 GB/s (#10) | 0.94 GB/s (#10)   |
| valence              | 2.16 GB/s (#12)  | 0.40 GB/s (#13)    | 0.89 GB/s (#13)  | 6.77 GB/s (#10)  | 0.40 GB/s (#13)  | 12.67 GB/s (#12) | 0.43 GB/s (#13)   |
| graphite             | 2.05 GB/s (#13)  | 0.42 GB/s (#12)    | 1.01 GB/s (#12)  | 0.65 GB/s (#13)  | 0.41 GB/s (#12)  | 11.83 GB/s (#15) | 0.37 GB/s (#14)   |
| crab_nbt             | 1.59 GB/s (#14)  | 0.35 GB/s (#14)    | 0.64 GB/s (#14)  | 0.42 GB/s (#15)  | 0.37 GB/s (#14)  | 27.75 GB/s (#9)  | 0.44 GB/s (#12)   |
| hematite             | 0.85 GB/s (#16)  | 0.26 GB/s (#15)    | 0.55 GB/s (#15)  | 0.43 GB/s (#14)  | 0.26 GB/s (#15)  | 11.91 GB/s (#14) | 0.26 GB/s (#15)   |
| fastnbt              | 1.15 GB/s (#15)  | 0.25 GB/s (#16)    | 0.54 GB/s (#16)  | 0.28 GB/s (#16)  | 0.25 GB/s (#16)  | 11.48 GB/s (#16) | 0.24 GB/s (#16)   |

### Write Performance

![Write Performance - Logarithmic](./images/write_log.png)

| Library              | bigtest.nbt     | complex_player.dat | hypixel.nbt      | inttest1023.nbt  | level.dat        | longtest1024.nbt | simple_player.dat |
| -------------------- | --------------- | ------------------ | ---------------- | ---------------- | ---------------- | ---------------- | ----------------- |
| na_nbt v0.2 (shared) | 95.10 GB/s (#1) | 150.85 GB/s (#2)   | 210.77 GB/s (#2) | 174.65 GB/s (#2) | 183.31 GB/s (#2) | 242.49 GB/s (#1) | 72.93 GB/s (#1)   |
| na_nbt v0.2 (borrow) | 95.06 GB/s (#2) | 150.88 GB/s (#1)   | 211.73 GB/s (#1) | 175.07 GB/s (#1) | 183.43 GB/s (#1) | 242.32 GB/s (#2) | 71.08 GB/s (#2)   |
| na_nbt v0.3 (borrow) | 91.62 GB/s (#3) | 146.76 GB/s (#3)   | 209.65 GB/s (#3) | 170.28 GB/s (#3) | 178.22 GB/s (#3) | 236.69 GB/s (#3) | 53.27 GB/s (#4)   |
| na_nbt v0.3 (shared) | 86.90 GB/s (#4) | 141.38 GB/s (#4)   | 206.33 GB/s (#4) | 164.37 GB/s (#4) | 173.88 GB/s (#4) | 235.89 GB/s (#4) | 60.11 GB/s (#3)   |
| na_nbt v0.3 (owned)  | 11.63 GB/s (#5) | 5.36 GB/s (#5)     | 6.59 GB/s (#6)   | 55.45 GB/s (#6)  | 5.44 GB/s (#5)   | 55.11 GB/s (#6)  | 5.20 GB/s (#5)    |
| na_nbt v0.2 (owned)  | 11.52 GB/s (#6) | 4.96 GB/s (#6)     | 7.39 GB/s (#5)   | 56.08 GB/s (#5)  | 5.17 GB/s (#6)   | 58.49 GB/s (#5)  | 4.84 GB/s (#6)    |
| simdnbt (borrow)     | 8.05 GB/s (#8)  | 2.78 GB/s (#8)     | 5.36 GB/s (#8)   | 28.31 GB/s (#7)  | 2.99 GB/s (#8)   | 31.37 GB/s (#7)  | 2.93 GB/s (#8)    |
| simdnbt (owned)      | 8.49 GB/s (#7)  | 3.20 GB/s (#7)     | 6.11 GB/s (#7)   | 11.95 GB/s (#8)  | 3.36 GB/s (#7)   | 11.85 GB/s (#8)  | 2.99 GB/s (#7)    |
| azalea               | 7.37 GB/s (#10) | 2.61 GB/s (#9)     | 5.29 GB/s (#9)   | 7.42 GB/s (#9)   | 2.86 GB/s (#9)   | 11.08 GB/s (#10) | 2.55 GB/s (#9)    |
| graphite             | 7.80 GB/s (#9)  | 2.03 GB/s (#10)    | 4.52 GB/s (#10)  | 2.19 GB/s (#10)  | 2.27 GB/s (#10)  | 11.24 GB/s (#9)  | 2.21 GB/s (#10)   |

### Parse Performance

![Parse Performance - Linear](./images/parse.png)

| Library     | hypixel.nbt    |
| ----------- | -------------- |
| na_nbt v0.3 | 1.73 GB/s (#1) |
| na_nbt v0.2 | 1.60 GB/s (#2) |
| simdnbt     | 1.41 GB/s (#3) |
| azalea      | 0.54 GB/s (#4) |
| graphite    | 0.31 GB/s (#5) |

---

_Higher throughput (GB/s) is better._

## Contributing

This crate is under active development. Contributions are welcome!

- **Bug reports**: Please open an issue with a minimal reproducible example
- **Feature requests**: Open an issue describing the use case
- **Pull requests**: Fork the repo, make your changes, and submit a PR

## License

MIT OR Apache-2.0
