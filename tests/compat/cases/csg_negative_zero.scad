// Negative zero: printed as "-0" in CSG, as "0" in echo.
z = 0 * -1;
echo(z, [z], str(z));
translate([z, 0, 0]) cube();
cube([1, z, 1]);
polygon([[z, 0], [1, 0], [0, 1]]);
color([z, 0, 0]) cube();
