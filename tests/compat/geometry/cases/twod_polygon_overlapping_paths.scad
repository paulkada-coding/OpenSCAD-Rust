// two overlapping paths: the overlap becomes a hole
linear_extrude(1)
  polygon(points = [[0, 0], [6, 0], [6, 6], [0, 6], [3, 3], [9, 3], [9, 9], [3, 9]],
          paths = [[0, 1, 2, 3], [4, 5, 6, 7]]);
