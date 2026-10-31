// imported mesh in transformations and booleans
difference() {
  translate([1, 0, 0]) scale(2) import("data/octahedron.stl");
  translate([1, 0, 0]) cube(3, center = true);
}
