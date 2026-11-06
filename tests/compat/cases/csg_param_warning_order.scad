// A builtin module warns about its parameters before its children run.
module part(z) translate([0, 0, z]) {
  echo("child of translate");
  cube(z);
}
part(undef);
scale(undef) { echo("child of scale"); translate([0, 0, undef]) cube(); }
