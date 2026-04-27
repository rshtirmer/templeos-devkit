" ftdetect/holyc.vim — register HolyC / ZealC filetypes.
"
" .ZC and .HC are HolyC source. .HH is HolyC header (the kernel uses both).
" Match case-insensitively because the on-disk convention is uppercase but
" the host filesystem is often case-insensitive (macOS) and we don't want
" to miss a stray .zc.

au BufNewFile,BufRead *.ZC,*.zc,*.HC,*.hc,*.HH,*.hh setfiletype holyc
