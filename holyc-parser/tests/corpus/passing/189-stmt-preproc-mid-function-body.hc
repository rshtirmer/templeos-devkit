U0 F() {
  #ifdef DEBUG
  Foo();
  #endif
  Bar();
  for (i = 0; i < 10; i++) {
    #ifdef BAR
    Baz();
    #endif
  }
}
