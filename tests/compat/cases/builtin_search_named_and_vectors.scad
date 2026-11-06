// search(): arguments by position even when named; vector keys.
l = [[2,"foo"],[4,"bar"],[3,"baz"]];
echo(1, search(["bar"], l, num_returns_per_match=1, index_col_num=1), search(["bar"], l, 0, 1));
echo(2, search(match_value=[2], string_or_vector=[1,2]), search([2], [1,2], index_col_num=0));
echo(3, search(num_returns_per_match=0, match_value=[2], string_or_vector=[2,2]));
st = [[5,3],[[5,4],3],["a",1]];
echo(4, search([[5,4]], st), search([5], st), search([[5,4]], st, 0));
echo(5, search([[1]], [[1,5],[2]]), search([[5,4]], [[5,4],[1]]), search([[5,4]], [[[5,4],3]]));
echo(6, search([[1,5]], [[1,5],[2]]), search([[1]], [1, [1], [[1]]]), search([[1,2]], [[1,2,3],[[1,2]]], 0));
echo(7, search([[5,4]], [[[5,4],3]], 0, 0), search([[3]], [[[5,4],3]], 0, 1), search([3], [[[5,4],3]], 0, 1));
echo(8, search([[1]], [[1]]), search([[1]], [[[1]]]), search([[1]], [[[1],7]]));
