# gwbasic

A GW-BASIC 3.23 environment in Rust: the screen editor, the F1–F10 soft keys, and an interpreter with text and graphics modes. The rules are in [docs/spec.md](docs/spec.md).

The window is the GW-BASIC screen (80×25, an 8×16 font, integer-scaled). It is a winit window with a softbuffer, so the same program runs on Windows, Linux, and macOS. The host console is not the screen. Tones use `kernel32` `Beep` on Windows, `osascript beep` on macOS, and `paplay` on Linux. If the player is missing, the statement still succeeds.

What runs: the editor, the text language (`PRINT` through `WHILE`/`WEND`, types, `INPUT`, `DATA`), `SAVE`/`LOAD`/`MERGE`/`CHAIN` in ASCII, tokenized, and protected form, `CHAIN MERGE` with `ALL` and `DELETE`, sequential files, `PEEK`/`POKE` on one 64K segment, `SOUND`, `ON ERROR`, `SHELL`, `RENUM`, `LSET`/`RSET`, `ENVIRON`, the `MKx$`/`CVx` conversions, `RESET`, `COMMON` across `CHAIN`, and `INPUT$`. Graphics: `SCREEN` (modes 0–2 and 7–13, one page), `PSET`/`PRESET`, `POINT`, `PMAP`, `VIEW`, `WINDOW`, `PALETTE`, and graphics `COLOR`. Still `Illegal function call`: `CIRCLE`, `PAINT`, `DRAW`, `PLAY`, random `FIELD`/`GET`/`PUT`, `CALL`/`USR`, serial, cassette, joystick, and light pen. A graphics `LINE` is a syntax error; `LINE INPUT` is the line-input statement.

## Build and run

```
cargo run
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo run -- examples/hello.bas
```

GitHub Actions runs those three checks on Ubuntu, macOS, and Windows. Ubuntu installs `pkg-config`, `libwayland-dev`, `libxkbcommon-dev`, and `libx11-dev` so winit can link.

`cargo run -- examples/hello.bas` loads that ASCII program and prints the banner. `LIST` shows it. `RUN` executes it. At the `Ok` prompt, `PRINT "Hello"` prints `Hello` and then `Ok`. F3 types `LOAD"` and F4 types `SAVE"`.

A filename with no extension is opened as `.BAS`. A missing file prints `File not found` and stays in the editor. A real tokenized (`FF`) or protected (`FE`) file loads. A corrupt one prints `Illegal function call`.

Useful keys, once the window is up: F1 types `LIST`, F2 runs `RUN`, Esc erases the logical line, Ctrl+Break abandons it. `SYSTEM` closes the window.

## License

MIT. See [LICENSE](LICENSE). The 8×8 face doubled to 8×16 is the public-domain font8x8 basic font (Daniel Hepper / Marcel Sondaar). Byte `0x1B` is drawn as the CP437 left arrow used on the soft-key line.
