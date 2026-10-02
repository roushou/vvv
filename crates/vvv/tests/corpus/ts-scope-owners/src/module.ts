import { imported } from './peer';
export {};
forward();
function forward() { return imported; }
var early = early;
let later = 1;
later;
let before = after;
const after = 2;
{
  nested();
  function nested() { return later; }
}
function overloads() {
  overloaded(1);
  function overloaded(value: number): number;
  function overloaded(value: string): string;
  function overloaded(value: number | string) { return value; }
}
class View {
  static {
    local;
    var local = 1;
    let lexical = local;
    lexical;
  }
  method() { return local; }
}
namespace Space {
  use();
  export function use() { return value; }
  var value = 1;
  value;
}
function assignments(entries: unknown[]) {
  let target = 0;
  for ({ value: target } of entries) { target; }
  for ([target] of entries) { target; }
}
function types() {
  let value: Shape;
  interface Shape { size: number; }
  type Alias = Shape;
  class Local { method(): Local { return new Local(); } }
  let instance: Local;
}
switch (later) {
  case 1:
    branch();
    function branch() { return later; }
    let switched = later; switched;
  default:
    switched;
}
const callback = ((value: number) => { return value; }) satisfies (value: number) => number;
const First = 1;
enum Order { First = 2, Second = First, Third = Fourth, Fourth = 4 }
First;
