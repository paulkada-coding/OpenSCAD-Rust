// Line breaks typed inside a string are not part of it; escapes are.
x = "a
  b";
echo(x, len(x));
y = "c
d";
echo(y, len(y));
echo(len("e\nf"), "g\th");
z = "g

h";
echo(len(z));
