// cube with a through hole
difference() {
  cube(10, center = true);
  cylinder(h = 12, r = 3, center = true, $fn = 20);
}
