# HolyC for Neovim / Vim

Syntax highlighting for HolyC / ZealC source files (`.ZC`, `.HC`, `.HH`).

Native Vim regex syntax — no plugin manager required, no tree-sitter
build step, no external dependencies. Loads in any Vim ≥ 7.4 and any
Neovim.

## Install (Neovim, native package)

```sh
mkdir -p ~/.config/nvim/pack/local/start
ln -s "$(pwd)/tooling/holyc-nvim" ~/.config/nvim/pack/local/start/holyc
```

Restart Neovim. Open any `.ZC` file. `:set filetype?` should show
`filetype=holyc`.

## Install (Vim 8+, native package)

```sh
mkdir -p ~/.vim/pack/local/start
ln -s "$(pwd)/tooling/holyc-nvim" ~/.vim/pack/local/start/holyc
```

## Install (lazy.nvim / packer)

Point your plugin manager at `tooling/holyc-nvim/` as a local plugin.
For lazy.nvim:

```lua
{ dir = "/path/to/templeos-devkit/tooling/holyc-nvim", ft = "holyc" }
```

## What it highlights

Same coverage as the VSCode extension — types (`U0`/`U8`/…/`F64`/`Bool`,
plus ZealOS class types `C[A-Z]…`), control flow keywords including
sub-switch `start`/`end`, storage modifiers (`extern`, `public`,
`interrupt`, `haserrcode`, `lastclass`, `lock`, …), DolDoc `$$` escape
inside strings, multi-char literals, hex/decimal/float numbers,
`#include`/`#define`/etc. preprocessor, and the kernel/stdlib functions
this repo uses (`Print`, `Spawn`, `Sys`, `CommPrint`, `FifoU8Remove`,
`AHCIPortInit`, …).

All groups are linked to standard Vim highlight groups (`Type`,
`Keyword`, `String`, `Function`, etc.), so any colorscheme works.

## What it does NOT do

- No semantic analysis — this is regex syntax, not tree-sitter.
- No diagnostics. The boot-phase quirks documented in `NOTES.md` are
  not flagged. For ground-truth checks, push the file through `make
  repl` + `scripts/zpush.sh` and read the compiler's verdict from
  `build/serial.log`.

## Tree-sitter alternative (not provided)

If you want semantic highlighting later, the path is to write a
`tree-sitter-holyc` grammar and register it via `nvim-treesitter`. Much
bigger lift — needs a real grammar in `grammar.js`, `tree-sitter
generate`, and a queries file. Out of scope for this dev kit.
