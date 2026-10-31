t = [[0, 0], [1, 10], [2, 20], [5, 50]];
echo(lookup(1.5, t), lookup(-1, t), lookup(6, t), lookup(0, t), lookup(5, t), lookup(3, t));
echo(lookup(0.5, [[1, 10], [0, 0]]), lookup(1, [[1, 5]]), lookup(1, []));
