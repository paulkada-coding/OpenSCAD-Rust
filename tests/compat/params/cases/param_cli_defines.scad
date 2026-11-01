// compat-args: -D overridden=42 -D kept_header=7
overridden = 10; // [0:100]
untouched = 20; // [0:50]
module m() {
    cube([overridden, untouched, 1]);
}
m();
