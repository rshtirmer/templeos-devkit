// bad: parameter named `eps` collides with TempleOS's machine-epsilon
// compile-time constant. PrsType resolves `eps` to the constant value
// (bit pattern 0x3CB0000000000000) before allocating the parameter
// slot, then trips with "Expecting '*' at INT:..."
U0 ASSERT_NEAR(F64 actual, F64 expected, F64 eps, U8 *name) {
  Print("near");
}

// also bad: local var named `pi` shadows the F64 pi constant
U0 Compute() {
  F64 pi = 3.14;
  Print("%f\n", pi);
}
