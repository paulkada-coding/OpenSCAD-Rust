// Parametric box with holes.
width = 40;
depth = 30;
height = 15;
wall = 2;
hole_d = 3;
hole_inset = 5;
module box_shell() {
  difference() {
    cube([width, depth, height]);
    translate([wall, wall, wall]) cube([width - 2 * wall, depth - 2 * wall, height]);
  }
}
module holes() {
  for (x = [hole_inset, width - hole_inset], y = [hole_inset, depth - hole_inset])
    translate([x, y, -1]) cylinder(d = hole_d, h = wall + 2, $fn = 16);
}
difference() {
  box_shell();
  holes();
}
echo("box volume", width * depth * height);
