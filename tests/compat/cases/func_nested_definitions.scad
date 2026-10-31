module mo() {
  function nf(a) = a + 100;
  module nm() echo("nested module");
  echo(nf(1));
  nm();
}
mo();
echo(nf(1));
nm();
