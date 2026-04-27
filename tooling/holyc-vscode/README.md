# HolyC for VSCode

Syntax highlighting for HolyC / ZealC source files (`.ZC`, `.HC`, `.HH`).

This is a local extension — there is no marketplace publish step. Two
install paths:

## Quick install (symlink)

```sh
# from the repo root:
ln -s "$(pwd)/tooling/holyc-vscode" ~/.vscode/extensions/local.holyc-0.1.0
```

Restart VSCode. Open any `.ZC` file. The mode line should show "HolyC".

## Install via VSIX

```sh
cd tooling/holyc-vscode
npx @vscode/vsce package        # produces holyc-0.1.0.vsix
code --install-extension holyc-0.1.0.vsix
```

## What it highlights

- Primitive types: `U0`, `U8`..`U64`, `I0`..`I64`, `F64`, `Bool`
- ZealOS class types: any `C[A-Z]…` identifier (`CFifoU8`, `CBlkDev`, `CTCPSocket`)
- Control flow: `if`, `else`, `while`, `for`, `do`, `switch`, `case`,
  `default`, `break`, `continue`, `goto`, `return`, `try`, `catch`,
  `throw`, plus HolyC's sub-switch `start` / `end`
- Storage modifiers: `extern`, `public`, `static`, `interrupt`,
  `haserrcode`, `argpop`/`noargpop`, `reg`/`noreg`, `no_warn`,
  `lastclass`, `lock`
- Other keywords: `class`, `union`, `sizeof`, `offset`, `asm`
- Constants: `TRUE`, `FALSE`, `NULL`, `ON`, `OFF`
- Standard library / kernel functions used in this repo: `Print`,
  `Sleep`, `Spawn`, `Sys`, `MAlloc`, `Free`, `CommInit8n1`, `CommPrint`,
  `FifoU8Remove`, `BlkDevAdd`, `AHCIPortInit`, `TCPSocketListen`,
  `ExeFile`, `TaskExe`, etc.
- Strings with `\X` C escapes and `$$` DolDoc literal-`$` escape
- Char literals (HolyC allows multi-char: `'ABC'`)
- Hex / decimal / float numbers
- `#include`, `#define`, `#ifdef`, etc. preprocessor directives
- `asm { … }` blocks with label recognition

## What it does NOT do

- Type-aware semantic highlighting. This is a TextMate grammar — regex,
  not a parser. Function-vs-variable disambiguation is purely lexical.
- Linting / diagnostics. The boot-phase quirks documented in
  `NOTES.md` (`return` at top level, `for` body with type decls, etc.)
  are not flagged here. If you want that, the natural place is a
  separate VSCode language server, or push the file through the live
  REPL via `make repl` + `scripts/zpush.sh` for the real compiler's
  verdict.

## Theming

This extension only assigns scope names. Colors come from your VSCode
theme. The grammar uses standard scopes (`storage.type.holyc`,
`keyword.control.holyc`, `support.function.builtin.holyc`, etc.) so any
mainstream theme will paint sensibly.
