"""Generates the Hopper icon sources (pixel-art Minecraft-style hopper).

    python gen_icon.py

Writes hopper.svg (app icon, on a dark rounded tile) and hopper-tray.svg
(sprite only, tight crop). Then, from src-tauri:

    pnpm tauri icon icons/source/hopper.svg
"""
from pathlib import Path

ROWS = [
    "..HHHHHHHHHHHH..",
    "..HSDDDDDDDDSS..",
    "..HSDDDDDDDDSS..",
    "...SSSSSSSSSS...",
    "....SSSSSSSS....",
    "....SSSSSSSS....",
    ".....SSSSSS.....",
    "......SSSS......",
    "......SSSS......",
    "......SSSS......",
    "......SSSS......",
]
PAL = {"H": "#9a9a9a", "S": "#6a6a6a", "T": "#474747", "D": "#1c1c1c", "O": "#0b0b0a"}


def sprite() -> list[str]:
    grid = [list(r) for r in ROWS]
    for row in grid:
        if "S" in row:
            row[len(row) - 1 - row[::-1].index("S")] = "T"
    h, w = len(grid), 16
    filled = lambda x, y: 0 <= x < w and 0 <= y < h and grid[y][x] != "."
    rects = []
    for y in range(-1, h + 1):
        for x in range(w):
            c = grid[y][x] if 0 <= y < h else "."
            if c == ".":
                if not any(filled(x + dx, y + dy) for dx, dy in ((-1, 0), (1, 0), (0, -1), (0, 1))):
                    continue
                c = "O"
            rects.append(f'<rect x="{x}" y="{y}" width="1" height="1" fill="{PAL[c]}"/>')
    return rects


def main() -> None:
    here = Path(__file__).parent
    body = "\n".join(sprite())
    # 11 sprite rows + outline = 13 rows; centre them in a 16-row square.
    tray = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -2.5 16 16" shape-rendering="crispEdges">\n'
        f"{body}\n</svg>\n"
    )
    app = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" shape-rendering="crispEdges">\n'
        '<rect x="0" y="0" width="128" height="128" rx="28" fill="#151411"/>\n'
        '<rect x="3" y="3" width="122" height="122" rx="25" fill="none" stroke="#2a2822" stroke-width="2"/>\n'
        f'<g transform="translate(16 31) scale(6)">\n{body}\n</g>\n</svg>\n'
    )
    (here / "hopper-tray.svg").write_text(tray)
    (here / "hopper.svg").write_text(app)


if __name__ == "__main__":
    main()
