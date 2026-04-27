# A2 — capturing HolyC compile errors over COM1

## Investigation summary (live, against the running VM)

Goal: stop screenshotting + OCRing the framebuffer to read JIT compile
errors. Get the text into `build/serial-temple.log`.

### Findings

1. **`ExePutS` swallows `'Compiler'` exceptions internally.**
   `Compiler/CMain.HC:589-598` shows ExePutS wraps Lex + ExeCmdLine in
   its own `try { ... } catch { ... }`. On a 'Compiler' or 'Break'
   exception it sets `Fs->catch_except=TRUE` and returns 0. So a host
   `try { ExePutS(buf); } catch { ... }` around it never sees the
   throw — by the time control returns, the exception is already
   consumed. Verified on the live VM:
   ```
   try { ExePutS("zzz garbage @!! ;"); CommPrint(1,"NOERR\n"); }
   catch { CommPrint(1,"CAUGHT\n"); }
   ```
   logs `NOERR`, not `CAUGHT`.

2. **`ExePutS` return value is the last expression's I64 result, not a
   success bool.** It returns 0 on compile failure but also 0 on
   most successful chunks (`F64 x=1.0;` returns 0, the implicit result
   of the assignment). So the return code alone is not a reliable
   compile-success signal.

3. **`Fs->catch_except` is the success signal.** When ExePutS catches
   a 'Compiler' or 'Break' throw it sets `Fs->catch_except=TRUE`
   before returning. Reading and clearing this flag immediately after
   the call gives a per-chunk pass/fail bit:
   ```
   Fs->catch_except=FALSE;
   ExePutS(buf);
   Bool ok = !Fs->catch_except;
   ```

4. **`Fs->put_doc` redirection captures the error text.** TempleOS's
   `PrintErr` ultimately routes through the standard `""` Print path,
   which writes into the calling task's `Fs->put_doc` CDoc. If we
   point `put_doc` at a fresh `DocNew` for the duration of the
   `ExePutS` call, the lexer's `ERROR: ...` lines land in our doc
   instead of adam's framebuffer. Verified live:
   ```
   CDoc *save=Fs->put_doc, *cap=DocNew;
   Fs->put_doc=cap;
   ExePutS("zzz garbage @!! ;");
   Fs->put_doc=save;
   // walk cap->head.next list; DOCT_TEXT entries' tag is the string
   ```
   The capture contains:
   `&LexExcept PrsStmt &LexStmt2Bin &ExeCmdLine ERROR: Undefined identifier at "garbage"  zzz garbage @!! ;`
   The `&Foo` tokens are `$LK,...$` link entries — DolDoc menu links
   that get rendered as the bare label when the `$$` is stripped. Good
   enough for the host log; the parser-error text is intact.

5. **Walking the captured doc.** `CDocEntry` fields used:
   - `e->type_u8 == DOCT_TEXT` (= 0xF000): `e->tag` is the text run.
   - `e->type_u8 == DOCT_NEW_LINE` (= 0xF001): emit `\n`.
   - Everything else (link, color, etc.) we drop. Linked list runs
     `cap->head.next` ... until back to `&cap->head`.

6. **`1e-9` compiles fine on stock TempleOS.** The user's example
   regression input parses; the chunk returns 0 because the F64 result
   of an assignment converts to integer 0. To exercise the error path
   in the test we use an obviously-broken chunk like
   `zzz garbage @!! ;` — `Undefined identifier`.

### Approach picked: best-tier (full text)

Daemon now wraps every `ExePutS` with put_doc redirect + catch_except
sampling, then:

- emits `COMPILE_OK\n` after a clean chunk, or
- emits `COMPILE_ERR_BEGIN\n<captured text>\nCOMPILE_ERR_END\nCOMPILE_FAIL\n`
  after a failed chunk,
- always followed by `D_DONE\n` so the existing wait_for("D_DONE", ...)
  contract is preserved.

Host-side `temple-run.py` watches its per-chunk wait window for
`COMPILE_FAIL`; on hit it auto-runs `scripts/screenshot.sh` and emits
the saved path so the test runner / human sees the framebuffer state
at the moment of failure (in case the captured text is truncated).
