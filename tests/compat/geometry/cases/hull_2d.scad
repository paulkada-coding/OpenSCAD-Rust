// 2D hull
linear_extrude(1) hull() {
  circle(2, $fn = 16);
  translate([6, 0]) square(2);
}
