#!/usr/bin/env python3
"""Generate the corpus tree under holyc-parser/tests/corpus/.

Each entry is (filename_short, category, body, expect_pass).
Numbering is auto-assigned per directory in insertion order.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "holyc-parser/tests/corpus"
PASS = ROOT / "passing"
FAIL = ROOT / "failing"

# (short_name, body, expect_pass [True/False], category_tag)
# Each body is a single self-contained HolyC chunk. To prevent
# name collisions across multiple snippets pushed into the SAME
# daemon (the daemon has one global namespace), we use unique
# identifiers per snippet. Format: variable / type / fn names
# include the snippet's tag.
ENTRIES = []

def add(name, body, ok, cat):
    ENTRIES.append((name, body, ok, cat))

# ====================================================================
# CATEGORY: Atom — integer literals
# ====================================================================
add("atom-int-decimal",       "I64 a_dec = 42;", True, "atom")
add("atom-int-decimal-zero",  "I64 a_zero = 0;", True, "atom")
add("atom-int-decimal-neg",   "I64 a_neg = -1;", True, "atom")
add("atom-int-hex-lower",     "I64 a_hex_l = 0xdeadbeef;", True, "atom")
add("atom-int-hex-upper",     "I64 a_hex_u = 0xDEADBEEF;", True, "atom")
add("atom-int-hex-mixed",     "I64 a_hex_m = 0xCafe;", True, "atom")
add("atom-int-binary",        "I64 a_bin = 0b1011;", True, "atom")
add("atom-int-u64-large",     "U64 a_u64 = 0xFFFFFFFFFFFFFFFF;", True, "atom")
# ====================================================================
# CATEGORY: Atom — float literals
# ====================================================================
add("atom-float-plain",       "F64 a_fp = 3.14;", True, "atom")
add("atom-float-leading-dot", "F64 a_fld = .5;", True, "atom")
add("atom-float-trailing-dot","F64 a_ftd = 5.;", True, "atom")
add("atom-float-pos-exponent","F64 a_fpe = 1e9;", True, "atom")
add("atom-float-pos-exponent-cap","F64 a_fpec = 1E9;", True, "atom")
add("atom-float-pos-exponent-mantissa", "F64 a_fpem = 1.5e3;", True, "atom")
# ====================================================================
# CATEGORY: Atom — char literals
# ====================================================================
add("atom-char-1byte",        "I64 a_c1 = 'A';", True, "atom")
add("atom-char-2byte",        "I64 a_c2 = 'AB';", True, "atom")
add("atom-char-4byte",        "I64 a_c4 = 'ABCD';", True, "atom")
add("atom-char-8byte",        "U64 a_c8 = 'ABCDEFGH';", True, "atom")
add("atom-char-escape-newline","I64 a_cne = '\\n';", True, "atom")
add("atom-char-escape-tab",   "I64 a_cte = '\\t';", True, "atom")
add("atom-char-escape-zero",  "I64 a_cz = '\\0';", True, "atom")
add("atom-char-escape-bs",    "I64 a_cbs = '\\\\';", True, "atom")
add("atom-char-escape-quote", "I64 a_cq = '\\'';", True, "atom")
# ====================================================================
# CATEGORY: Atom — string literals + escapes
# ====================================================================
add("atom-string-empty",      'U8 *a_se = "";', True, "atom")
add("atom-string-plain",      'U8 *a_sp = "hello";', True, "atom")
add("atom-string-escape-newline", 'U8 *a_sn = "line\\n";', True, "atom")
add("atom-string-escape-tab", 'U8 *a_st = "col\\tx";', True, "atom")
add("atom-string-escape-quote", 'U8 *a_sq = "say \\"hi\\"";', True, "atom")
add("atom-string-escape-bs",  'U8 *a_sbs = "C:\\\\path";', True, "atom")
# ====================================================================
# CATEGORY: Atom — identifiers
# ====================================================================
add("atom-ident-underscore",  "I64 _a_iu = 1;", True, "atom")
add("atom-ident-mixed-case",  "I64 a_iMixed = 2;", True, "atom")
add("atom-ident-with-digits", "I64 a_i_3_d4 = 3;", True, "atom")

# ====================================================================
# CATEGORY: Expressions — binary operators by precedence
# ====================================================================
add("expr-bin-add",           "I64 e_add = 1 + 2;", True, "expr")
add("expr-bin-sub",           "I64 e_sub = 5 - 2;", True, "expr")
add("expr-bin-mul",           "I64 e_mul = 3 * 4;", True, "expr")
add("expr-bin-div",           "I64 e_div = 10 / 3;", True, "expr")
add("expr-bin-mod",           "I64 e_mod = 10 % 3;", True, "expr")
add("expr-bin-shl",           "I64 e_shl = 1 << 4;", True, "expr")
add("expr-bin-shr",           "I64 e_shr = 32 >> 2;", True, "expr")
add("expr-bin-and",           "I64 e_band = 0xFF & 0x0F;", True, "expr")
add("expr-bin-or",            "I64 e_bor  = 0x10 | 0x01;", True, "expr")
add("expr-bin-xor",           "I64 e_bxor = 0xFF ^ 0x0F;", True, "expr")
add("expr-bin-power",         "I64 e_pow = 2 ` 8;", True, "expr")
add("expr-bin-eq",            "I64 e_eq  = 1 == 1;", True, "expr")
add("expr-bin-neq",           "I64 e_neq = 1 != 2;", True, "expr")
add("expr-bin-lt",            "I64 e_lt  = 1 < 2;", True, "expr")
add("expr-bin-le",            "I64 e_le  = 1 <= 2;", True, "expr")
add("expr-bin-gt",            "I64 e_gt  = 2 > 1;", True, "expr")
add("expr-bin-ge",            "I64 e_ge  = 2 >= 1;", True, "expr")
add("expr-bin-andand",        "I64 e_aa  = 1 && 1;", True, "expr")
add("expr-bin-oror",          "I64 e_oo  = 0 || 1;", True, "expr")
add("expr-bin-xorxor",        "I64 e_xx  = 1 ^^ 0;", True, "expr")
add("expr-chained-cmp",       "I64 e_cc  = 1 < 5 < 10;", True, "expr")
add("expr-prec-shl-add",      "I64 e_psa = 1 + 2 << 3;", True, "expr")
add("expr-prec-power-mul",    "I64 e_ppm = 2 * 3 ` 2;", True, "expr")

# Unary forms
add("expr-un-neg",            "I64 eu_neg = -5;", True, "expr")
add("expr-un-plus",           "I64 eu_pos = +5;", True, "expr")
add("expr-un-not",            "I64 eu_not = !1;", True, "expr")
add("expr-un-com",            "I64 eu_com = ~0;", True, "expr")
add("expr-un-preinc",         "I64 eu_pri=0; ++eu_pri;", True, "expr")
add("expr-un-predec",         "I64 eu_prd=10; --eu_prd;", True, "expr")
add("expr-un-postinc",        "I64 eu_poi=0; eu_poi++;", True, "expr")
add("expr-un-postdec",        "I64 eu_pod=10; eu_pod--;", True, "expr")
add("expr-un-addr-deref",     "I64 eu_a=1; I64 *eu_p=&eu_a; I64 eu_d=*eu_p;", True, "expr")

# Assignment compound
add("expr-assign",            "I64 ea_a=0; ea_a = 5;", True, "expr")
add("expr-add-assign",        "I64 ea_b=1; ea_b += 2;", True, "expr")
add("expr-sub-assign",        "I64 ea_c=5; ea_c -= 1;", True, "expr")
add("expr-mul-assign",        "I64 ea_d=2; ea_d *= 3;", True, "expr")
add("expr-div-assign",        "I64 ea_e=10; ea_e /= 2;", True, "expr")
add("expr-mod-assign",        "I64 ea_f=10; ea_f %= 3;", True, "expr")
add("expr-and-assign",        "I64 ea_g=0xF; ea_g &= 0x3;", True, "expr")
add("expr-or-assign",         "I64 ea_h=0; ea_h |= 0x1;", True, "expr")
add("expr-xor-assign",        "I64 ea_i=0xF; ea_i ^= 0x3;", True, "expr")
add("expr-shl-assign",        "I64 ea_j=1; ea_j <<= 4;", True, "expr")
add("expr-shr-assign",        "I64 ea_k=16; ea_k >>= 2;", True, "expr")

# Sizeof / offset / defined
add("expr-sizeof-type",       "I64 e_sof = sizeof(I64);", True, "expr")
add("expr-sizeof-var",        "I64 e_sov_x=0; I64 e_sov = sizeof(e_sov_x);", True, "expr")
add("expr-defined-true",      "#define E_DEF_X 1\nI64 e_def = defined(E_DEF_X);", True, "expr")
add("expr-defined-false",     "I64 e_ndef = defined(NOT_DEFINED_E);", True, "expr")
add("expr-postfix-typecast",  "F64 e_pcv = 5.0; I64 e_pc = e_pcv(I64);", True, "expr")
add("expr-paren",             "I64 e_pn = (1+2)*3;", True, "expr")

# Function call
add("expr-fun-call-noargs",   "U0 EFnA() { return; }\nEFnA();", True, "expr")
add("expr-fun-call-args",     "I64 EFnB(I64 x, I64 y) { return x+y; }\nI64 e_fc=EFnB(1,2);", True, "expr")

# ====================================================================
# CATEGORY: Statements
# ====================================================================
add("stmt-empty",             "U0 SE_emp() { ; }", True, "stmt")
add("stmt-block",             "U0 SE_blk() { { I64 a=1; } }", True, "stmt")
add("stmt-if",                "U0 SE_if() { I64 x=1; if (x) x=2; }", True, "stmt")
add("stmt-if-else",           "U0 SE_ife() { I64 x=1; if (x) x=2; else x=3; }", True, "stmt")
add("stmt-if-block",          "U0 SE_ifb() { I64 x=1; if (x) { x=2; x++; } }", True, "stmt")
add("stmt-while",             "U0 SE_whl() { I64 i=0; while (i<3) i++; }", True, "stmt")
add("stmt-do-while",          "U0 SE_dw()  { I64 i=0; do { i++; } while (i<3); }", True, "stmt")
add("stmt-for",               "U0 SE_for() { I64 i; for (i=0; i<3; i++) ; }", True, "stmt")
# SURPRISE: parse-spec §5.4 implied `for (I64 i=...)` works at fn scope
# but TempleOS rejects it ("Missing ';'"). It only works as a
# pre-declared `I64 i; for (i=0; ...)`. Documented in surprises.md.
add("surprise-for-decl-fnscope","U0 SE_ford() { for (I64 i=0; i<3; i++) ; }", False, "surprises")
add("stmt-switch-bracket",    "U0 SE_swb() { I64 x=1; switch [x] { case 1: break; } }", True, "stmt")
add("stmt-switch-paren",      "U0 SE_swp() { I64 x=1; switch (x) { case 1: break; case 2: break; default: break; } }", True, "stmt")
add("stmt-switch-case-range", "U0 SE_swr() { I64 x=1; switch (x) { case 1...3: break; default: break; } }", True, "stmt")
add("stmt-switch-case-auto",  "U0 SE_swa() { I64 x=2; switch (x) { case 1: break; case : break; case : break; default: break; } }", True, "stmt")
add("stmt-switch-dft",        "U0 SE_swd() { I64 x=1; switch (x) { case 1: break; dft: break; } }", True, "stmt")
add("stmt-switch-substart",   "U0 SE_sws() { I64 x=1; switch [x] { start: case 1: break; end: case 2: break; } }", True, "stmt")
add("stmt-break",             "U0 SE_brk() { while (1) break; }", True, "stmt")
add("stmt-return-void",       "U0 SE_retv() { return; }", True, "stmt")
add("stmt-return-val",        "I64 SE_retn() { return 42; }", True, "stmt")
add("stmt-goto-label",        "U0 SE_gl()  { goto SE_GL_l; SE_GL_l: ; }", True, "stmt")
add("stmt-try-catch",         "U0 SE_tc()  { try { I64 x=1; } catch { I64 y=2; } }", True, "stmt")
add("stmt-asm-block",         "U0 SE_asm() { asm { NOP } }", True, "stmt")
add("stmt-lock",              "I64 SE_lockv = 0; lock SE_lockv++;", True, "stmt")
add("stmt-no-warn",           "U0 SE_nw() { I64 nw_unused; no_warn nw_unused; }", True, "stmt")

# ====================================================================
# CATEGORY: Declarations
# ====================================================================
add("decl-scalar",            "I64 d_sc = 0;", True, "decl")
add("decl-pointer",           "U8 *d_pt = NULL;", True, "decl")
add("decl-pointer-pointer",   "U8 **d_pp = NULL;", True, "decl")
add("decl-array",             "I64 d_ar[8];", True, "decl")
add("decl-multi-array",       "I64 d_mar[4][8];", True, "decl")
add("decl-array-init-aggr",   "I64 d_ai[3] = {1,2,3};", True, "decl")
add("decl-scalar-init",       "I64 d_si = 7;", True, "decl")
add("decl-extern",            "extern I64 d_ext;", True, "decl")
add("decl-static-fn",         "I64 d_sf_count() { static I64 n=0; return ++n; }", True, "decl")
add("decl-fn-proto",          "I64 d_fnp(I64 x);", True, "decl")
add("decl-fn-def",            "I64 d_fnd(I64 x) { return x*2; }", True, "decl")
add("decl-fn-no-args",        "U0 d_fn0() { ; }", True, "decl")
add("decl-fn-default-arg",    "I64 d_fdef(I64 x = 5) { return x; }", True, "decl")
add("decl-class",             "class CDeclA { I64 x; I64 y; };", True, "decl")
add("decl-class-base",        "class CDeclB { I64 a; }; class CDeclC : CDeclB { I64 b; };", True, "decl")
add("decl-union",             "union UDecl { I64 i; F64 f; };", True, "decl")
add("decl-public",            "public I64 d_pub() { return 1; }", True, "decl")
add("decl-multi-var-fnscope", "U0 D_mvfn() { I64 a, b, c; a=b=c=0; }", True, "decl")

# ====================================================================
# CATEGORY: BUG-COMPAT — must FAIL on TempleOS (parse-spec §5)
# ====================================================================
# §5.1 — return at file scope
add("bug51-toplevel-return",  "return;", False, "bug-compat")
# §5.1 — label at file scope
add("bug51-toplevel-label",   "BUG51_L: ;", False, "bug-compat")
# §5.4 — for(I64 i...) at file scope
add("bug54-for-decl-filescope","for (I64 i_b54 = 0; i_b54 < 3; i_b54++) ;", False, "bug-compat")
# §5.5 — do/while without trailing ;
add("bug55-dowhile-missing-semi","U0 B55_f() { I64 i=0; do { i++; } while (i<3) }", False, "bug-compat")
# §5.6 — chained typecasts via grouping parens
# A C-programmer-style nested cast: not portable.
add("bug56-nested-typecast",  "I64 b56_x=1; F64 b56_y = ((F64)b56_x);", False, "bug-compat")
# §5.9 — C-style prefix cast
add("bug59-c-style-cast",     "F64 b59_y=1.0; I64 b59_x = (I64)b59_y;", False, "bug-compat")
# §5.2 — `continue` in fn
add("bug52-continue-keyword", "U0 B52_f() { I64 i=0; while (i<10) { i++; if (i>5) continue; } }", False, "bug-compat")
# §5.7 — &Internal_Fun
add("bug57-addr-of-internal", "I64 *b57_p = &Sqr;", False, "bug-compat")
# Negative-control: ternary doesn't exist
add("bug-ternary-not-supported","I64 bt_x=1; I64 bt_y = bt_x ? 1 : 2;", False, "bug-compat")
# Unbalanced parens
add("err-unbalanced-paren",   "I64 eup = (1 + 2;", False, "errors")
# Missing semi
add("err-missing-semi",       "I64 ems = 1", False, "errors")
# Invalid lvalue
add("err-invalid-lval",       "U0 EIL_f() { 1 = 2; }", False, "errors")
# Use of undeclared identifier
add("err-undecl-ident",       "I64 eud = some_undecl_ident_xx;", False, "errors")
# Empty for cond is mandatory per spec §3.1
add("err-for-empty-cond",     "U0 EFC_f() { for (;;) ; }", False, "errors")
# duplicate label
add("err-dup-label",          "U0 EDL_f() { EDL_a: ; EDL_a: ; }", False, "errors")
# Reserved keyword as ident
# break outside loop
add("err-break-outside-loop", "U0 EBOL_f() { break; }", False, "errors")

# ====================================================================
# CATEGORY: more atoms — type variety
# ====================================================================
add("atom-type-i8",           "I8 ai_8 = 1;", True, "atom")
add("atom-type-i16",          "I16 ai_16 = 1;", True, "atom")
add("atom-type-i32",          "I32 ai_32 = 1;", True, "atom")
add("atom-type-u8",           "U8 au_8 = 1;", True, "atom")
add("atom-type-u16",          "U16 au_16 = 1;", True, "atom")
add("atom-type-u32",          "U32 au_32 = 1;", True, "atom")
add("atom-type-u64",          "U64 au_64 = 1;", True, "atom")
add("atom-type-bool",         "Bool ab_b = TRUE;", True, "atom")
add("atom-type-bool-false",   "Bool ab_b2 = FALSE;", True, "atom")
add("atom-type-null",         "U8 *an_p = NULL;", True, "atom")

# ====================================================================
# CATEGORY: expressions — more combinations
# ====================================================================
add("expr-prec-mul-add",      "I64 e_pma = 1 + 2 * 3;", True, "expr")
add("expr-prec-mul-add-paren","I64 e_pmap = (1 + 2) * 3;", True, "expr")
add("expr-prec-and-or",       "I64 e_pao = 1 && 0 || 1;", True, "expr")
add("expr-prec-cmp-and",      "I64 e_pca = 1 < 2 && 3 > 2;", True, "expr")
add("expr-deep-paren",        "I64 e_dp = (((1 + 2)));", True, "expr")
add("expr-double-deref",      "I64 e_ddv=1; I64 *e_ddp=&e_ddv; I64 **e_ddpp=&e_ddp; I64 e_ddr=**e_ddpp;", True, "expr")
add("expr-array-index",       "I64 e_ai_a[3] = {1,2,3}; I64 e_ai_v = e_ai_a[1];", True, "expr")
add("expr-multi-array-index", "I64 e_mai_a[2][2]; e_mai_a[0][0] = 5;", True, "expr")
add("expr-member-access",     "class CExA { I64 x; I64 y; }; CExA e_ma_v; e_ma_v.x = 1;", True, "expr")
add("expr-arrow-access",      "class CExB { I64 x; }; CExB e_arr_v; CExB *e_arr_p = &e_arr_v; e_arr_p->x = 1;", True, "expr")
add("expr-assign-chain",      "I64 e_ac_a, e_ac_b, e_ac_c; e_ac_a = e_ac_b = e_ac_c = 0;", True, "expr")
add("expr-comma-in-call",     "I64 ECC_f(I64 a, I64 b) { return a+b; }\nI64 e_ccv = ECC_f(1, 2);", True, "expr")
add("expr-string-concat-args",'"%d\\n", 5;', True, "expr")
add("expr-power-right-assoc", "I64 e_pra = 2 ` 2 ` 3;", True, "expr")
add("expr-neg-power",         "I64 e_np = -2 ` 2;", True, "expr")
add("expr-bitops-mix",        "I64 e_bom = (0xF0 | 0x0F) & 0xFF;", True, "expr")
add("expr-shift-mix",         "I64 e_sm = (1 << 8) >> 4;", True, "expr")
add("expr-cmp-mixed-eq",      "I64 e_cme = (1 == 1) && (2 != 3);", True, "expr")

# ====================================================================
# CATEGORY: statements — more
# ====================================================================
add("stmt-nested-if",         "U0 SE_nif() { I64 a=1; if (a) if (a==1) a=2; else a=3; }", True, "stmt")
add("stmt-nested-while",      "U0 SE_nwh() { I64 i=0,j=0; while (i<3) { while (j<3) j++; i++; } }", True, "stmt")
add("stmt-nested-for",        "U0 SE_nfor() { I64 i,j; for (i=0;i<3;i++) for (j=0;j<3;j++) ; }", True, "stmt")
add("stmt-while-block",       "U0 SE_wb() { I64 i=0; while (i<3) { i++; i--; i++; } }", True, "stmt")
add("stmt-if-return",         "I64 SE_ifret(I64 x) { if (x>0) return 1; return 0; }", True, "stmt")
add("stmt-if-break",          "U0 SE_ifbr() { while (1) { if (1) break; } }", True, "stmt")
add("stmt-if-goto",           "U0 SE_ifgo() { I64 x=0; if (x) goto SE_IFGO_l; SE_IFGO_l: ; }", True, "stmt")
# SURPRISE: switch with only `default:` (no `case`) -> "switch range error".
# TempleOS requires at least one case to compute lo/hi range.
add("surprise-switch-only-default","U0 SE_swe() { I64 x=0; switch (x) { default: break; } }", False, "surprises")
add("stmt-try-catch-throw",   "U0 SE_tct() { try { throw('Test'); } catch { ; } }", True, "stmt")
add("stmt-asm-multi-ins",     "U0 SE_asm2() { asm { NOP; NOP; NOP; } }", True, "stmt")
add("stmt-lock-block",        'I64 SE_lb_v=0; U0 SE_lb() { lock { SE_lb_v++; SE_lb_v--; } }', True, "stmt")
add("stmt-for-empty-init",    "U0 SE_fei() { I64 i=0; for (; i<3; i++) ; }", True, "stmt")
add("stmt-for-empty-step",    "U0 SE_fes() { I64 i; for (i=0; i<3;) i++; }", True, "stmt")

# ====================================================================
# CATEGORY: declarations — more
# ====================================================================
add("decl-class-empty",       "class CDeclE {};", True, "decl")
add("decl-class-pointers",    "class CDeclF { U8 *name; I64 *list; };", True, "decl")
add("decl-class-array-member","class CDeclG { I64 buf[16]; };", True, "decl")
add("decl-multi-var-types",   "I64 d_mva = 1, d_mvb = 2, d_mvc = 3;", True, "decl")
add("decl-string-init",       'U8 *d_si_s = "hello";', True, "decl")
add("decl-array-3d",          "I64 d_a3[2][3][4];", True, "decl")
add("decl-fn-pointer-arg",    "U0 d_fpa(U8 *p) { ; }", True, "decl")
add("decl-fn-recursive",      "I64 d_rec(I64 n) { if (n<=1) return 1; return n * d_rec(n-1); }", True, "decl")
add("decl-fn-multi-args",     "I64 d_ma(I64 a, I64 b, I64 c, I64 d) { return a+b+c+d; }", True, "decl")
add("decl-fn-mix-types",      "F64 d_mt(I64 a, F64 b) { return a + b; }", True, "decl")
add("decl-static-var-fn",     "I64 d_svf() { static I64 ctr=0; ctr++; return ctr; }", True, "decl")
add("decl-class-nested",      "class CDeclH { I64 a; }; class CDeclI { CDeclH inner; I64 b; };", True, "decl")
# Function modifier `_extern` form (declared elsewhere)
add("decl-fn-with-default-2", "I64 d_dfn(I64 a, I64 b = 7) { return a + b; }", True, "decl")
add("decl-public-class",      "public class CDeclJ { I64 x; };", True, "decl")

# ====================================================================
# CATEGORY: more error cases — distinct messages
# ====================================================================
add("err-invalid-lval-literal","U0 ELL_f() { 5 += 1; }", False, "errors")
# SURPRISE: TempleOS's incremental ExePutS appears to accept the snippet
# even though the closing `}` is missing — likely the partial fn defn is
# held until the next chunk would extend it. We reclassify as surprise.
add("surprise-missing-rbrace-fn","U0 EMR_f() { I64 a=1;", True, "surprises")
add("err-missing-rparen-call","U0 EMRC_f() { } I64 EMRC_g = 1; EMRC_f(", False, "errors")
add("err-undef-goto",         "U0 EUG_f() { goto EUG_nowhere; }", False, "errors")
add("err-redeclare-var",      "U0 ERV_f() { I64 erv_a=1; I64 erv_a=2; }", False, "errors")
add("err-bad-token",          "I64 ebt = @;", False, "errors")
add("err-throw-outside-try",  "I64 e_throw_no_try = 1;", True, "errors")  # actually fine at parse time
add("err-case-outside-switch","U0 ECOS_f() { case 1: ; }", False, "errors")
# SURPRISE: `dft: ;` outside a switch is parsed as an ordinary
# label-decl with a no-op body — `dft` isn't reserved at function scope,
# only meaningful inside `PrsSwitch`. Move to surprises.
add("surprise-default-outside-switch","U0 EDOS_f() { dft: ; }", True, "surprises")
# SURPRISE: class body without trailing `;` after the last member is
# accepted by TempleOS. The `;` between members is required, but the
# one before `}` is optional — and the one *after* `}` is optional too
# at top level (since the daemon's incremental parse just treats the
# remaining tokens as the next statement).
add("surprise-class-missing-semi","class CErr1 { I64 x }", True, "surprises")

# remove the throw-outside one — that's not actually an error
# (we'll handle by not adding it). Recreate clean list:

# ====================================================================
# CATEGORY: bug-compat — more §5 cases
# ====================================================================
# §5.3 — `if (cond) static I64 x;` at file scope: `static` global is
# created unconditionally. Note that the daemon executes top-level
# code, so the static slot is created.
# This is hard to demonstrate as a "fail" — TempleOS accepts it but the
# semantics are surprising. We classify as bug-compat-passing with a
# note in surprises.md.
add("bug53-if-static-decl",   "I64 b53_cond = 0; if (b53_cond) static I64 b53_x;", True, "bug-compat")
# §5.8 — Bt parses as a fun call (so it works as a callable form)
add("bug58-bt-as-call",       "I64 b58_v = 5; I64 b58_r = Bt(&b58_v, 0);", True, "bug-compat")
# §5.10 — default arg evaluated at parse time. Construct a counter.
add("bug510-default-arg-once","I64 b510_count = 0;\nI64 B510_g() { b510_count++; return b510_count; }\nU0 B510_f(I64 x = B510_g()) { ;}", True, "bug-compat")
# §5.11 — switch range too big should LexExcept
add("bug511-switch-range-huge","U0 B511_f() { I64 x=0; switch (x) { case 0...0x20000: break; default: break; } }", False, "bug-compat")
# §5.12 — case auto-increment from no expression — same as switch-case-auto, which passed
# §5.13 — implicit Print at top level, ; ends arg list
add("bug513-implicit-print",  '"hello %d\\n", 5;', True, "bug-compat")


def main():
    PASS.mkdir(parents=True, exist_ok=True)
    FAIL.mkdir(parents=True, exist_ok=True)
    pi = fi = 0
    cats = {}
    for short, body, ok, cat in ENTRIES:
        target = PASS if ok else FAIL
        i = pi if ok else fi
        fname = f"{i:03d}-{cat}-{short}.hc"
        if not body.endswith("\n"):
            body = body + "\n"
        (target / fname).write_text(body)
        cats.setdefault(cat, [0,0])
        cats[cat][0 if ok else 1] += 1
        if ok: pi += 1
        else:  fi += 1
    print(f"Wrote {pi} passing + {fi} failing snippets")
    print("By category (pass/fail):")
    for c in sorted(cats):
        p,f = cats[c]
        print(f"  {c}: {p} pass / {f} fail")


if __name__ == "__main__":
    main()
