// Limit: a range with 1000000 or more elements is rejected.
echo(len([for (i = [0:1:999998]) 0]));
echo(len([for (i = [0:1:999999]) 0]));
echo(len([for (i = [0:1e-9:1]) 0]));
x = [0:1:1e7];
echo(x);
