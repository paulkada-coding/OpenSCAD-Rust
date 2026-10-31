// Seeded rands is deterministic (std::mt19937 semantics).
echo(rands(0, 1, 3, 42));
echo(rands(0, 10, 4, 1), rands(0, 1, 2, 0), rands(-5, 5, 3, 123));
echo(rands(5, 5, 2, 1), rands(1, 0, 1, 1), rands(0, 1, 0, 1));
echo(rands(0, 1, 3, 42) == rands(0, 1, 3, 42), len(rands(0, 10, 5)));
