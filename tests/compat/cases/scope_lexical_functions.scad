k = 3;
function addk(x) = x + k;
module setk() {
  k = 100;
  echo("addk in module", addk(1), k);
}
setk();
echo("addk top", addk(1));
