// An error while evaluating an assignment adds `assignment to "x"`.
function f(a) = assert(a > 0, "neg") a;
module m() {
  y = 2;
  x = f(-1);
}
module n() m();
n();
