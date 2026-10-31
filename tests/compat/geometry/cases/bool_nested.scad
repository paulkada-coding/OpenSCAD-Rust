// nested booleans
difference() {
  union() {
    cube([20, 10, 4]);
    translate([10, 5, 0]) cylinder(h = 10, r = 4, $fn = 24);
  }
  translate([10, 5, -1]) cylinder(h = 12, r = 2, $fn = 24);
  for (x = [3, 17]) translate([x, 5, -1]) cylinder(h = 6, r = 1, $fn = 12);
}
