#!/usr/bin/env python3
"""Regenerate the README benchmark tables' ranks/order and the charts in `images/`.

The README tables are the single source of truth: edit the `X.XX GB/s` numbers (ranks may be
left stale), then run

    python bench/charts.py

from the repository root (needs matplotlib). For every table listed in `TABLES` this
recomputes each cell's `(#n)` rank within its column, sorts the rows by geometric mean
throughput, rewrites the table in place, and renders its charts.
"""

from __future__ import annotations

import math
import re
from dataclasses import dataclass, field
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.ticker import MaxNLocator  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
README = ROOT / "README.md"
IMAGES = ROOT / "images"


@dataclass
class Chart:
    file: str
    title: str
    log: bool
    xlabel: str = "Test File"


@dataclass
class TableSpec:
    heading: str  # exact README heading line the table follows
    charts: list[Chart] = field(default_factory=list)


def pair(stem: str, title: str) -> list[Chart]:
    """A linear and a log chart of the same table."""
    return [
        Chart(f"{stem}_linear.png", f"{title} (Linear Scale)", False),
        Chart(f"{stem}_log.png", f"{title} (Log Scale)", True),
    ]


TABLES = [
    TableSpec("### Read", pair("read", "NBT Read Throughput")),
    TableSpec("### Write", pair("write", "NBT Write Throughput")),
    TableSpec(
        "### Parse + Extract (hypixel.nbt)",
        [Chart("parse.png", "NBT Parse + Extract Throughput, hypixel.nbt (Linear Scale)", False,
               xlabel="Test File")],
    ),
    TableSpec("#### DOM Deserialize", pair("serde_de", "Serde DOM Deserialize Throughput")),
    TableSpec("#### DOM Serialize", pair("serde_ser", "Serde DOM Serialize Throughput")),
    TableSpec(
        "#### Typed Struct (hypixel.nbt)",
        [Chart("serde_typed.png", "Serde Typed Struct Throughput, hypixel.nbt (Linear Scale)",
               False, xlabel="Operation")],
    ),
]

# ----------------------------------------------------------------------------- styling

BG = "#0d1117"
PANEL = "#161b22"
GRID = "#30363d"
TEXT = "#e6edf3"
EDGE = "#c9d1d9"

COLORS = {
    "na_nbt v0.3 (borrow)": "#1a7f37",
    "na_nbt v0.3 (shared)": "#2ea043",
    "na_nbt v0.3 (owned)": "#56d364",
    "na_nbt v0.3": "#1a7f37",
    "na_nbt v0.2 (borrow)": "#1f6f3a",
    "na_nbt v0.2 (shared)": "#23824a",
    "na_nbt v0.2 (owned)": "#3a9a52",
    "na_nbt v0.2": "#1f6f3a",
    "simdnbt (borrow)": "#1f6feb",
    "simdnbt (owned)": "#388bfd",
    "simdnbt": "#1f6feb",
    "ussr (borrow)": "#6e40c9",
    "azalea": "#e3631a",
    "shen": "#da3633",
    "valence": "#1b9aaa",
    "graphite": "#6b7a90",
    "crab_nbt": "#8b5a2b",
    "hematite": "#a333c8",
    "fastnbt": "#d6246e",
}
FALLBACK = ["#f0b429", "#8dd3c7", "#bebada", "#fb8072", "#80b1d3"]


def style_for(label: str, i: int) -> dict:
    color = COLORS.get(label, FALLBACK[i % len(FALLBACK)])
    if "v0.2" in label:
        return dict(color=color, hatch="////", edgecolor=EDGE, linewidth=0.6, alpha=0.85)
    return dict(color=color, edgecolor=EDGE, linewidth=0.6)


# ----------------------------------------------------------------------------- tables

CELL = re.compile(r"^\s*([\d.]+)\s*GB/s(?:\s*\(#\d+\))?\s*$")


@dataclass
class Table:
    start: int  # line index of the header row
    end: int  # one past the last row
    columns: list[str]
    rows: list[tuple[str, list[float]]]


def find_table(lines: list[str], heading: str) -> Table:
    try:
        h = lines.index(heading)
    except ValueError:
        raise SystemExit(f"heading not found in README: {heading!r}")
    i = h + 1
    while not lines[i].startswith("|"):
        if lines[i].startswith("#"):
            raise SystemExit(f"no table under {heading!r}")
        i += 1
    start = i
    header = [c.strip() for c in lines[i].strip().strip("|").split("|")]
    i += 2  # header + separator
    rows = []
    while i < len(lines) and lines[i].startswith("|"):
        cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
        values = []
        for c in cells[1:]:
            m = CELL.match(c)
            if not m:
                raise SystemExit(f"cannot parse cell {c!r} under {heading!r}")
            values.append(float(m.group(1)))
        rows.append((cells[0], values))
        i += 1
    return Table(start, i, header[1:], rows)


