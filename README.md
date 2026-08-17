# Latchshot

A lightweight yet intelligent window-aware screenshot tool for Wayland. Latchshot freezes the current desktop, snaps to the window under the pointer, and falls back to a freely drawn region when you drag.

## Requirements

Latchshot targets compositors that expose the Wayland protocols needed by its custom overlay. The compositor must support:

- [`wlr-layer-shell-unstable-v1`](https://wayland.app/protocols/wlr-layer-shell-unstable-v1) (`zwlr_layer_shell_v1`)
- [`viewporter`](https://wayland.app/protocols/viewporter) (`wp_viewporter`)
- At least one supported capture path:
  - [`ext-image-copy-capture-v1`](https://wayland.app/protocols/ext-image-copy-capture-v1) together with an output source from [`ext-image-capture-source-v1`](https://wayland.app/protocols/ext-image-capture-source-v1)
  - [`wlr-screencopy-unstable-v1`](https://wayland.app/protocols/wlr-screencopy-unstable-v1), together with [`xdg-output-unstable-v1`](https://wayland.app/protocols/xdg-output-unstable-v1)

### Compositor support

| Compositor | Status | Selection support |
| --- | --- | --- |
| [Niri (with a customized fork)](https://github.com/so1ve/niri/tree/feat/latchshot-support) | Supported | Window snapping and free-form regions |
| [Upstream Niri](https://github.com/niri-wm/niri) | Supported | Free-form regions only |
| [Sway](https://github.com/swaywm/sway) | Supported | Window snapping and free-form regions |
| [Hyprland](https://github.com/hyprwm/Hyprland) | Supported | Window snapping and free-form regions |
| [Mango](https://github.com/mangowm/mango) | Supported | Window snapping and free-form regions |
| Other compatible Wayland compositors | Best effort | Free-form regions only |
| KDE Plasma | Intentionally unsupported | — |
| GNOME | Intentionally unsupported | — |

The `generic` backend is selected automatically for unknown compositors. Upstream Niri also switches to it when the compositor rejects the custom `WindowGeometries` request as unknown. Capture and free-form region selection continue to work, while window snapping is disabled.

KDE Plasma and GNOME remain intentionally out of scope because Latchshot targets this protocol stack rather than portal- or desktop-shell-specific screenshot flows.

Copying to clipboard (the default destiniation) also requires `wl-copy` from [`wl-clipboard`](https://github.com/bugaevc/wl-clipboard).

## Installation

### From source

With Cargo:

```sh
cargo install latchshot
```

### Nix

Run directly:

```sh
nix run github:so1ve/latchshot
```

With a Nix flake, add Latchshot to the inputs:

```nix
inputs.latchshot.url = "github:so1ve/latchshot";
```

Then add the package to a NixOS configuration:

```nix
{ inputs, pkgs, ... }:

{
  environment.systemPackages = [
    inputs.latchshot.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```

Alternatively, use the overlay to make `pkgs.latchshot` available:

```nix
{ inputs, pkgs, ... }:

{
  nixpkgs.overlays = [
    inputs.latchshot.overlays.default
  ];

  environment.systemPackages = [
    pkgs.latchshot
  ];
}
```

### Cachix

Use the project cache for prebuilt Nix artifacts when available:

```nix
nix.settings = {
  extra-substituters = [ "https://so1ve.cachix.org" ];
  extra-trusted-public-keys = [
    "so1ve.cachix.org-1:51jcW4FkJhiLcqPsiUx3nglRP469les8F9zjFxio1nw="
  ];
};
```

## Usage

Run Latchshot with no destination to copy the selected screenshot to the clipboard:

```sh
latchshot
```

During selection:

- Move the pointer to highlight the window underneath it.
- Left-click a highlighted window to capture it.
- Left-drag to select an arbitrary region.
- Press <kbd>F</kbd> to capture the output under the pointer.
- Press <kbd>Esc</kbd> or right-click to cancel.

Save directly to a file:

```sh
latchshot --output ~/Pictures/screenshot.png
```

Write PNG data to standard output:

```sh
latchshot --stdout > screenshot.png
```

To disable animation:

```sh
latchshot --no-animation
```

Print the discovered scene as JSON for diagnostics:

```sh
latchshot --windows
```

Run `latchshot --help` for all options. Set `RUST_LOG=latchshot=debug` for additional diagnostics.

## Library Usage

Latchshot can also be used as a Rust library. See the [API documentation on docs.rs](https://docs.rs/latchshot) for details.

## License

[MIT](LICENSE). Made with ♥️ by [Ray](https://github.com/so1ve).
