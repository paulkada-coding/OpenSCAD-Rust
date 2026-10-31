echo([if (true) 1, if (false) 2, 3]);
echo([for (i = [0:5]) if (i < 2) "a" else if (i < 4) "b" else "c"]);
echo([for (i = [0:3]) if (i % 2) if (i > 1) i]);
echo([(1), (each [1])]);
