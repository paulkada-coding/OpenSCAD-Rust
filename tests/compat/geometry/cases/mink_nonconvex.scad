// minkowski with a non-convex operand (L-shaped prism and a cube).
// With a rounded second operand OpenSCAD's own output is not deterministic
// (convex parts are unioned in parallel), so a cube is used here.
minkowski() {
  linear_extrude(1) polygon([[0, 0], [6, 0], [6, 2], [2, 2], [2, 6], [0, 6]]);
  cube(0.5);
}
