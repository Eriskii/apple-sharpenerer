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

> **Status:** early, but working on macOS 26.4: per-corner and screen-aware corners, sheen
> removal, live settings and the background daemon.

## Requirements

- Apple Silicon, macOS 26
- SIP partially disabled (debugging restrictions off) and the `-arm64e_preview_abi` boot-arg.
  Code has to run inside each app to change its window shape, and macOS only allows that with
  these relaxed. Full SIP disable is not required.
- Root, to load the payload into apps

## Install

```sh
make
sudo target/dist/sharpenerer install
```

This puts the files in `/Library/Application Support/apple-sharpenerer`, links `sharpenerer` into
`/usr/local/bin`, starts a LaunchDaemon that loads into every app as it launches, and writes your
settings file to `~/.config/sharpenerer/config.toml`. `sudo sharpenerer uninstall` removes it all.

## Settings

Changes apply live to every app, no sudo needed:

```sh
sharpenerer set corners 0               # all four corners square away from display corners
sharpenerer set corners 10 10 0 0       # top-left, top-right, bottom-left, bottom-right
sharpenerer set screen-aware real       # corners near rounded display corners follow their curve
sharpenerer set screen-aware 8          # ...or just get 8pt there (any radius)
sharpenerer set screen-aware off        # just use the radii above
sharpenerer set anchor display          # measure from the panel edges instead of the usable area
sharpenerer set curve circular          # instead of Apple's continuous curve
sharpenerer set top-highlight on        # keep the bright top edge
sharpenerer set inner-rim on            # keep the white inner line
sharpenerer set outer-rim off           # drop the dark outline too
sharpenerer set enabled off             # everything back to stock, live
sharpenerer exclude com.apple.Terminal  # leave an app alone from its next launch
sharpenerer status
```

Screen-aware rounding only touches window corners within reach of a display corner that's actually
rounded (as far as its rounding extends, about 44pt on a MacBook). With `real`, such a corner
follows the display's curve concentrically: flush against it, the window gets the display's
rounding, and every point further away takes a point off, never going below the corner's base
radius. With a number, such corners simply get that radius. On a MacBook only the top corners of
the panel are rounded, so bottom window corners always keep their base radius. The daemon log is
`/Library/Logs/sharpenerer.log`; the payload logs to the unified log (`log stream --predicate
'eventMessage BEGINSWITH "[sharpenerer]"'`).

## How it works

`libsharpenerer.dylib` runs inside each app. It replaces the corner mask AppKit hands to the
window server (`-[NSCGSWindow setCornerMask:]`) with a per-corner path, keeps AppKit's own corner
radii consistent with it, hides the top highlight layer and zeroes the shadow rims. The display's
corner shape comes from `NSScreen.bezelPath`. Settings arrive packed into 64 bits, as the state of
a Darwin notification, so even sandboxed apps get live updates without reading any file.

Loading is [ErisMacInjectLib](https://github.com/Eriskii/ErisMacInjectLib)'s job, shared with
ErisMenus. The daemon doesn't use `dlopen`, so library validation never sees the payload: it maps
the payload's segments into the app itself and resolves its imports against the shared cache,
then a tiny arm64e helper (`eris-inject-thread`) starts its thread with properly signed
registers. The payload is `no_std` Rust on the library's payload runtime, which explains why std
can't run in arm64e apps.

`make devhost` builds a test app that loads the payload, opens windows at known spots and
captures them.

## Building

Needs the Xcode command line tools and rustup (the pinned nightly toolchain installs itself). Cargo
fetches ErisMacInjectLib from GitHub.

```sh
make            # target/dist/{sharpenerer,eris-inject-thread,libsharpenerer.dylib}
make test
```

## Credits

- [apple-sharpener](https://github.com/aspauldingcode/apple-sharpener) for the idea.
- [yabai](https://github.com/asmvik/yabai) for the injection technique (its arm64e path is based
  on work by Jeremy Legendre).

## License

GPL-3.0-or-later
