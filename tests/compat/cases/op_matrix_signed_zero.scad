// Products of vectors and matrices are summed from +0.
function sz(m) = [for (r = m) [for (x = r) x == 0 ? (1/x < 0 ? "-0" : "0") : "x"]];
echo(1, sz([[1,0],[0,1]] * [[-1, -0],[ -0, -1]]));
echo(2, sz([[-0, 1]] * [[1, 0], [0, -0]]), sz([[1,0,0]] * [[-0],[-0],[-0]]));
echo(3, 1/([-0, 0] * [1, -1]), 1/([-0] * [1]), 1/([[ -0 ]] * [1])[0]);
