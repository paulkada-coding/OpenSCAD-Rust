module branch(len, depth) {
  cylinder(h = len, r1 = len / 10, r2 = len / 14, $fn = 8);
  if (depth > 0)
    translate([0, 0, len])
      for (a = [-30, 30])
        rotate([a, 0, 0]) branch(len * 0.7, depth - 1);
}
branch(10, 2);
