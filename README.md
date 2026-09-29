# spotdiff

[![CI](https://github.com/mitsuru/spotdiff/actions/workflows/ci.yml/badge.svg)](https://github.com/mitsuru/spotdiff/actions/workflows/ci.yml)

[日本語](README.ja.md)

A terminal image diff viewer written in Rust. It displays images using the Kitty Graphics Protocol and lets you switch between side-by-side comparison, highlighting changes, and Blink mode, which alternates between the images in a single pane.

Select an image in lazygit and press `I` to open the fullscreen viewer. Press `q` to quit, then press Enter at lazygit's return prompt to go back.

![spotdiff's side-by-side comparison. The original image is on the left and the modified image is on the right, showing color changes to a circle and rectangle and the addition of a yellow bar.](https://raw.githubusercontent.com/mitsuru/spotdiff/main/docs/images/side-by-side.png)

`Before` is on the left and `After` is on the right. The footer shows the display mode, zoom level, number of changed pixels, and percentage of pixels changed.

## Installation

You need Rust, Cargo, and a terminal that supports the Kitty Graphics Protocol, such as Kitty or Ghostty. Builds have been verified with Rust 1.96.0.

```sh
cargo install --path crates/spotdiff --locked
```

Add the installation directory, `~/.cargo/bin`, to your PATH. To try it without installing, run `cargo run --release -- before.png after.png`. Use a release build for image processing.

## Usage

```sh
# Two image files
spotdiff before.png after.png

# Unstaged changes: index → working tree
spotdiff git -- assets/image.png

# Staged changes: HEAD → index
spotdiff git --staged -- assets/image.png
```

PNG, JPEG, and static WebP images are supported. Transparent areas are shown on a checkerboard background. Git mode handles added and untracked files, `git add -N`, deleted files, and repositories with no commits yet. File paths can be specified relative to the directory where you run the command.

The viewer opens even when there are no changes. The exit code is 0 on normal exit and nonzero for loading or terminal errors. Git files, the index, and configuration are left unchanged.

### Try the sample images

Run the following command from the repository root to compare the images used in the screenshots:

```sh
cargo run --release --locked -- docs/images/before.png docs/images/after.png
```

The viewer starts with the side-by-side comparison shown above. Each press of `Tab` cycles through side-by-side → highlight → Blink → side-by-side.

**Highlight (press `Tab` once):** Changed areas are highlighted in magenta, and unchanged areas are dimmed. This shows the color changes to the circle and rectangle, as well as the location of the added bar.

![Highlight mode. The circle and rectangle with changed colors and the added bar are highlighted in magenta.](https://raw.githubusercontent.com/mitsuru/spotdiff/main/docs/images/highlight.png)

**Blink (press `Tab` twice):** Switch between the original and modified images in a single pane to compare the same location. Press `Space` to switch manually, or `a` to start switching automatically every 500 ms, like a GIF. Press `a` again to stop.

![Blink mode. Before and After alternate in the same pane, revealing the color changes and the added bar.](https://raw.githubusercontent.com/mitsuru/spotdiff/main/docs/images/blink.gif)

### Keyboard controls

| Key | Action |
| --- | --- |
| `Tab` | Cycle through side-by-side → highlight → Blink |
| `Space` | Switch between Before and After in Blink mode (stops automatic playback if active) |
| `a` | Start / stop automatic switching in Blink mode (500 ms interval) |
| `+` / `-` | Zoom in / out |
| Arrow keys / `h j k l` | Pan |
| `f` | Fit to screen |
| `1` | Actual size (one image pixel per terminal pixel) |
| `q` / `Esc` / `Ctrl-C` | Quit |

Both sides share the same image coordinates, zoom level, and position. Resizing the terminal recalculates the zoom level when fitting to the screen and preserves it when zooming manually. Images are resized using nearest-neighbor interpolation to make pixel boundaries easier to inspect.

Entering Blink mode displays `Before` in a single pane and starts in manual mode. The pane title identifies the current image, and the footer shows `Manual` or `Auto`. Before and After share the same zoom level and position, and you can zoom and pan in this mode as well. When both images exist, they are cached, so switching alone does not retransmit image data. For additions or deletions, an explanation is shown for the missing image. With `herdr --remote`, returning to the existing image after displaying the missing side may cause the relay to retransmit the image.

Only the image comparison canvas is transmitted as an image; the terminal background is shown outside it. Transparent areas within images and areas where one image is absent because of differing dimensions use a checkerboard background.

## lazygit integration

Merge [examples/lazygit.yml](https://github.com/mitsuru/spotdiff/blob/main/examples/lazygit.yml) into `customCommands` in your existing lazygit configuration. Press `e` in lazygit's Status panel to open the configuration file. If `customCommands` already exists, add the array entries without duplicating the key.

- `I`: Compare the index and working tree if there are unstaged changes; otherwise, compare HEAD and the index.
- `Ctrl-S`: Explicitly compare staged changes.
- For files with both staged and unstaged changes, `I` prioritizes unstaged changes.

If these keys conflict with existing bindings, change `key` in the example configuration. Non-image files produce an error with an explanation. The viewer uses `output: terminal` to pause lazygit while it runs. In lazygit 0.65.0, `Press enter to return to lazygit` appears after the viewer exits; press Enter to return.

Displaying images within lazygit's diff panel is a future extension. The initial integration uses a fullscreen viewer.

## Diff definition and limitations

- Decoded 8-bit RGBA values are compared before scaling. A pixel is considered changed if any RGBA value differs. However, RGB differences are ignored when alpha is 0 on both sides.
- Images with different dimensions are aligned at the top-left corner. Pixels that exist on only one side also count as changed. The denominator for the percentage of pixels changed is the area where pixels exist on at least one side.
- Highlight mode displays changed areas in magenta and dims unchanged areas. Deleted areas use the original image.
- Each input's decoded RGBA data and the comparison canvas are each limited to 256 MiB. Total memory usage can exceed this amount.
- To limit memory usage for displayed images, each image's display area is capped at 297 columns × 297 rows. Images fit within this area even on larger terminals; when zoomed in, you can pan across the entire image.
- Inputs other than regular files, such as FIFOs and devices, are rejected.
- SVG, animations, perceptual diffs, ICC color management, automatic EXIF rotation, Git LFS expansion, rename tracking, merge conflicts, and symbolic links are not supported.
- The viewer refuses to start inside tmux or screen. Run it directly in a supported terminal. There is no text fallback for unsupported terminals.
- An interactive TTY is required. The viewer is not designed for redirecting image control sequences to a file.

## Development and verification

The root `Cargo.toml` defines a virtual workspace. The binary, library, Rust tests, and Rust examples live in `crates/spotdiff`, while dependencies and package metadata are managed at the root. `Cargo.lock` and `target/` are shared across the workspace. Python PTY tests and the lazygit configuration example are kept at the root. Run Cargo commands from the repository root.

GitHub Actions runs formatting checks, Rust tests, Clippy, release builds, and terminal PTY tests across the workspace on pushes to main and pull requests targeting main, using Ubuntu 24.04 and Rust 1.96.0. It also creates and verifies a build of the distribution crate and runs integration tests for launching the viewer and returning to lazygit 0.65.0.

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo build --workspace --locked --release --bins --examples
cargo package -p spotdiff --locked
python3 tests/terminal_pty.py target/release/spotdiff target/release/examples/terminal-failure
python3 tests/lazygit_pty.py target/release/spotdiff
```

The PTY tests simulate terminal responses and verify that terminal modes, the cursor, and the screen are restored. The lazygit PTY tests use an installed copy of lazygit and a temporary configuration. Follow the [manual testing instructions](https://github.com/mitsuru/spotdiff/blob/main/docs/manual-testing.md) to check image appearance and restoration in a real terminal.

## Design

See the [design specification](https://github.com/mitsuru/spotdiff/blob/main/docs/superpowers/specs/2026-09-28-spotdiff-design.md) and [implementation plan](https://github.com/mitsuru/spotdiff/blob/main/docs/superpowers/plans/2026-09-28-spotdiff.md). Image loading and diff generation are separate from terminal rendering.

The [implementation and verification notes](https://github.com/mitsuru/spotdiff/blob/main/docs/implementation-notes.md) summarize fixes from independent reviews and decisions made during implementation.

See [performance verification](https://github.com/mitsuru/spotdiff/blob/main/docs/performance.md) for image transfer improvements addressing update delays in Ghostty and the measurement conditions.
