function tail(n, acc = 0) = n == 0 ? acc : tail(n - 1, acc + 1);
echo("before");
echo(tail(1000000));
echo("after");
