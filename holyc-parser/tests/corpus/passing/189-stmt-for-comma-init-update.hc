U0 F() {
  I64 i;
  I64 j;
  I64 n = 10;
  for (i = 0, j = 10; i < n; i++, j--) {
    n = i + j;
  }
  for (i = 0, j = 0; i < n; i++) {
    n = i + j;
  }
  for (i = 0; i < n; i++, j--) {
    n = i + j;
  }
  for (i = 0; i < n; i++) {
    n = i;
  }
}
