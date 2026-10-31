// Plate with embossed label.
label = "OpenSCAD";
size = 6;
union() {
  cube([60, 15, 2]);
  translate([30, 7.5, 2])
    linear_extrude(1)
      text(label, size = size, halign = "center", valign = "center");
}
