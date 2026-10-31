g = 1;
module h() {
  echo("g in h", g);
  g = 10;
  echo("g after", g);
}
h();
echo("g top", g);
