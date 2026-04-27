// Split into separate decls. Function params (in parens) are fine.
U0 AddXY(F64 x, F64 y, F64 *out) {
  F64 a;
  F64 b;
  a = x;
  b = y;
  *out = a + b;
}
