module m() {
  x = 1;
  echo("x", x);
  x = 2;
}
m();
module never() { y = 1; y = 2; }
