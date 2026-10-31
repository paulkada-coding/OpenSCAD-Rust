// Matrix helpers used by real-world libraries.
function identity(n) = [for (i = [0:n - 1]) [for (j = [0:n - 1]) i == j ? 1 : 0]];
function transpose(m) = [for (j = [0:len(m[0]) - 1]) [for (i = [0:len(m) - 1]) m[i][j]]];
function rot_z(a) = [[cos(a), -sin(a), 0, 0], [sin(a), cos(a), 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]];
m = rot_z(30) * rot_z(60);
echo(identity(3));
echo(transpose([[1, 2, 3], [4, 5, 6]]));
echo(m);
multmatrix(m) cube(2);
