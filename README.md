# apple-sharpenerer

Window corner control for macOS 26 (Tahoe), in the spirit of
[apple-sharpener](https://github.com/aspauldingcode/apple-sharpener), but with more knobs:

- **Per-corner radii**: set each corner of a window independently.
- **Screen-aware rounding**: a window corner only rounds when it sits near a rounded corner of
  the display, and follows it concentrically. On a MacBook that means the top corners can match
  the panel's rounding while the bottoms (square on the panel) stay square. Corners are measured
  against either the usable area (below the menu bar) or the physical panel.
- **No sheen**: independently remove the bright top-edge highlight, the white inner rim and the
  dark outer rim Tahoe draws around windows.

> **Status:** early work in progress. The corner geometry and the injector work; the payload that
> reshapes windows, the config and the background daemon are being built.

## Requirements

- Apple Silicon, macOS 26
- SIP partially disabled (debugging restrictions off) and the `-arm64e_preview_abi` boot-arg.
  Code has to run inside each app to change its window shape, and macOS only allows that with
  these relaxed. Full SIP disable is not required.
- Root, to load the payload into apps

## How it works

`libsharpenerer.dylib` is loaded into GUI apps. Inside, it replaces the corner mask AppKit hands
to the window server (`-[NSCGSWindow setCornerMask:]`) with a per-corner path, keeps AppKit's own
titlebar drawing consistent with it, and turns off the highlight layer and shadow rims. The
display's corner shape comes from `NSScreen.bezelPath`.

The payload is `no_std` Rust built for both arm64 (third-party apps) and arm64e (Apple's apps).
See `crates/payload/src/lib.rs` for why it can't use std.

## Building

Needs the Xcode command line tools and rustup; the pinned nightly toolchain installs itself.

```sh
make            # target/universal/{sharpenerer,libsharpenerer.dylib}
make test
```

## Credits

- [apple-sharpener](https://github.com/aspauldingcode/apple-sharpener) for the idea.
- [yabai](https://github.com/asmvik/yabai) for the injection technique (its arm64e path is based
  on work by Jeremy Legendre).

## License

GPL-3.0-or-later
