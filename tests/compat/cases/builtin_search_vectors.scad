echo(search(3, [1, 2, 3, 3]), search([3], [1, 2, 3, 3], 0), search([1, 9], [1, 2, 3]));
echo(search([[1, "x"]], [[1, "x"], [2, "y"]]), search("x", [["a", 1], ["x", 2]]));
echo(search(2, [["a", 1], ["x", 2]], 0, 1), search([1, 2], [[1, "a"], [2, "b"], [1, "c"]], 0));
echo(search(["b"], ["abc", "b", "xb"], 0));
