// three orthogonal holes
difference() {
  cube(10, center = true);
  for (r = [[0, 0, 0], [90, 0, 0], [0, 90, 0]])
    rotate(r) cylinder(h = 12, r = 2.5, center = true, $fn = 16);
}
