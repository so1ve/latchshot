# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.5](https://github.com/so1ve/latchshot/compare/v0.2.4...v0.2.5) - 2026-08-21

### Added

- display absolute path in notification
- allow disabling desktop notifications

### Fixed

- *(overlay)* reattach solid surface buffers on every redraw

### Other

- publish binaries when releasing

## [0.2.4](https://github.com/so1ve/latchshot/compare/v0.2.3...v0.2.4) - 2026-08-21

### Added

- allow combining multiple output destinations

### Fixed

- *(overlay)* prevent input events from leaking to underlying apps
- *(capture)* avoid extra pixels from floating-point rounding

### Other

- rename `write` to `write_to_targets`
- simply loop over targets instead of generating specific messages

### Added

- allow file, standard output, and clipboard destinations to be combined

## [0.2.3](https://github.com/so1ve/latchshot/compare/v0.2.2...v0.2.3) - 2026-08-19

### Added

- *(niri)* add env var to force fallback reconstruction

### Fixed

- avoid running multiple latchshot instances
- *(niri)* ignore spurious edge fragments in fallback
- *(niri)* recover partially offscreen windows in fallback

### Other

- add ai usage disclosure
- mention limitations about upstream niri support
- use webp demo
- update demo video
- add demo

## [0.2.2](https://github.com/so1ve/latchshot/compare/v0.2.1...v0.2.2) - 2026-08-17

### Fixed

- *(niri)* fall back when `WindowGeometries` is rejected

### Other

- add binary build

## [0.2.1](https://github.com/so1ve/latchshot/compare/v0.2.0...v0.2.1) - 2026-08-17

### Added

- add fallback path for upstream niri

### Other

- add nix ci

## [0.2.0](https://github.com/so1ve/latchshot/compare/v0.1.1...v0.2.0) - 2026-08-17

### Added

- support capturing full screen using `F`
- remove corners to align with actual capture output
- [**breaking**] automatically select capture backend and remove `wlr-capture`

## [0.1.1](https://github.com/so1ve/latchshot/compare/v0.1.0...v0.1.1) - 2026-08-17

### Added

- dedicated compositor backends for hyprland, mango and sway
- provide a fallback backend
- packaging for nix

### Other

- install missing dependencies
- add ci and release workflows
- link to docs.rs
- correct terms
- configure renovate ([#1](https://github.com/so1ve/latchshot/pull/1))
- update comment
- remove interior mutability to align with existing IPCs
- remove slop
- use `swaps_axes`
- reduce useless code
- add README
- init
