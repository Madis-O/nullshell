# NullShell

A standalone terminal emulator for Windows, written in Rust, styled after Cyberpunk 2077.
Its own window and its own app: it hosts PowerShell 7 through ConPTY and draws everything
itself on the GPU.

> **Built with AI help.** I made NullShell together with Claude (Anthropic's AI, through
> Claude Code). I chose the stack, the requirements and the look I wanted; Claude wrote
> most of the code in this first version and tested it with me. I'm upfront about that
> because I'd rather you know how it was made. From here on I'm extending it myself.

- winit window, wgpu renderer, cosmic-text for font shaping
- alacritty_terminal for terminal emulation, portable-pty for ConPTY
- truecolor, scrollback, mouse selection, copy/paste, mouse reporting for TUI apps
- per-pixel window transparency (text stays solid, only the background fades)
- HUD chrome, glow, scanlines, chromatic aberration, scan beam, glitch burst on bell
- pixel-perfect built-in box drawing, block and braille characters

## Requirements

- Windows 10 1809 or newer (Windows 11 for the tinted title bar and square corners)
- [Rust](https://rustup.rs) with the MSVC toolchain (Visual Studio Build Tools, "Desktop development with C++")
- Optional: [PowerShell 7](https://aka.ms/powershell). Without it NullShell falls back to
  Windows PowerShell, then cmd.
- Optional: a [Nerd Font](https://www.nerdfonts.com) for icon glyphs. Either set it as
  `font.family`, or just install one (for example "Symbols Nerd Font Mono" or
  "JetBrainsMono NF") and NullShell uses it for icons automatically.

## Install

```
.\install.ps1
```

This builds NullShell, copies `nullshell.exe` to `%LOCALAPPDATA%\Programs\NullShell` and
adds a **NullShell** entry to the Start menu, which you can pin to the taskbar. No admin
rights needed. Add `-Desktop` for a desktop shortcut, `-Uninstall` to remove it again.

If PowerShell refuses to run the script, start it with
`powershell -ExecutionPolicy Bypass -File .\install.ps1`.

The exe is a single self-contained file with its icon built in, so it can also be copied
anywhere and started by double click.

## Build and run

To run straight from the source tree without installing:

```
cargo run --release
```

Or build once and start the exe directly:

```
cargo build --release
target\release\nullshell.exe
```

Other commands:

```
cargo test                                     run the unit tests
nullshell --config path\to\config.toml         use a different config file
nullshell --print-default-config               print the default config
```

## Configuration

The config is read once at startup from

```
%APPDATA%\NullShell\config.toml
```

The file is created with commented defaults on first run; see
[assets/default_config.toml](assets/default_config.toml) for every option. Font, font
size, all colors, shell path and arguments, window opacity, cursor, scrollback size, every
effect strength and the HUD can be changed there. Restart NullShell to apply changes.

If the config has an error, NullShell starts with the defaults and shows the problem in
red in the status bar. Errors and panics are also written to
`%APPDATA%\NullShell\nullshell.log`.

To run something every time a window opens, set `startup_command` under `[shell]`, for
example `startup_command = "fastfetch"`.

To turn the styling down: `[effects] enabled = false` removes the shader effects,
`[hud] enabled = false` removes the header, frame and status bar.

## Keys

| Key | Action |
| --- | --- |
| Ctrl+Shift+C | Copy selection |
| Ctrl+C | Copy if there is a selection, otherwise send Ctrl+C |
| Ctrl+Shift+V, Shift+Insert | Paste |
| Right click | Copy the selection if there is one, otherwise paste |
| Middle click | Paste |
| Drag | Select. Double click selects a word, triple click a line, Alt+drag a block |
| Shift+drag | Select even when a program (nvim, btop) has grabbed the mouse |
| Mouse wheel, Shift+PageUp/PageDown | Scroll through history |
| Ctrl+= / Ctrl+- / Ctrl+0 | Font size up / down / reset |
| F11 | Fullscreen |

## Code layout

```
build.rs         embeds the icon and version info into the exe
install.ps1      per-user install with Start menu shortcut
assets/          default config, app icon
tools/           make_icon.py, regenerates assets/nullshell.ico
src/
  main.rs        entry point, logging, argument parsing
  app.rs         window + event loop, glue between the three parts below
  config.rs      TOML config
  pty.rs         ConPTY, shell process, reader/writer threads
  term.rs        terminal emulation (alacritty_terminal), produces a Frame
  frame.rs       plain-data snapshot of the screen: the only thing render sees
  input.rs       keyboard / mouse / paste encoding
  platform.rs    small Win32 helpers
  render/
    mod.rs       Renderer: layout, frame orchestration
    gpu.rs       wgpu device and swapchain
    scene.rs     Scene (list of quads) and the pipeline that draws it
    glyphs.rs    fonts, cell metrics, glyph atlas
    grid.rs      layer: terminal cells -> quads
    boxdraw.rs   built-in box drawing / block / braille characters
    hud.rs       layer: header, frame, status bar
    post.rs      post-process pass
    shaders/
      scene.wgsl quad shader
      post.wgsl  screen effects
```

`pty`, `term` and `render` do not know about each other. `term` turns shell output into a
`Frame` (cells with resolved colors, cursor, scroll position); `render` draws a `Frame`.

### Where to add things

- **Shader effects**: edit `render/shaders/post.wgsl`. It receives the finished frame as a
  texture plus time, resolution and the effect strengths. New uniforms go in `PostUniforms`
  in `render/post.rs`.
- **Custom visuals**: write a new layer next to `hud.rs`. A layer is a function that pushes
  quads into the `Scene` (`rect`, `cut_rect`, `arc`, `glyph`), called from
  `Renderer::render` in `render/mod.rs`.
- **Custom title bar**: create the window with `.with_decorations(false)` in `app.rs`, give
  the header more height in `Renderer::update_layout`, draw the buttons in `hud.rs`, and
  handle clicks in `app.rs` (`window.drag_window()` for dragging).
- **Tabs**: `app.rs` owns one `Terminal` + `Pty` pair. Make that a list, snapshot the
  active one into the `Frame`, and draw the tab strip in the HUD header.

## Notes

- Animated effects redraw at about 30 fps while the window is focused. Set
  `[effects] animate = false` for a terminal that only redraws when something changes.
- Rounded box corners, double lines, blocks and braille are drawn by NullShell, not the
  font, so TUI borders never show gaps. Dashed box lines are drawn solid.
- Font ligatures are not supported: every cell is shaped on its own to keep the grid exact.

## License

[MIT](LICENSE)
