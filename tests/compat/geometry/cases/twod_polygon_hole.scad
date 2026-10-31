// polygon with an outer path and a hole
linear_extrude(2)
  polygon(points = [[0, 0], [10, 0], [10, 10], [0, 10], [3, 3], [7, 3], [7, 7], [3, 7]],
          paths = [[0, 1, 2, 3], [4, 5, 6, 7]]);
