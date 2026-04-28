U0 ScanRange(I64 start, I64 end) { return; }
U0 F() {
  I64 start = 0;
  I64 reg = 1;
  reg I64 hint = 2;
  switch (start) {
    start:
      case 1: start = 5; break;
    end:
      case 2: reg = hint; break;
  }
  ScanRange(start, reg);
}
