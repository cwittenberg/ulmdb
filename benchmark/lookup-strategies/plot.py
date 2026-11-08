"""Export a static comparison figure; each row uses its own native baseline."""
from pathlib import Path
import argparse
import json
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("summary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--preview", type=Path)
    args = parser.parse_args()
    cache = Path(__file__).resolve().parents[1] / ".cache/python"
    if cache.is_dir():
        sys.path.insert(0, str(cache))
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    rows = json.loads(args.summary.read_text())["rows"]
    selected = []
    for row in rows:
        if row["keys"] != 100000:
            continue
        if row["suite"] == "prefix" and row["distribution"] == "hit":
            label = f"4-bit prefix / {row['backend_or_family']} / {row['operation']}"
        elif row["suite"] == "exact" and row["backend_or_family"] == "resident" and row["operation"] == "mixed50":
            label = f"Hash / Resident / {row['key_kind']} / mixed"
        elif row["suite"] == "ordered" and row["candidate"] == "bbtree_0_2_0_unsync" and row["operation"] in {"mixed_50_50", "range64"}:
            label = f"bbtree / Legacy / {row['key_kind']} / {row['operation']}"
        else:
            continue
        selected.append((label, row))
    figure, axis = plt.subplots(figsize=(11, 6.8), layout="constrained")
    for index, (label, row) in enumerate(selected):
        middle = row["paired_process_speedup_median"]
        low, high = row["paired_process_speedup_min"], row["paired_process_speedup_max"]
        color = "#216c59" if low >= 1.10 else "#a33e33" if middle < 1 else "#887743"
        axis.plot(middle, index, "o", color=color, markersize=7)
        axis.errorbar(middle, index, xerr=[[middle-low], [high-middle]], color="#202020", capsize=3, linewidth=1)
        axis.text(high * 1.04, index, f"{middle:.2f}x", va="center", fontsize=9)
    axis.set_yticks(range(len(selected)), [label for label, _ in selected])
    axis.invert_yaxis()
    axis.set_xscale("log")
    axis.axvline(1, color="#555555", linewidth=1, linestyle="--")
    axis.set_xlabel("Speedup versus that experiment's native baseline (log scale)")
    axis.set_title("100,000 records · warm single-core lookups\nThree fresh processes; marks span observed process medians", loc="left")
    axis.spines[["top", "right"]].set_visible(False)
    axis.grid(axis="x", color="#dddddd", alpha=.5)
    axis.set_axisbelow(True)
    figure.savefig(args.output, format="svg", metadata={"Date": None})
    if args.preview:
        figure.savefig(args.preview, dpi=120)


if __name__ == "__main__":
    main()
