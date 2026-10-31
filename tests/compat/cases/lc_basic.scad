echo([for (i = [0:3]) i * i]);
echo([for (i = [0:3]) if (i % 2) i]);
echo([for (i = [0:3]) if (i % 2) i else -i]);
echo([for (i = [0:2]) let (j = i * i) j]);
echo([for (x = [1, 2]) x, 9]);
