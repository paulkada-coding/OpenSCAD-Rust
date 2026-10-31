module a() {
  echo("a", $parent_modules, parent_module(0), parent_module(1), $children);
  children();
}
module b() { a() echo("in b child", $parent_modules, parent_module(0)); }
b();
a() a();
translate([0, 0, 0]) a();
