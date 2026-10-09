# GW-BASIC 3.23 behavior

This is an original summary of the behavior the emulator follows. It is not a reproduction of Microsoft's manuals. Where the 1986 books and the PC-BASIC notes disagree, follow GW-BASIC 3.23 as PC-BASIC describes it.

## Sources

- Microsoft GW-BASIC Interpreter User's Guide, document 410130001-320-R01-0686 (1986). HTML mirrors: <http://www.antonis.de/qbebooks/gwbasman/> and <http://manmrk.net/tutorials/basic/gwbasic/GW-MAN/default.html>. PDF: <https://bitsavers.org/pdf/microsoft/gw-basic/410130001-320-R01-0686_Microsoft_GW-BASIC_Interpreter_Users_Guide_1986.pdf>. Chapter 2 (startup and the command line), chapter 4 (the editor), chapter 6 (types), appendix A (errors).
- Microsoft GW-BASIC User's Reference, document 410130013-320-R01-0686 (1986). PDF: <https://bitsavers.org/pdf/microsoft/gw-basic/410130013-320-R01-0686_Microsoft_GW-BASIC_Interpreter_Users_Reference_1986.pdf>.
- KEY: <http://www.antonis.de/qbebooks/gwbasman/key.html>. SCREEN: <http://www.antonis.de/qbebooks/gwbasman/screens.html>. Chapter 4 HTML: <http://www.antonis.de/qbebooks/gwbasman/chapter%204.html>.
- PC-BASIC 2.0, Rob Hagemans: <https://robhagemans.github.io/pcbasic/doc/2.0/>. Behavioral oracle only. Its source is GPL-3 and is not copied.
- Keyword and constant tokens, Norman De Forest: <https://chebucto.ns.ca/~af380/GW-BASIC-tokens.html>, checked against a 3.23 decoder. Files store a line-number operand as `0x0E`. `0x0D` is an in-memory pointer and must not be written to disk.
- Protected-program cipher, Stephen Kitt, Retrocomputing Stack Exchange question 7104.
- Microsoft Binary Format: <https://en.wikipedia.org/wiki/Microsoft_Binary_Format>.
- Microsoft's 10 February 1983 8088 sources (<https://github.com/microsoft/GW-BASIC>, MIT) are earlier than 3.23. They may be read. They are not a line-by-line port.

## What runs today

The editor and the text-mode interpreter:

