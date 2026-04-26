" syntax/holyc.vim — HolyC / ZealC syntax for Vim and Neovim.
"
" Linked to standard Vim highlight groups (Type, Keyword, String, etc.)
" so any colorscheme paints it sensibly. No colors are hardcoded.

if exists("b:current_syntax")
  finish
endif

syn case match

" -- comments ----------------------------------------------------------
syn keyword holycTodo contained TODO FIXME XXX HACK NOTE
syn match   holycLineComment  +//.*+ contains=holycTodo,@Spell
syn region  holycBlockComment start=+/\*+ end=+\*/+ contains=holycTodo,@Spell

" -- preprocessor ------------------------------------------------------
syn match   holycInclude   +^\s*#\s*include\>+ nextgroup=holycString skipwhite
syn match   holycPreProc   +^\s*#\s*\(define\|undef\|if\|ifdef\|ifndef\|else\|elif\|endif\|exe\|help_index\|help_file\|help_link\|asm\|pragma\|line\|error\|warning\)\>+

" -- strings & chars ---------------------------------------------------
" HolyC strings use C-style \X escapes AND DolDoc $$ for literal $.
syn match   holycEscape       +\\[abefnrtv\\"'?0]+ contained
syn match   holycEscape       +\\x[0-9A-Fa-f]\++ contained
syn match   holycEscape       +\\[0-7]\++ contained
syn match   holycDolDocEscape +\$\$+ contained
syn match   holycFormat       +%[-+0# ]*[0-9*]*\(\.[0-9*]\+\)\?[hljztL]\?[diouxXeEfgGaAcspn%]+ contained
syn region  holycString  start=+"+ skip=+\\"+ end=+"+ contains=holycEscape,holycDolDocEscape,holycFormat,@Spell
syn region  holycChar    start=+'+ skip=+\\'+ end=+'+ contains=holycEscape

" -- numbers -----------------------------------------------------------
syn match   holycHex     +\<0[xX][0-9A-Fa-f_]\+\>+
syn match   holycBinary  +\<0[bB][01_]\+\>+
syn match   holycFloat   +\<[0-9][0-9_]*\.[0-9_]\+\([eE][+-]\?[0-9]\+\)\?\>+
syn match   holycFloat   +\<[0-9][0-9_]*[eE][+-]\?[0-9]\+\>+
syn match   holycNumber  +\<[0-9][0-9_]*\>+

" -- constants ---------------------------------------------------------
syn keyword holycBoolean  TRUE FALSE
syn keyword holycConstant NULL ON OFF INVALID_PTR

" -- types -------------------------------------------------------------
syn keyword holycPrimType U0 U8 U16 U32 U64 I0 I8 I16 I32 I64 F64 Bool
" ZealOS class convention: identifier starts with C followed by capital.
syn match   holycClassType +\<C[A-Z][A-Za-z0-9_]*\>+

" -- storage modifiers -------------------------------------------------
syn keyword holycStorageClass extern _extern import _import public static
syn keyword holycStorageClass interrupt haserrcode argpop noargpop reg noreg
syn keyword holycStorageClass no_warn lastclass lock

" -- control flow & keywords -------------------------------------------
syn keyword holycConditional if else switch case default
syn keyword holycRepeat      while for do
syn keyword holycStatement   break continue goto return start end
syn keyword holycException   try catch throw
syn keyword holycKeyword     class union sizeof offset
syn keyword holycKeyword     asm ALIGN BINFILE DU8 DU16 DU32 DU64 ORG USE16 USE32 USE64

" -- builtins (this codebase + ZealOS stdlib subset) -------------------
syn keyword holycBuiltin Print PrintErr PutChars PutS GetChar GetS
syn keyword holycBuiltin DocPrint DocForm
syn keyword holycBuiltin MAlloc CAlloc Free MSize MemSet MemCpy MemCmp
syn keyword holycBuiltin StrLen StrCmp StrCpy StrCat StrPrint StrNew
syn keyword holycBuiltin ToUpper ToLower
syn keyword holycBuiltin Spawn Sys Once OnceExe Sleep Yield
syn keyword holycBuiltin TaskExe Exit Kill ExeFile ExePrint
syn keyword holycBuiltin JobsHandler RFlagsGet Bt Bts Btr
syn keyword holycBuiltin FileWrite FileRead FileFind Cd Dir
syn keyword holycBuiltin CommInit8n1 CommPrint CommPutChar CommPutS CommGetChar
syn keyword holycBuiltin FifoU8New FifoU8Del FifoU8Insert FifoU8Remove FifoU8Cnt
syn keyword holycBuiltin BlkDevNextFreeSlot BlkDevAdd AHCIPortInit
syn keyword holycBuiltin TCPSocketListen TCPSocketAccept TCPSocketBind TCPSocketClose
syn keyword holycBuiltin OutU8 OutU16 OutU32 InU8 InU16 InU32
syn keyword holycBuiltin ToI64 ToBool ToF64

" -- operators ---------------------------------------------------------
syn match   holycOperator +[-+*/%=<>!&|^~?:]+
syn match   holycOperator +->+
syn match   holycOperator +::+
syn match   holycOperator +`+

" -- linking to standard groups ----------------------------------------
hi def link holycTodo          Todo
hi def link holycLineComment   Comment
hi def link holycBlockComment  Comment

hi def link holycInclude       Include
hi def link holycPreProc       PreProc

hi def link holycString        String
hi def link holycChar          Character
hi def link holycEscape        SpecialChar
hi def link holycDolDocEscape  SpecialChar
hi def link holycFormat        SpecialChar

hi def link holycNumber        Number
hi def link holycHex           Number
hi def link holycBinary        Number
hi def link holycFloat         Float

hi def link holycBoolean       Boolean
hi def link holycConstant      Constant

hi def link holycPrimType      Type
hi def link holycClassType     Type
hi def link holycStorageClass  StorageClass

hi def link holycConditional   Conditional
hi def link holycRepeat        Repeat
hi def link holycStatement     Statement
hi def link holycException     Exception
hi def link holycKeyword       Keyword

hi def link holycBuiltin       Function

hi def link holycOperator      Operator

let b:current_syntax = "holyc"
