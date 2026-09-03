# phaethon

generate base16 color schemes from images. you give it a wallpaper or photo, it gives you 16 colors as json. does the math in oklch so the colors actually look right.


## install

**binary**

grab the tip build from releases (linux x86_64, static musl, updated on every push to main):

```
https://github.com/hambosto/iridion/releases/tag/tip
```

```bash
curl -LO https://github.com/hambosto/phaethon/releases/download/tip/phaethon-x86_64-unknown-linux-musl.tar.gz
tar xzf phaethon-x86_64-unknown-linux-musl.tar.gz
sudo mv phaethon /usr/local/bin/
```

**from source**

```bash
git clone https://github.com/hambosto/phaethon
cd phaethon
cargo build --release
# binary at target/release/phaethon
```

needs rust stable. `cargo install --path .` also works.


## usage

```
phaethon -i <image> [-c <contrast>] [--resize <size>] [-o <output>]
```

- `-i, --image` path to image, required
- `-c, --contrast` 0.0 to 1.0, default 0.5. higher = more saturated/brighter accents
- `--resize` resize image to NxN before clustering, default 256. 0 = full resolution
- `-o, --output` write JSON to file instead of stdout

examples:

```bash
phaethon -i wallpaper.png
phaethon -i photo.jpg -c 0.8 > theme.json
phaethon -i sunset.jpg --contrast 0.2
phaethon -i image.webp | jq .
phaethon -i photo.jpg --resize 512 -o theme.json
phaethon -i photo.jpg --resize 0
```

output is json with `base00` through `base0F`, hex without `#`:

```json
{
  "base00": "1a1c2a",
  "base01": "171a27",
  "base02": "2a2e3e",
  "base03": "3a3f52",
  "base04": "5a5f72",
  "base05": "c8ccd8",
  "base06": "d0d4e0",
  "base07": "607080",
  "base08": "e06070",
  "base09": "e0a060",
  "base0A": "e0d060",
  "base0B": "60c070",
  "base0C": "60b0c0",
  "base0D": "6080e0",
  "base0E": "b060d0",
  "base0F": "d060a0"
}
```

`base00-05` are backgrounds, `base06` foreground, `base07` bright background, `base08-0F` accents.


## how it works

1. loads image, resizes to NxN (default 256x256, configurable with `--resize`)
2. converts to oklch, throws out near-gray pixels
3. normalizes and runs k-means with 8 clusters, each locked to a 45-degree hue slice (tries a few offsets and picks the best)
4. finds the two dominant hue zones, builds backgrounds from the main one
5. scales chroma/lightness based on `--contrast` and maps to the 16 base16 slots

the hue-locking is the main thing — it stops clusters from spinning around the color wheel like normal k-means does.


## formats

whatever the `image` crate can open: png, jpeg, gif, bmp, tiff, webp, qoi, avif, exr, etc.


## dev

```bash
cargo fmt --check
cargo clippy
cargo test
```


## license

mit — see [LICENSE](LICENSE)
