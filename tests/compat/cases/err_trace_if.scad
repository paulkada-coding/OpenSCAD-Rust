// `called by 'if'` frames, at the condition.
module m() { if (true) { if (
  true) { assert(false, "f"); } } }
m();