def geomean(values: list[float]) -> float:
    return math.exp(sum(math.log(v) for v in values) / len(values))


def render_table(table: Table) -> list[str]:
    rows = sorted(table.rows, key=lambda r: geomean(r[1]), reverse=True)
    ncol = len(table.columns)
    ranked = []
    for name, values in rows:
        cells = []
        for c, v in enumerate(values):
            rank = 1 + sum(other[1][c] > v for other in rows)
            cells.append(f"{v:.2f} GB/s (#{rank})")
        ranked.append([name] + cells)
    header = ["Library"] + table.columns
    widths = [max(len(r[c]) for r in ranked + [header]) for c in range(ncol + 1)]
    fmt = lambda r: "| " + " | ".join(x.ljust(w) for x, w in zip(r, widths)) + " |"
    out = [fmt(header), "| " + " | ".join("-" * w for w in widths) + " |"]
    out += [fmt(r) for r in ranked]
    table.rows = rows
    return out


# ----------------------------------------------------------------------------- charts


def render_chart(table: Table, chart: Chart) -> None:
    columns = table.columns
    series = table.rows

    plt.rcParams.update({"font.family": "Noto Sans", "font.size": 11})
    fig, ax = plt.subplots(figsize=(16, 10), dpi=150)
    fig.patch.set_facecolor(BG)
    ax.set_facecolor(PANEL)

    n = len(series)
    group_width = 0.84 if n > 1 else 0.6
    width = group_width / n
    for i, (name, values) in enumerate(series):
        xs = [x - group_width / 2 + width * (i + 0.5) for x in range(len(columns))]
        ax.bar(xs, values, width * 0.96, label=name, **style_for(name, i))

    ax.set_xticks(range(len(columns)))
    ax.set_xticklabels(columns, rotation=30, ha="right", color=TEXT)
    peak = max(max(v) for _, v in series)
    if chart.log:
        ax.set_yscale("log")
        low = min(min(v) for _, v in series)
        ax.set_ylim(10 ** math.floor(math.log10(low) - 0.15), peak * 2.5)
    else:
        ticks = MaxNLocator(nbins=5).tick_values(0, peak * 1.12)
        ax.set_ylim(0, ticks[-1])
    ax.set_xlim(-0.5, len(columns) - 0.5)

    ax.set_title(chart.title, color=TEXT, fontsize=17, fontweight="bold", pad=14)
    ax.set_xlabel(chart.xlabel, color=TEXT, fontsize=13, fontweight="bold", labelpad=12)
    ax.set_ylabel(
        "Throughput (GB/s) - higher is better",
        color=TEXT,
        fontsize=13,
        fontweight="bold",
        labelpad=12,
    )
    ax.tick_params(colors=TEXT, which="both")
    ax.grid(axis="y", color=GRID, linestyle="--", linewidth=0.9)
    ax.set_axisbelow(True)
    for spine in ax.spines.values():
        spine.set_color(GRID)

    legend = ax.legend(
        loc="upper right",
        ncol=2,
        fontsize=9,
        frameon=True,
        facecolor=PANEL,
        edgecolor=GRID,
        labelcolor=TEXT,
        borderpad=1.0,
        columnspacing=3.0,
    )
    legend.get_frame().set_alpha(1.0)

    fig.tight_layout()
    fig.savefig(IMAGES / chart.file, facecolor=BG)
    plt.close(fig)
    print(f"wrote images/{chart.file}")


# ----------------------------------------------------------------------------- main


def main() -> None:
    lines = README.read_text(encoding="utf-8").split("\n")
    # Process bottom-up so earlier line indices stay valid while tables are rewritten.
    found = [(spec, find_table(lines, spec.heading)) for spec in TABLES]
    for spec, table in sorted(found, key=lambda st: st[1].start, reverse=True):
        lines[table.start : table.end] = render_table(table)
    README.write_text("\n".join(lines), encoding="utf-8")
    print("updated README.md tables")
    for spec, table in found:
        for chart in spec.charts:
            render_chart(table, chart)


if __name__ == "__main__":
    main()
