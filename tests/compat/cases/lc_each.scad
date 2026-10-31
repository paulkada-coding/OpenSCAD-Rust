echo([each [1, 2], each [3:5], each "ab", each 7]);
echo([for (i = [0:1]) each [i, i]]);
echo([each undef], [each []], [each [[1]]]);
