"""Generate fresh synthetic fixtures; their answers are never included in prompts.

Requires Pillow >= 10. Example:
    python scripts/create-xjcode-vision-fixtures.py F:/XJCodeModelTests/vision-new
"""
import argparse
import hashlib
import json
from pathlib import Path
import secrets

from PIL import Image, ImageDraw, ImageFont


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_directory", type=Path)
    root = parser.parse_args().output_directory.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if (root / "fixtures.json").exists():
        parser.error("Use a new directory; existing fixture evidence is preserved.")
    font_path = Path("C:/Windows/Fonts/consolab.ttf")
    font = ImageFont.truetype(str(font_path), 72) if font_path.exists() else ImageFont.load_default(size=72)
    fixtures = []
    for index in range(2):
        code = "".join(secrets.choice("ABCDEFGHJKLMNPQRSTUVWXYZ23456789") for _ in range(8))
        picture = Image.new("RGB", (800, 440), "white")
        draw = ImageDraw.Draw(picture)
        if index == 0:
            draw.polygon([(140, 35), (45, 220), (235, 220)], fill="#ef7c18")
            draw.ellipse((540, 35, 730, 225), fill="#8038bd")
        else:
            draw.rectangle((45, 40, 235, 230), fill="#2054d9")
            draw.polygon([(635, 35), (540, 225), (730, 225)], fill="#d92b31")
        draw.text((180, 310), code, font=font, fill="black")
        path = root / f"fixture-{index + 1}.png"
        picture.save(path)
        fixtures.append({"file": str(path), "code": code, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
    (root / "fixtures.json").write_text(json.dumps(fixtures, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Created {len(fixtures)} independent synthetic image fixtures")


if __name__ == "__main__":
    main()
