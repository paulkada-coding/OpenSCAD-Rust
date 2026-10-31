// 2D minkowski
linear_extrude(1) minkowski() {
  square(4);
  circle(1, $fn = 8);
}
