%cube();
#cube();
*cube();
translate([1, 0, 0]) %sphere();
module q() { #cube(); }
q();
#union() { cube(); #sphere(); }
%translate([1, 0, 0]) #cube();
difference() { cube(); %sphere(); }
module n() { translate([1, 0, 0]) { %cube(); #sphere(); } }
n();
