// sin/cos/tan are exact at multiples of 30/45/90 degrees.
echo(sin(30) == 0.5, sin(150) == 0.5, sin(-30) == -0.5, sin(210) == -0.5, sin(330) == -0.5);
echo(cos(60) == 0.5, cos(120) == -0.5, cos(240) == -0.5, cos(300) == 0.5);
echo(cos(90) == 0, sin(180) == 0, cos(270) == 0, sin(360) == 0, sin(390) == 0.5, sin(720) == 0);
echo(sin(45) == sqrt(2)/2, cos(45) == sqrt(0.5), tan(45) == 1, tan(135) == -1, tan(60) == sqrt(3));
echo(tan(90), tan(-90), tan(270), tan(30));