- An 80×25 (or 40×25) text window, not the host terminal. Cells are 8×16. The default window is 1280×800, which is the 640×400 grid at scale 2, letterboxed if the window is another size.
- Startup banner, `Ok`, and the row-25 soft-key strip.
- Logical lines, the editing keys below, and F1–F10.
- Direct commands: `NEW`, `LIST`, `EDIT`, `DELETE`, `AUTO`, `SYSTEM`, `KEY`, `KEY ON`, `KEY OFF`, `KEY LIST`, `RUN`, `CONT`, `TRON`, `TROFF`, `SCREEN 0`, `WIDTH 40|80`, `CLS`, `REM`, and a leading `'`.
- Numbered lines tokenized on Enter. `LIST` prints the detokenized source, so an edit after `LIST` round-trips. `?` is stored and listed as `PRINT`.
- `RUN` executes from the first line, or from a line number. An empty program still prints `Ok`. A filename on the command line loads and does not run.
- `PRINT`, `LPRINT`, `PRINT USING` (the common field forms), and `?`. Numbers are Microsoft Binary Format. A positive number prints with a leading space, and every number prints with a trailing space. A trailing `;` or `,` holds the cursor, so `PRINT "Hello";` is followed by `Ok` on the same line.
- `LET` (optional), `GOTO`, `GOSUB`/`RETURN`, `IF`/`THEN`/`ELSE`, `FOR`/`NEXT`, `WHILE`/`WEND`, `ON n GOTO`/`GOSUB`, `END`, `STOP`, `CONT`, `TRON`/`TROFF`, and `REM`.
- `CLS`, `LOCATE`, `COLOR`, `WIDTH`, and `BEEP`. Text `COLOR` and `LOCATE` only; there is no graphics screen.
- `INTEGER`, `SINGLE`, `DOUBLE`, and `STRING`, plus `DEFINT`/`DEFSNG`/`DEFDBL`/`DEFSTR`, `DEF FN`, arrays, and `OPTION BASE`. `FNA` is the function `FN A`.
- `INPUT`, `LINE INPUT`, and `INKEY$`. `DATA`/`READ`/`RESTORE`.
- A program line that fails to tokenize is not stored. The screen shows `^`, then `Syntax error` or `Overflow`, then `Ok`.
- Ctrl+Break stops a running program (`Break` or `Break in n`, and `CONT` can resume). Ctrl+C does not. At the editor and at `INPUT`, Ctrl+C abandons the line the way Ctrl+Break does.
- `SAVE`, `LOAD`, `MERGE`, and `CHAIN`. `SAVE "f"` writes a tokenized program (`FF`). `SAVE "f",A` writes ASCII (CRLF lines and a trailing `1A`). `SAVE "f",P` writes a protected program (`FE`, Stephen Kitt's cipher). `LOAD` reads all three. `LOAD "f",R` and `CHAIN` run the loaded program. `MERGE` accepts ASCII only.
- A protected program can `RUN`. It cannot be listed, edited, deleted, auto-numbered, or entered as a line. `SAVE` without `,P`, and `PEEK`, `POKE`, `BSAVE`, and `BLOAD`, are `Illegal function call` while it is protected.
- Sequential files: `OPEN` (`FOR INPUT`/`OUTPUT`/`APPEND AS #n`, and `OPEN "O",#1,"file"`), `CLOSE`, `PRINT #`, `WRITE #`, `INPUT #`, `LINE INPUT #`, `EOF`, `LOF`, `LOC`, `KILL`, `NAME`, `FILES`, `CHDIR`, `MKDIR`, and `RMDIR`. File numbers run from 1 to `min(/F, 15)`. The default is 3. Writes use CRLF. `EOF` is −1 at the end and 0 otherwise. `LOF` is the length. `LOC` is the position divided by 128.
- `PEEK` and `POKE` on one 64K array. `DEF SEG` is remembered and written into the `BSAVE` header. Every segment number aliases that same array; this is not a map of video memory. `RUN` and `NEW` do not clear it. `BSAVE`/`BLOAD` use a 7-byte header (`FD`, segment, offset, length) and then the bytes.
- `SOUND freq,duration`. `freq` is 37 to 32767 Hz. `duration` is in 18.2 Hz ticks. `BEEP` and `CHR$(7)` stay 800 Hz for 200 ms. The window thread plays the tone. `PLAY` is `Illegal function call`.
- `ON ERROR GOTO`, `RESUME`, `RESUME NEXT`, `RESUME n`, and `ERROR n`. A second error inside the handler is reported normally. `RESUME` with no error trapped is `RESUME without error`.
- `SHELL`. Windows runs `cmd /C`. Other hosts run `sh -c`. Empty `SHELL` does nothing. The command's output is printed on the BASIC screen. The child sees the interpreter's environment copy, not a later change to the process environment.
- `RENUM [new[,[old[,increment]]]]`. Defaults are 10, the first line, and 10. It rewrites line references after `GOTO`, `GOSUB`, `THEN`, `ELSE`, `ON`…`GOTO`/`GOSUB`, `RESTORE`, and `RESUME`, and integer comparisons with `ERL`. A reference to a missing line prints `Undefined line n in m` and is left unchanged; the renumber still happens. `ON ERROR GOTO 0` stays 0, because that disables trapping. A renumber that would reorder lines, use an increment of 0, or pass 65529 is `Illegal function call` and changes nothing. So is `RENUM` inside a running program, or on a protected program. If `old` names no line, the result is `Undefined line number` and the program is left as it was.
- `LSET` and `RSET` justify a string into the variable's current length. They pad with spaces and truncate. An empty string stays empty. There is no `FIELD` buffer yet, so this applies to string variables.
- `MKI$`/`CVI`, `MKS$`/`CVS`, and `MKD$`/`CVD` convert between numbers and the 2-, 4-, and 8-byte forms (integer, single MBF, double MBF). A string shorter than that width is `Illegal function call`. Extra bytes are ignored.
- `ENVIRON "name=value"` sets a variable in the interpreter's copy of the environment. `ENVIRON "name="` removes it. The name match ignores case. `ENVIRON$` reads it back. This does not change the process environment. `RUN` and `CLEAR` do not clear it.
- `RESET` closes every open file.
- `CHAIN [MERGE] file[,[line][,ALL][,DELETE range]]` loads a program and runs it. `line` is a numeric expression. A negative value is the 16-bit two's complement. If that line is missing after the load, the result is `Illegal function call` and the load still stands. Without `MERGE`, the new program replaces the old one. `MERGE` overlays an ASCII file and keeps `DEFINT`, `DEFSNG`, `DEFDBL`, and `DEFSTR`. A tokenized or protected file is `Bad file mode`. `MERGE` on a protected program in memory is `Illegal function call`. `DELETE start-end` removes that range before the overlay. The end line has to exist, or nothing is deleted and nothing is loaded. `ALL` keeps every variable, array, and `DEF FN`. Without `ALL`, only names recorded by `COMMON` are kept. `OPTION BASE` is kept either way. `LOAD` does not keep variables.
- `COMMON name` and `COMMON name()` name the scalars and arrays that the next `CHAIN` keeps when `ALL` is absent. A scalar and an array of the same name are separate. The names are recorded when `COMMON` runs.
- `INPUT$(n)` and `INPUT$(n,#file)` read `n` characters, with `n` from 1 to 255. From a file, those characters are raw bytes, including CR and LF. If fewer bytes remain, the result is `Input past end` and the file position stays put. From the keyboard, `INPUT$` does not echo. It waits until that many keys are waiting, then the statement runs again. Ctrl+Break and Ctrl+C during the wait stop the program the way they do at `INPUT`.

Still `Illegal function call` at run time, after they tokenize: graphics (`SCREEN` other than 0, `PSET`, `PRESET`, `LINE`, `CIRCLE`, `PAINT`, `DRAW`), `PLAY`, random files (`FIELD`, `GET`, `PUT`), `CALL`/`USR`, and device statements such as `OUT` and `WAIT`. Serial, cassette, joystick, and light pen stay out.

The window is winit and softbuffer, so the editor is the same program on Windows, Linux, and macOS. The tone uses `kernel32` `Beep` on Windows, `osascript beep` on macOS, and `paplay` of the freedesktop bell on Linux. A missing player is ignored. The unit tests never open a window and never play a tone.

## Startup

```
GW-BASIC 3.23
(C) Copyright Microsoft 1983,1984,1985,1986,1987,1988
60300 Bytes free
Ok
```

Reported free space is the workspace minus 5236 bytes of overhead minus the bytes of the stored program, and never goes below 0. The default workspace is 65536, so an empty program reports 60300. Each stored line counts as its token bytes plus 4. The token bytes already include the terminating zero; the extra 4 is the next-line pointer and the line number. `/M:n` sets the workspace, capped at 65536. `/D` makes `SIN`, `COS`, `TAN`, `ATN`, `LOG`, `EXP`, and `SQR` return doubles. It does not change the banner.

Command line, from Guide section 2.3:

```
gwbasic [filename] [/F:n] [/I] [/S:n] [/C:n] [/M:[n][,n]] [/D]
```

Numbers may be decimal, `&H` hex, or `&O` octal. `/F` defaults to 3 and sizes the open-file table, capped at 15, whether or not `/I` is present. `/I` is accepted. `/S` defaults to 128 and is stored for random-file records, which are not implemented. `/C` is accepted and ignored until serial I/O exists. `/D` makes `SIN`, `COS`, `TAN`, `ATN`, `LOG`, `EXP`, and `SQR` return doubles. It does not change the banner. A second filename is an error.

A filename with no extension gets `.BAS`. The file is loaded before the banner, so the free-space line includes it. A missing file stays in the editor and prints `File not found`. A file that starts with `FF` or `FE` is loaded as a tokenized or protected program. A truncated or otherwise illegal tokenized file prints `Illegal function call` and does not load. ASCII load skips a UTF-8 BOM and a trailing `1A`, and blank lines. Every other line must start with a line number.

`SYSTEM` closes the window and does not print `Ok`.

## The screen is the line

There is no separate line buffer. The cells under the cursor are the line being entered.

- A logical line is at most 255 characters. Further typing is ignored.
- Typing past the right edge wraps. That physical row is marked as continuing, so the next row is the same logical line. Ctrl+Enter (Ctrl+J) joins rows the same way. A join that would pass 255 characters is refused.
- After a row is filled exactly, the cursor sits on a blank continuation row. Enter in that state does not skip an extra line: the continuation flag is cleared and command output starts on that blank row.
- Enter (Ctrl+M) submits the whole logical line, even if the cursor is at the start or in the middle. Trailing spaces are not part of the text.
- The first non-space token, if it is a number from 0 to 65529, makes the line a program line. It replaces any stored line with that number. No `Ok` is printed. Leading zeros are canonical (`00010` is stored as `10`). Leading spaces before the number are dropped. The characters after the digits, including one separating space, are kept.
- A bare line number deletes that line. No `Ok`.
- A number above 65529 is `Syntax error`.
- Anything else is direct mode. A blank Enter prints `Ok`. A direct `REM`, or a line whose first non-space character is `'`, prints `Ok` and does nothing else.
- Changing the line number on screen stores the new number and does not delete the old one.
- `.` means the last line entered or edited, for `EDIT`, `LIST`, `DELETE`, and `AUTO`. If no line has been entered, `.` is `Undefined line number`.

Row 25 (index 24) is the soft-key strip while `KEY ON` (the default). It is not a place you type. Down and Right stop on row 24. `KEY OFF` makes all 25 rows editable and hides the strip. `KEY ON` shows it again.

`CLS`, `SCREEN 0`, and `WIDTH` clear the editable rows and home the cursor. `WIDTH 40` or `WIDTH 80` also rebuilds the grid. Any other width, and any `SCREEN` mode other than 0, is `Illegal function call`. `SCREEN 0` accepts the extra numeric arguments F10 sends (`SCREEN 0,0,0`).

Ctrl+Home clears the screen and homes the cursor. The program store is unchanged. Home moves to the top left and does not clear.

## Editing keys

From User's Guide chapter 4, with PC-BASIC's notes where the manual is thin.

| Key | Action |
| --- | --- |
| Arrows, or Ctrl+6 / Ctrl+- / Ctrl+] / Ctrl+\ | Move. The left edge wraps to the previous row except on row 1. The right edge wraps except on the last editable row. Arrow keys leave insert mode. |
| Ctrl+Left / Ctrl+B, Ctrl+Right / Ctrl+F | Previous or next word. A word is a run of `A–Z`, `a–z`, or `0–9`. These also leave insert mode. |
| Backspace / Ctrl+H | Delete the character left of the cursor and shift the rest of the logical line left, including across wrapped rows. At column 0 this is Delete: it does not eat the last character of the previous row. |
| Del / Ctrl+Backspace | Delete the character under the cursor and shift the rest of the logical line left. |
| Ins / Ctrl+R | Toggle insert mode. The cursor is drawn as a lower half-block. Typed characters push the rest of the line right. |
| Tab / Ctrl+I | Next stop. Stops are every 8 columns. Overwrite mode jumps. Insert mode inserts spaces. |
| Esc / Ctrl+[ | Erase the whole logical line and pull the rows below it up. |
| Ctrl+End / Ctrl+E | Erase from the cursor to the end of the logical line. |
| End / Ctrl+N | Move to the first column after the last non-space of the logical line. |
| Home / Ctrl+K | Cursor to the top left. The screen is unchanged. |
| Ctrl+Home / Ctrl+L | Clear the screen and home the cursor. |
| Ctrl+Enter / Ctrl+J | Column 1 of the next row, joined to this logical line. |
| Ctrl+Break | Abandon the line. Do not store it. Leave `AUTO`. Print `Ok`. |
| Ctrl+C | Same as Ctrl+Break at the editor (and, later, at `INPUT`). It does not interrupt a running program. PC-BASIC's default differs; this emulator follows GW-BASIC. |
| Ctrl+G | Beep (800 Hz, 200 ms, off the window thread). |
| Pause / Ctrl+Num Lock / Ctrl+S | Pause until another key. That second key is consumed and not delivered. |
| Ctrl+PrtSc | Toggle copying later screen output to an in-memory `LPT1:` buffer. |
| Shift+PrtSc, or Print Screen | Append the screen to that buffer. The printer device itself is later. |

Ctrl+Pause and Ctrl+Scroll Lock are also Ctrl+Break, because many keyboards have no Break key. That is an accommodation. Ctrl+C remains "not a break while a program is running."

The physical key is used for Ctrl+letter chords, so Ctrl+M is Enter and Ctrl+Enter is the line join. Windows often reports both as the Enter key.

Alt is ignored for now. It is reserved for `KEY` traps.

## Soft keys

F1–F10 inject up to 15 bytes into the keyboard buffer. They are not a hidden command hook, so the same bytes will later reach `INPUT` and `LINE INPUT`. A trailing CR (byte 13) submits the line. On the strip that CR is drawn as the CP437 left arrow, byte `0x1B`. A macro byte of `0x1B` itself is the arrow glyph, not Esc. Only the Esc key erases a line.

| Key | Bytes |
| --- | --- |
| F1 | `LIST` |
| F2 | `RUN` + CR |
| F3 | `LOAD"` |
| F4 | `SAVE"` |
| F5 | `CONT` + CR |
| F6 | `,"LPT1:"` + CR |
| F7 | `TRON` + CR |
| F8 | `TROFF` + CR |
| F9 | `KEY` |
| F10 | `SCREEN 0,0,0` + CR |

The 80-column strip is ten slots of 8 columns: a digit (`1`–`9`, and `0` for F10), the first six display bytes, and a trailing space. Width 40 shows five slots. Ctrl+T scrolls which key is leftmost, and only when the width is under 80.

`KEY n,"string"` replaces a macro and truncates it to 15 bytes. Pieces may be quoted strings or `CHR$(n)` joined by `+`. Quotes have no backslash escapes. `CHR$` is case-insensitive. A null string disables that soft key. `n` outside 1–10 is `Illegal function call`. `KEY` with no argument is `Syntax error` (F9 does not include a CR). `KEY ON` and `KEY OFF` show or hide the strip. `KEY LIST` prints all ten macros.

`KEY(n) ON/OFF/STOP` is a different statement (event traps) and is not implemented.

F6 and a bare `KEY` or `LOAD"` are supposed to be finished by the user. Pressed alone, F6 and F9 produce `Syntax error` after Enter, which is what GW-BASIC does at the `Ok` prompt. F2, F5, F7, F8, and F10 include a CR, so they run immediately. F5's `CONT` reports `Can't continue` when nothing is suspended. F7 and F8 set and clear the trace flag. While it is set, each program line prints `[n]` before its output.

## Direct commands implemented now

`LIST` and `DELETE` take `n`, `n-m`, `n-`, `-m`, or `.`, with optional spaces. `LIST` with no arguments lists everything. `DELETE` with no arguments is `Syntax error`. A syntactically valid empty range still prints `Ok`. `LIST10` works: the command is the leading run of letters.

`EDIT n` prints the stored line and leaves the cursor on the first character of the statement (after the digits and one following space). It does not print `Ok`. A missing line is `Undefined line number`. Extra tokens after the number are `Illegal function call`. `EDIT` with no number edits the last line.

`AUTO [start][,step]` defaults to 10,10. The counter, not a number the user retypes, chooses the next prompt. An existing line is shown as `n*` with the cursor on the asterisk, in overwrite mode, so the first typed character replaces it. Enter on `n`, on `n*`, or on a blank prompt skips that number and does not delete the stored line. Step 0 is `Illegal function call`. A start above 65529 is `Syntax error`. When the counter passes 65529, AUTO ends and prints `Ok`. Ctrl+Break leaves AUTO, does not store the line, and prints `Ok`.

`NEW` clears the program and prints `Ok`. The screen stays. `CLS` clears only the screen.

Errors used here: `Syntax error`, `Undefined line number`, `Illegal function call`, `Can't continue`, `File not found`. A line that does not tokenize prints `^`, then `Syntax error` or `Overflow`, then `Ok`, and is not stored. A runtime error during `RUN` appends ` in n` for the line that failed.

## Characters

The window draws an 8×16 face. Glyphs 0x20–0x7F are the public-domain font8x8 basic face (Daniel Hepper / Marcel Sondaar), doubled vertically, so the repo can stay MIT. Byte `0x1B` is a left arrow (`[0x00, 0x04, 0x0C, 0x1F, 0x0C, 0x04, 0x00, 0x00]`). In these bitmaps the low bit is the leftmost pixel. It replaces the blank control glyph. Other bytes below 0x20 are blank. Bytes 0x80–0xFF are framed placeholders until a real CP437 face is needed for graphics.

The default attribute is `0x07` (light gray on black): low nibble foreground, high nibble background, VGA text palette. The soft-key strip is `0x70` (black on light gray) and is drawn over row 25 rather than stored in the cell buffer. The cursor inverts the cell under it, or only the lower eight pixel rows while insert mode is on.

## Language rules

The text rules below are what the interpreter runs. Tokenized and protected programs use the on-disk format and the cipher in this section.

Tokenization happens on Enter, not while typing. `LIST` will print keywords in uppercase. Quoted strings, and the text of `REM` and `DATA`, keep their case. `'` is stored as the token sequence for `:REM'`. `WHILE` is followed by a hidden byte that listings normally hide.

Types: `INTEGER` (`%`, −32768..32767), `SINGLE` (`!`, MBF 32-bit), `DOUBLE` (`#`, MBF 64-bit), `STRING` (`$`, at most 255). The default type is single. `DEFINT` / `DEFSNG` / `DEFDBL` / `DEFSTR` set the default by initial letter. `/D` makes `SIN`, `COS`, `TAN`, `ATN`, `LOG`, `EXP`, and `SQR` return doubles. Numbers are Microsoft Binary Format from the first expression evaluator, not IEEE `f64`. The exponent sits in the high byte with bias 128. The sign is the high bit of the next byte. The leading 1 of the significand is not stored. There are no NaNs, infinities, or denormals. De Forest's bit patterns to test: 0.5, −0.5, 1, 2, 7.75.

Operators, high to low: `^`, unary `-`, `*` `/`, `\`, `MOD`, `+` `-`, relations (`=` `<>` `<` `>` `<=` `>=`) which yield −1 or 0, then `NOT`, `AND`, `OR`, `XOR`, `EQV`, `IMP`.

On disk, a tokenized program starts with `FF`, or `FE` if protected. Each line is a `u16` next-line offset, a `u16` line number, token bytes, and `00`. A `0000` offset ends the program. The file ends with `1A`. Offsets are recomputed on load. Keyword tokens are one byte `0x81`–`0xF4`, or two bytes after `FD`, `FE`, or `FF`. Constants: `0x11`–`0x1B` are 0..10; `0x0F` plus a byte; `0x1C` plus a signed i16; `0x0B` octal i16; `0x0C` hex i16; `0x0E` plus a `u16` line number in files; `0x0D` plus a `u16` is the in-memory line pointer and must not appear in files; `0x1D` is a 4-byte MBF single; `0x1F` is an 8-byte MBF double.

Protected files use Stephen Kitt's cipher. Key 1 is the 13 bytes `9A F7 19 83 24 63 43 83 75 CD 8D 84 A9`. Key 2 is the 11 bytes `7C 88 59 74 E0 97 26 77 C4 1D 1E`. The counters start at `0x0D` and `0x0B`.

`CALL` and `USR` stay `Illegal function call` until there is a reason to embed an 8086 emulator. Serial, cassette, joystick, and light pen stay unimplemented.
