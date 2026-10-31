module e() { echo("children", $children); }
e();
e() cube();
e() { cube(); sphere(); }
e() { cube(); if (false) sphere(); for (i = [0:2]) cube(); echo("x"); }
