# membox brand

`icon-master.png` is the source of truth. Everything under `generated/` comes from it:

```sh
bin/icons                    # regenerate all sets from icon-master.png
bin/icons path/to/other.png  # or from a different master
```

The mark is a flat isometric cube, empty, no glow — flat geometry is what stays
legible at 16 px. `bin/icons` finds the dark artwork in the master, crops it
square, and cuts its own Apple superellipse (n=5), so the squircle geometry is
ours rather than whatever the generator drew.

| Output | Use |
|---|---|
| `generated/mac/AppIcon.icns` | macOS app bundle. Art inset to 824/1024 per Apple's icon grid. |
| `generated/ios/AppIcon.appiconset/` | iOS. Full-bleed, no alpha, no corners — iOS masks it. Includes a dark variant. |
| `generated/ios/LaunchBackground.colorset/` + `LaunchMark.imageset/` | iOS launch screen via the `UILaunchScreen` Info.plist dict (see `LaunchScreen-Info.plist.snippet`). No storyboard. |
| `generated/mark-1024.png`, `launch-mark-512.png` | The masked mark with real alpha, for the macOS About/first-run window, the sidebar library chip, and docs. |
| `generated/tauri/` | RGBA set + `.icns` that `desktop/src-tauri/tauri.conf.json` points at directly. `generate_context!` rejects non-RGBA PNGs. |
| `generated/contact-sheet.png` | 256→16 px legibility check. Look at this before shipping a redesign. |

macOS has no launch screen — see `docs/Feature-Spec.md` §9.5.
