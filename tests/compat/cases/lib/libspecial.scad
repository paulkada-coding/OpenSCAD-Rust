// Helper library for use_dollar_and_scope.
private_v = 5;
module show_q() echo("show_q", $q);
function get_q() = $q;
function uses_private() = private_v;
