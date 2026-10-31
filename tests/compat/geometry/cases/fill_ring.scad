// fill() removes the hole
linear_extrude(1) fill() difference() {
  circle(5, $fn = 24);
  circle(3, $fn = 24);
}
