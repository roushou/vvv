function script() {
  { function legacy() {} }
  legacy();
}
function strict() {
  'use strict';
  { inner(); function inner() {} }
}
root();
function root() {}
var repeated;
var repeated;
repeated;
