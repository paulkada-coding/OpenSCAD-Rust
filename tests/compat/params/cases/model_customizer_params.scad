// Customizer-style parameters with annotations.
/* [Dimensions] */
// Length of the part
length = 50; // [10:1:100]
// Shape
shape = "round"; // [round, square]
/* [Hidden] */
eps = 0.01;
if (shape == "round")
  cylinder(d = length / 2, h = 5, $fn = 32);
else
  cube([length, length / 2, 5]);
echo(length = length, shape = shape);
