# Device fonts

`*.x3f` are bitmap fonts made by `x3-tools font` (format: `x3_core::font`)
from Literata 3.103, the googlefonts/literata release
<https://github.com/googlefonts/literata/releases/tag/3.103>
(`3.103.zip`, SHA-256 `f7fb973cafb26cf785cbebaeaf51c18f87c15a3bcf4d82a7d4857564db5b056d`).

| File | Source | Size |
| --- | --- | --- |
| `literata-regular-24.x3f` | `fonts/ttf/Literata-Regular.ttf` | 24 px, body |
| `literata-bold-28.x3f` | `fonts/ttf/Literata-Bold.ttf` | 28 px, headings |

Characters: `x3_tools::font::charset` (Latin, Polish, Latin-1, Greek,
punctuation, arrows, maths operators the font has). Rebuilt 2026-10-27 with
Greek and maths added (24,370 and 31,398 bytes).

Rebuild:

```
cargo run -p x3-tools -- font Literata-Regular.ttf 24 assets/fonts/literata-regular-24.x3f
cargo run -p x3-tools -- font Literata-Bold.ttf 28 assets/fonts/literata-bold-28.x3f
```

Licence: SIL Open Font License 1.1, copyright 2017 The Literata Project
Authors (`Literata-OFL.txt`). The licence declares no Reserved Font Name, so
these rasterised versions keep the name. The TTFs are not stored here.
