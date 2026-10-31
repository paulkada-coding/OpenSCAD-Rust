// 2D primitives side by side
linear_extrude(2) {
  square([4, 3], center = true);
  translate([5, 0]) circle(1.5, $fn = 12);
}
