// twist, scale and center together
linear_extrude(height = 6, center = true, twist = 45, scale = 0.5, slices = 5)
  polygon([[0, 0], [4, 0], [4, 1], [1, 1], [1, 3], [0, 3]]);
