// compat-args: --enable=object-function
o = object(a = 1, b = "x", c = [1, 2]);
echo(o, o.a, o["b"], has_key(o, "a"), has_key(o, "z"), is_object(o), len(o), o.z);
echo(object(o, d = 4), object([["k", 1], ["j", 2]]));
for (k = o) echo(k);
