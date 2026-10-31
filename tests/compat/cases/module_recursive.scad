module tree(n) {
  if (n > 0) {
    cube(n);
    translate([0, 0, n]) tree(n - 1);
  }
}
tree(3);
