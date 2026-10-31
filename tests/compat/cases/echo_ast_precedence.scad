// Printed expressions are fully parenthesized according to precedence.
echo(function() 2 ^ 3 ^ 4 % 5 * 6 / 7 - 8 + 9 < 10 <= 11 > 12 >= 13 == 14 != 15 && 16 || 17);
echo(function() (1 + 2) * 3);
echo(function() 1 | 2 & 3 << 4 >> 5 + ~6);
echo(function() 1 < 2 | 3, function() 1 == 2 | 3, function() 1 | 2 == 3);
echo(function() !a == b, function() -a ^ b, function() !a ^ b, function() ~a ^ b);
echo(function() -x + !y - -1 + +3);
echo(function() -(-x), function() !!x, function() -0, function() -(1));
