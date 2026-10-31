// L bracket with chamfer and mounting holes.
t = 3;
l = 30;
w = 20;
module plate(size) cube(size);
module hole_at(p, d = 4) translate(p) rotate([90, 0, 0]) cylinder(d = d, h = 3 * t, center = true, $fn = 20);
difference() {
  union() {
    plate([l, w, t]);
    plate([t, w, l]);
    hull() {
      translate([t, 0, t]) cube([0.01, w, 8]);
      translate([t, 0, t]) cube([8, w, 0.01]);
    }
  }
  translate([l * 0.7, w / 2, 0]) rotate([0, 0, 0]) cylinder(d = 4, h = 3 * t, center = true, $fn = 20);
  hole_at([t / 2, w / 2, l * 0.7]);
}
