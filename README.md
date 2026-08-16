# Latchshot

A lightweight yet intelligent window-aware screenshot tool for Wayland. Latchshot freezes the current desktop, snaps to the window under the pointer, and falls back to a freely drawn region when you drag.

## Requirements

Latchshot targets compositors that expose the wlroots-style protocols needed by its custom overlay. In addition to the core Wayland globals for compositing, shared memory, outputs, seats, and subsurfaces, the compositor must support:

- [`wlr-layer-shell-unstable-v1`](https://wayland.app/protocols/wlr-layer-shell-unstable-v1) (`zwlr_layer_shell_v1`)
- [`viewporter`](https://wayland.app/protocols/viewporter) (`wp_viewporter`)
- At least one supported capture path:
  - [`ext-image-copy-capture-v1`](https://wayland.app/protocols/ext-image-copy-capture-v1) together with an output source from [`ext-image-capture-source-v1`](https://wayland.app/protocols/ext-image-capture-source-v1)
  - [`wlr-screencopy-unstable-v1`](https://wayland.app/protocols/wlr-screencopy-unstable-v1), together with [`xdg-output-unstable-v1`](https://wayland.app/protocols/xdg-output-unstable-v1)

### Compositor support

| Compositor | Status | Selection support |
| --- | --- | --- |
| [Niri (with a customized fork)](https://github.com/so1ve/niri/tree/feat/latchshot-support) | Supported | Window snapping and free-form regions |
| [Upstream Niri](https://github.com/niri-wm/niri) | Not supported | Missing the `WindowGeometries` IPC extension |
| [Sway](https://github.com/swaywm/sway) | Planned | Region-only backend not implemented yet |
| [Hyprland](https://github.com/hyprwm/Hyprland) | Planned | Region-only backend not implemented yet |
| [Mango](https://github.com/mangowm/mango) | Planned | Region-only backend not implemented yet |
| KDE Plasma | Intentionally unsupported | — |
| GNOME | Intentionally unsupported | — |

Protocol support alone is not currently sufficient: Latchshot also needs a scene backend to provide output placement and, when available, window geometry. The CLI already reserves compositor identifiers for Sway, Hyprland, and Mango, but their scene backends have not been implemented.

KDE Plasma and GNOME remain intentionally out of scope because Latchshot targets this protocol stack rather than portal- or desktop-shell-specific screenshot flows.

The default clipboard destination also requires `wl-copy` from [`wl-clipboard`](https://github.com/bugaevc/wl-clipboard). It is included automatically by the Nix package and is unnecessary when using `--output` or `--stdout`.

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
- Press <kbd>Esc</kbd> or right-click to cancel.

Save directly to a file:

```sh
latchshot --output ~/Pictures/screenshot.png
```

Write PNG data to standard output:

```sh
latchshot --stdout > screenshot.png
```

Force a capture backend or disable animation:

```sh
latchshot --capture screencopy
latchshot --capture image-copy-capture --no-animation
```

Print the discovered scene as JSON for diagnostics:

```sh
latchshot --windows
```

Run `latchshot --help` for all options. Set `RUST_LOG=latchshot=debug` for additional diagnostics.

## License

[MIT](LICENSE). Made with ♥️ by [Ray](https://github.com/so1ve).
