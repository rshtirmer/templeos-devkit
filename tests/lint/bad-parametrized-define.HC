// Parametrized #define is unreliable in HolyC under JIT push.
#define SQR(x) ((x) * (x))
#define MAX(a, b) ((a) > (b) ? (a) : (b))

U0 Demo() {
  Print("%d\n", SQR(3));
}
