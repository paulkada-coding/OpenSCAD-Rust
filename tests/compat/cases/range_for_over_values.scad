for (i = 3) echo("scalar", i);
for (i = "ab") echo("str", i);
for (i = undef) echo("undef", i);
for (i = [1, [2, 3]]) echo("vec", i);
for (i = []) echo("empty", i);
for (i = true) echo("bool", i);
