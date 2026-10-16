"""Convert the official emblem into a build-time occupancy mask."""
from pathlib import Path
from PIL import Image
root = Path(__file__).resolve().parents[1]
assets = root / "codex-rs/tui/assets/xjtu"
image = Image.open(assets / "emblem.jpg").convert("RGB").resize((148, 148), Image.Resampling.LANCZOS)
mask = bytearray(160 * 160)
for y in range(148):
    for x in range(148):
        r, g, b = image.getpixel((x, y))
        mask[(y + 6) * 160 + x + 6] = int(r - g > 35 and g < 210)
(assets / "emblem.mask").write_bytes(mask)
print(f"Generated {len(mask)} samples; {sum(mask)} occupied")
