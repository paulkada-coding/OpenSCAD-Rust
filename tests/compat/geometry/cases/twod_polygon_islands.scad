// two separate islands in one polygon
linear_extrude(1)
  polygon(points = [[0, 0], [3, 0], [0, 3], [5, 5], [8, 5], [8, 8]],
          paths = [[0, 1, 2], [3, 4, 5]]);
