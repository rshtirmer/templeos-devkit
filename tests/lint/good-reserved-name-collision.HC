// good: parameters use non-reserved names. `tol` is the canonical
// rename for what would have been `eps`; `radius` stands in for
// what could have been `pi`. References to the actual built-in
// constants (`pi` etc.) inside the body are fine.
U0 ASSERT_NEAR(F64 actual, F64 expected, F64 tol, U8 *name) {
  F64 d = actual - expected;
  if (d < 0.0) d = -d;
  if (d <= tol) Print("ok\n");
}

U0 Circle(F64 radius) {
  // referencing pi (the constant) is fine
  F64 area = pi * radius * radius;
  Print("%f\n", area);
}
