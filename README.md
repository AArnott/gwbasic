# gwbasic

A GW-BASIC 3.23 environment in Rust: the screen editor, the F1–F10 soft keys, and a text-mode interpreter. Graphics, files, and tokenized `SAVE`/`LOAD` come later. The rules are in [docs/spec.md](docs/spec.md).

The window is the GW-BASIC screen (80×25, an 8×16 font, integer-scaled). The host console is not.

## Build and run

```
cargo run
cargo test
cargo run -- examples/hello.bas
```

`cargo run -- examples/hello.bas` loads that ASCII program and prints the banner. `LIST` shows it. `RUN` executes it. At the `Ok` prompt, `PRINT "Hello"` prints `Hello` and then `Ok`.

A filename with no extension is opened as `.BAS`. If the file is missing, or if it is a tokenized `FF`/`FE` program, the editor stays open and prints the error. Tokenized `LOAD`/`SAVE` is later.

Useful keys, once the window is up: F1 types `LIST`, F2 runs `RUN`, Esc erases the logical line, Ctrl+Break abandons it. `SYSTEM` closes the window.

## License

MIT. See [LICENSE](LICENSE). The 8×8 face doubled to 8×16 is the public-domain font8x8 basic font (Daniel Hepper / Marcel Sondaar). Byte `0x1B` is drawn as the CP437 left arrow used on the soft-key line.
