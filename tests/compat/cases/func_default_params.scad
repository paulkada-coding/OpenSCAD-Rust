k = 3;
function dp(a, b = k * 2) = [a, b];
echo(dp(1));
module mdp(a, b = k) { k = 100; echo("mdp", a, b); }
mdp(1);
function df(a, b = a + 1, c = $q) = [a, b, c];
echo(df(1));
module dm() { $q = 9; echo(df(1)); }
dm();
x = 10;
function g(x = x) = x;
echo(g());
