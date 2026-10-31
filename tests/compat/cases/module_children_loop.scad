module d() { for (i = [0:$children - 1]) translate([i, 0, 0]) children(i); }
d() { cube(); sphere(); }
