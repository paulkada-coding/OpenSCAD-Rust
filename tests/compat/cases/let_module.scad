let (a = 1, b = a + 1) echo("let module", a, b);
let (c = 3) {
  echo("block", c);
  cube(c);
}
