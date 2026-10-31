// hull of four spheres
hull() {
  for (p = [[0, 0, 0], [10, 0, 0], [0, 8, 0], [3, 3, 6]]) translate(p) sphere(1, $fn = 12);
}
