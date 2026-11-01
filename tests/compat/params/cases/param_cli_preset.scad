// compat-args: -p preset_fixture.json -P clamped
num_range = 25; // [0:0.5:20]
num_enum  = 10; // [10, 20, 30]
flag      = true;
label     = "ROSSO"; // 4
mode      = "a"; // [a, b]
vec3      = [1, 2, 3]; // [-5:5]
/* [Hidden] */
secret    = 99;

module demo() {
    echo(num_range, num_enum, flag, label, mode, vec3, secret);
}
demo();
