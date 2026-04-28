U0 ReadAt(I64 offset, I64 len) { return; }
U0 F() {
  I64 offset = 0;
  I64 hdr_off = offset(Foo.bar);
  offset = offset + hdr_off;
  ReadAt(offset, 16);
}
